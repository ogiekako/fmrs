//! PQ-based best-first scheduler over (seed, current_state) tasks.
//!
//! Replaces the per-seed `par_iter` loop when `--oracle-model` is given.
//! Each task represents a single seed's backward search; workers pop the
//! highest-scoring task, advance it by one step, recompute the oracle score,
//! and push it back. A task that finishes (Completed / MaxStep / EarlyExit)
//! is removed from the PQ and its terminal record is appended to the seed
//! result log.
//!
//! Concurrency model: `Mutex<BinaryHeap>` + `Condvar`. Workers block on the
//! condvar when the queue is empty but other workers are still active. The
//! scheduler shuts down when both PQ is empty and active count is zero.

use fmrs_core::{
    position::{position::PositionAux, BitBoard, Position, UndoMove},
    search::backward::BackwardSearch,
};
use rustc_hash::FxHashSet;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrd};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use super::super::smoke_constraints::{
    board_piece_count, satisfies_ideal_smoke_constraints,
    satisfies_ideal_smoke_generation_constraints, satisfies_ideal_smoke_undo_candidate,
    SearchConstraints,
};
use super::super::smoke_persistence::{
    append_seed_result_record, build_seed_result_record, condition_key,
    remove_seed_checkpoint, write_seed_checkpoint, SeedCheckpoint, SeedRunStats,
    TerminationReason,
};
use super::oracle::{OracleModel, StepRecord};
use super::search::log_global_best_if_improved;

/// Per-task state. `seeds` は canonical_digest が一致する seed のグループ
/// (canonicalize OFF では len=1)。
enum TaskState {
    Cold { seeds: Vec<PositionAux> },
    Active(BackwardSearch),
}

struct Task {
    seed_index: usize,
    seed_sfen: String,
    state: TaskState,
    history: Vec<StepRecord>,
    best_piece_count: u32,
    best_positions: Vec<PositionAux>,
    peak_frontier_size: usize,
    peak_memo_len: usize,
    score: f64,
    last_checkpoint_time: Option<Instant>,
}

impl Task {
    fn new_cold(seed_index: usize, seeds: Vec<PositionAux>, score: f64) -> Self {
        let seed_sfen = seeds[0].sfen();
        Task {
            seed_index,
            seed_sfen,
            state: TaskState::Cold { seeds },
            history: Vec::new(),
            best_piece_count: 0,
            best_positions: Vec::new(),
            peak_frontier_size: 0,
            peak_memo_len: 0,
            score,
            last_checkpoint_time: None,
        }
    }
}

// Max-heap by score; ties broken by seed_index for determinism.
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.seed_index == other.seed_index
    }
}
impl Eq for Task {}
impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.score.partial_cmp(&other.score) {
            Some(Ordering::Equal) | None => other.seed_index.cmp(&self.seed_index),
            Some(o) => o,
        }
    }
}

/// Shared scheduler state.
pub(super) struct Scheduler {
    pq: Mutex<BinaryHeap<Task>>,
    cond: Condvar,
    active: AtomicUsize,
    stop_signal: Arc<AtomicBool>,
}

impl Scheduler {
    fn new(stop_signal: Arc<AtomicBool>) -> Self {
        Self {
            pq: Mutex::new(BinaryHeap::new()),
            cond: Condvar::new(),
            active: AtomicUsize::new(0),
            stop_signal,
        }
    }

    fn push(&self, task: Task) {
        self.pq.lock().unwrap().push(task);
        self.cond.notify_one();
    }

    /// Pop the highest-priority task, blocking until one is available or
    /// until both PQ is empty and active count is zero.
    fn pop(&self) -> Option<Task> {
        let mut guard = self.pq.lock().unwrap();
        loop {
            if self.stop_signal.load(AtomicOrd::Relaxed) {
                if let Some(t) = guard.pop() {
                    self.active.fetch_add(1, AtomicOrd::Relaxed);
                    return Some(t);
                }
                return None;
            }
            if let Some(t) = guard.pop() {
                self.active.fetch_add(1, AtomicOrd::Relaxed);
                return Some(t);
            }
            if self.active.load(AtomicOrd::Relaxed) == 0 {
                return None;
            }
            guard = self.cond.wait(guard).unwrap();
        }
    }

    fn finish_active(&self) {
        self.active.fetch_sub(1, AtomicOrd::Relaxed);
        self.cond.notify_all();
    }
}

/// Per-step input shared across all workers.
pub(super) struct WorkerCtx<'a> {
    pub(super) constraints: SearchConstraints,
    pub(super) max_step: Option<u16>,
    pub(super) max_memo_entries: Option<usize>,
    pub(super) target_max: u32,
    pub(super) seed_result_log: &'a Mutex<File>,
    pub(super) seed_result_log_path: &'a Path,
    pub(super) trajectory_log: &'a Mutex<File>,
    pub(super) cond_hash: &'a str,
    pub(super) global_best_piece_count: &'a AtomicU64,
    pub(super) best: &'a Mutex<(u32, FxHashSet<String>, usize)>,
    pub(super) stop_signal: &'a AtomicBool,
    pub(super) canonicalize_attacker_goldish: bool,
    pub(super) checkpoint_interval_secs: u64,
}

enum StepOutcome {
    Continue,
    Done(TerminationReason),
}

/// Run one iteration of the search loop on `task`. Mirrors the body of the
/// existing `search_single_seed` loop but exits after a single advance so
/// the scheduler can re-rank between steps.
fn advance_one(task: &mut Task, ctx: &WorkerCtx<'_>) -> anyhow::Result<StepOutcome> {
    if ctx.stop_signal.load(AtomicOrd::Relaxed) {
        return Ok(StepOutcome::Done(TerminationReason::EarlyExit));
    }

    if matches!(task.state, TaskState::Cold { .. }) {
        let seeds = match std::mem::replace(
            &mut task.state,
            TaskState::Cold { seeds: Vec::new() },
        ) {
            TaskState::Cold { seeds } => seeds,
            _ => unreachable!(),
        };
        let mut search = if ctx.canonicalize_attacker_goldish {
            match BackwardSearch::new_canonical_group(&seeds, 1) {
                Ok(s) => s,
                Err(_) => {
                    task.state = TaskState::Cold { seeds };
                    return Ok(StepOutcome::Done(TerminationReason::Unknown));
                }
            }
        } else {
            match BackwardSearch::new_with_parallel(&seeds[0], false, 1, false) {
                Ok(s) => s,
                Err(_) => {
                    task.state = TaskState::Cold { seeds };
                    return Ok(StepOutcome::Done(TerminationReason::Unknown));
                }
            }
        };
        if let Some(limit) = ctx.max_memo_entries {
            search.set_memo_entry_limit(Some(limit));
        }
        search.set_canonicalize_attacker_goldish(ctx.canonicalize_attacker_goldish);
        let stats = search.stats();
        track_peaks_from_stats(task, stats.positions_len, stats.memo_len);
        task.state = TaskState::Active(search);
    }

    let search = match &mut task.state {
        TaskState::Active(s) => s,
        _ => unreachable!(),
    };

    // Output / best update on odd steps (and step 0 for completeness).
    let cur_step = search.step();
    if cur_step == 0 || cur_step % 2 == 1 {
        let (step, positions) = search.output_positions(true, false)?;
        if step > 0 && ctx.max_step.is_none_or(|limit| step <= limit) {
            let mut improved = false;
            let prev_len = task.best_positions.len();
            for position in positions {
                if !satisfies_ideal_smoke_constraints(&position, step, ctx.constraints) {
                    continue;
                }
                let pc = board_piece_count(&position);
                if pc > task.best_piece_count {
                    task.best_piece_count = pc;
                    task.best_positions.clear();
                    improved = true;
                }
                if pc == task.best_piece_count {
                    task.best_positions.push(position);
                }
            }
            let positions_grew = task.best_positions.len() > prev_len;
            if (improved || positions_grew) && task.best_piece_count >= 8 {
                if let Some(p) = task.best_positions.first() {
                    let url = p.sfen_url();
                    log_global_best_if_improved(
                        ctx.global_best_piece_count,
                        task.seed_index,
                        task.best_piece_count,
                        task.best_positions.len(),
                        &url,
                        search.stats(),
                    );
                }
            }
            if task.best_piece_count >= ctx.target_max
                && !ctx.stop_signal.swap(true, AtomicOrd::Relaxed)
            {
                eprintln!(
                    "early_exit: target_max={} reached by seed={} (pieces={})",
                    ctx.target_max, task.seed_index, task.best_piece_count
                );
            }
        }
    }

    if ctx.stop_signal.load(AtomicOrd::Relaxed) {
        return Ok(StepOutcome::Done(TerminationReason::EarlyExit));
    }

    let search_limit = ctx.max_step.map(|l| {
        if l % 2 == 0 {
            l.saturating_sub(1)
        } else {
            l
        }
    });
    if search_limit.is_some_and(|l| search.step() >= l) {
        return Ok(StepOutcome::Done(TerminationReason::MaxStep));
    }
    let next_step = search.step() + 1;
    if search_limit.is_some_and(|l| next_step > l) {
        return Ok(StepOutcome::Done(TerminationReason::MaxStep));
    }

    let advance_start = Instant::now();
    let constraints = ctx.constraints;
    let candidate_filter = move |p: &PositionAux, u: &UndoMove| {
        satisfies_ideal_smoke_undo_candidate(p, u, next_step, constraints)
    };
    let generation_filter = move |c: &Position, s: Option<BitBoard>| {
        let p = PositionAux::new(c.clone(), s);
        satisfies_ideal_smoke_generation_constraints(&p, next_step, constraints)
    };
    search.set_parallel(1);
    let advanced = search.advance_upto_with_candidate_filter(
        usize::MAX / 2,
        candidate_filter,
        generation_filter,
    )?;
    let elapsed_ms = advance_start.elapsed().as_millis();

    if !advanced {
        return Ok(StepOutcome::Done(TerminationReason::Completed));
    }

    let stats = search.stats();
    if stats.positions_len > task.peak_frontier_size {
        task.peak_frontier_size = stats.positions_len;
    }
    if stats.memo_len > task.peak_memo_len {
        task.peak_memo_len = stats.memo_len;
    }
    let record = StepRecord {
        step: stats.step,
        frontier: stats.positions_len,
        memo: stats.memo_len,
        inner: 1,
        ms: elapsed_ms,
    };
    task.history.push(record);
    emit_trajectory_row(ctx.trajectory_log, ctx.cond_hash, task.seed_index, &record);

    // Persist checkpoint throttled by interval so large-frontier searches
    // don't saturate disk I/O on big instances.
    let checkpoint_interval =
        std::time::Duration::from_secs(ctx.checkpoint_interval_secs);
    let should_checkpoint = match task.last_checkpoint_time {
        None => true,
        Some(t) => t.elapsed() >= checkpoint_interval,
    };
    if should_checkpoint {
        let _ = write_seed_checkpoint(
            ctx.seed_result_log_path,
            &SeedCheckpoint {
                seed_index: task.seed_index,
                seed_sfen: task.seed_sfen.clone(),
                max_step: ctx.max_step,
                max_frontier: None,
                constraints: ctx.constraints,
                resume_state: search.resume_state_header(),
                best_piece_count: task.best_piece_count,
                best_sfens: vec![],
                canonicalize_attacker_goldish: ctx.canonicalize_attacker_goldish,
                frontier_bytes: search.frontier_to_binary(),
                best_position_bytes: task
                    .best_positions
                    .iter()
                    .flat_map(|p| p.to_bytes())
                    .collect(),
            },
        );
        task.last_checkpoint_time = Some(Instant::now());
    }

    Ok(StepOutcome::Continue)
}

/// Update peak counters on `task` from current `search` stats.
/// Caller must guarantee that `search` borrow doesn't alias the peak fields,
/// which is true because they are disjoint struct fields.
fn track_peaks_from_stats(task: &mut Task, positions_len: usize, memo_len: usize) {
    if positions_len > task.peak_frontier_size {
        task.peak_frontier_size = positions_len;
    }
    if memo_len > task.peak_memo_len {
        task.peak_memo_len = memo_len;
    }
}

fn emit_trajectory_row(
    log: &Mutex<File>,
    cond_hash: &str,
    seed_index: usize,
    record: &StepRecord,
) {
    let mut file = log.lock().unwrap();
    let _ = writeln!(
        file,
        r#"{{"cond":"{cond}","seed":{seed},"step":{step},"frontier":{frontier},"memo":{memo},"inner":{inner},"ms":{ms}}}"#,
        cond = cond_hash,
        seed = seed_index,
        step = record.step,
        frontier = record.frontier,
        memo = record.memo,
        inner = record.inner,
        ms = record.ms,
    );
}

fn finalize_task(task: &mut Task, reason: TerminationReason, ctx: &WorkerCtx<'_>) -> anyhow::Result<()> {
    // Push observed peaks to capture state at termination, in case the last
    // advance bumped them.
    let post_stats = match &task.state {
        TaskState::Active(s) => Some(s.stats()),
        _ => None,
    };
    if let Some(s) = post_stats {
        track_peaks_from_stats(task, s.positions_len, s.memo_len);
    }

    let final_step = match &task.state {
        TaskState::Active(s) => s.stats().step,
        TaskState::Cold { .. } => 0,
    };
    let final_seen = match &task.state {
        TaskState::Active(s) => s.stats().seen_positions as u64,
        TaskState::Cold { .. } => 0,
    };

    let stats = SeedRunStats {
        peak_frontier_size: task.peak_frontier_size,
        peak_memo_len: task.peak_memo_len,
        total_seen_positions: final_seen,
        terminal_step: final_step,
        termination_reason: reason,
    };

    let best = if task.best_positions.is_empty() {
        None
    } else if ctx.canonicalize_attacker_goldish {
        // canonicalize 適用時の false positive を非正規化版 standard_solve で除外。
        let verified: Vec<PositionAux> = std::mem::take(&mut task.best_positions)
            .into_iter()
            .filter(|p| {
                fmrs_core::solve::standard_solve::standard_solve(p.clone(), 2, true)
                    .map(|r| r.solutions().len() == 1)
                    .unwrap_or(false)
            })
            .collect();
        if verified.is_empty() {
            None
        } else {
            Some((task.best_piece_count, verified))
        }
    } else {
        Some((task.best_piece_count, std::mem::take(&mut task.best_positions)))
    };

    // Update global best.
    if let Some((pc, ref positions)) = best {
        let mut guard = ctx.best.lock().unwrap();
        guard.2 += 1;
        if pc > guard.0 {
            guard.0 = pc;
            guard.1.clear();
        }
        if pc == guard.0 {
            for p in positions {
                guard.1.insert(p.sfen());
            }
        }
    }

    // EarlyExit + partial best → keep checkpoint, skip record (see ideal_backward.rs).
    let early_exited_partial = reason == TerminationReason::EarlyExit
        && best.as_ref().is_none_or(|(pc, _)| *pc < ctx.target_max);

    if !early_exited_partial {
        // Construct a fresh PositionAux from the seed sfen for record building.
        // Cheaper alternative: keep the seed in Task. We do that.
        // Fall through.
    }

    if !early_exited_partial {
        // We need a PositionAux to build the record. Reconstitute from SFEN.
        if let Ok(seed_pos) = PositionAux::from_sfen(&task.seed_sfen) {
            let record = build_seed_result_record(
                task.seed_index,
                &seed_pos,
                ctx.max_step,
                ctx.constraints,
                &best,
                stats,
                ctx.canonicalize_attacker_goldish,
            );
            append_seed_result_record(&mut ctx.seed_result_log.lock().unwrap(), record)?;
            remove_seed_checkpoint(
                ctx.seed_result_log_path,
                task.seed_index,
                ctx.max_step,
                ctx.constraints,
                ctx.canonicalize_attacker_goldish,
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(super) fn run_with_oracle(
    seeds: Vec<(usize, Vec<PositionAux>)>,
    constraints: SearchConstraints,
    max_step: Option<u16>,
    max_memo_entries: Option<usize>,
    parallel: usize,
    seed_result_log_path: PathBuf,
    seed_result_log: Mutex<File>,
    trajectory_log: Mutex<File>,
    oracle: OracleModel,
    target_max: u32,
    stop_signal: Arc<AtomicBool>,
    initial_best: (u32, FxHashSet<String>, usize),
    canonicalize_attacker_goldish: bool,
    checkpoint_interval_secs: u64,
) -> anyhow::Result<(u32, FxHashSet<String>, usize)> {
    let cond_hash = condition_key(max_step, constraints);
    let scheduler = Arc::new(Scheduler::new(stop_signal.clone()));
    let global_best_piece_count = AtomicU64::new(0);
    let best = Mutex::new(initial_best);
    let cold_score = oracle.cold_score();

    eprintln!(
        "scheduler: parallel={} pending_seeds={} oracle_intercept={:.3}",
        parallel,
        seeds.len(),
        cold_score
    );

    for (idx, group) in seeds {
        scheduler.push(Task::new_cold(idx, group, cold_score));
    }

    let oracle = Arc::new(oracle);
    let cond_hash = Arc::new(cond_hash);

    std::thread::scope(|s| -> anyhow::Result<()> {
        let mut handles = Vec::with_capacity(parallel);
        for _worker in 0..parallel {
            let scheduler = scheduler.clone();
            let oracle = oracle.clone();
            let cond_hash = cond_hash.clone();
            let stop_signal = stop_signal.clone();
            let seed_result_log = &seed_result_log;
            let trajectory_log = &trajectory_log;
            let seed_result_log_path = &seed_result_log_path;
            let global_best_piece_count = &global_best_piece_count;
            let best = &best;
            handles.push(s.spawn(move || -> anyhow::Result<()> {
                let ctx = WorkerCtx {
                    constraints,
                    max_step,
                    max_memo_entries,
                    target_max,
                    seed_result_log,
                    seed_result_log_path: seed_result_log_path.as_path(),
                    trajectory_log,
                    cond_hash: &cond_hash,
                    global_best_piece_count,
                    best,
                    stop_signal: &stop_signal,
                    canonicalize_attacker_goldish,
                    checkpoint_interval_secs,
                };
                while let Some(mut task) = scheduler.pop() {
                    let outcome = advance_one(&mut task, &ctx);
                    match outcome {
                        Ok(StepOutcome::Continue) => {
                            task.score = oracle.score(&task.history);
                            scheduler.push(task);
                            scheduler.finish_active();
                        }
                        Ok(StepOutcome::Done(reason)) => {
                            finalize_task(&mut task, reason, &ctx)?;
                            scheduler.finish_active();
                        }
                        Err(e) => {
                            scheduler.finish_active();
                            return Err(e);
                        }
                    }
                }
                Ok(())
            }));
        }
        let mut first_err: Option<anyhow::Error> = None;
        for h in handles {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
                Err(_) => {
                    if first_err.is_none() {
                        first_err = Some(anyhow::anyhow!("worker panicked"));
                    }
                }
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        Ok(())
    })?;

    Ok(best.into_inner().unwrap())
}

