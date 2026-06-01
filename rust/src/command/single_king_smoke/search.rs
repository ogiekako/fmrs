use fmrs_core::{
    position::{position::PositionAux, UndoMove},
    search::backward::{BackwardSearch, BackwardSearchStats},
};
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Frontier size threshold below which inner-parallel advance is skipped.
/// par_chunks dispatch overhead exceeds the work for small frontiers; this
/// threshold gates the path switch between single-threaded
/// `advance_upto_with_candidate_filter` and parallel
/// `advance_parallel_filtered`.
///
/// Inner parallel uses the outer rayon pool (pool=None → par_chunks inherits
/// the caller's pool context), so cross-seed work-stealing is possible even
/// when all `parallel` threads are busy: a thread at its own join point can
/// steal chunks from a concurrent seed's phase-1/phase-2. This benefit kicks
/// in whenever frontier is large enough to justify the dispatch overhead,
/// regardless of how many seeds are still in flight.
const FRONTIER_PARALLEL_THRESHOLD: usize = 1024;

use super::super::smoke_constraints::{
    board_piece_count, satisfies_ideal_smoke_constraints,
    satisfies_ideal_smoke_generation_constraints, satisfies_ideal_smoke_undo_candidate,
    SearchConstraints,
};
use super::super::smoke_persistence::{
    load_seed_checkpoint, load_split_progress, remove_split_progress, write_seed_checkpoint,
    write_split_progress, SeedCheckpoint, SeedRunStats, SplitProgress, TerminationReason,
};
use super::beam::{apply_beam, sample_features_to_log, BeamConfig};
use super::ideal_backward::SplitConfig;
use super::system::{MemoryBudget, ProcStatus, SearchStatsDisplay};

pub(super) struct SingleSeedResult {
    pub(super) best: Option<(u32, u16, Vec<PositionAux>)>, // (piece_count, step, positions)
    pub(super) stats: SeedRunStats,
    /// `true` if beam pruning reduced the frontier at least once during this run.
    /// When `false`, the result is exact even if `--beam-width` was specified.
    pub(super) beam_filtered: bool,
}

/// Zero result for the "could not initialize this seed" paths (empty seed list,
/// build failure). `best: None`, all stats zero.
fn zero_seed_result() -> SingleSeedResult {
    SingleSeedResult {
        best: None,
        stats: SeedRunStats {
            peak_frontier_size: 0,
            peak_memo_len: 0,
            total_seen_positions: 0,
            terminal_step: 0,
            termination_reason: TerminationReason::Unknown,
        },
        beam_filtered: false,
    }
}

/// Immutable per-seed configuration shared by the BFS loop (`run_seed_loop`) and
/// the split driver (`run_split`). Bundled so the loop can be reused across the
/// non-split run, the split prefix, and each split chunk without threading ~25
/// arguments through every call.
pub(super) struct SeedLoopCtx<'a> {
    seed_index: usize,
    representative_sfen: String,
    max_step: Option<u16>,
    max_memo_entries: Option<usize>,
    constraints: SearchConstraints,
    parallel: usize,
    total_pending: usize,
    completed_in_run: &'a AtomicUsize,
    mem_trace: bool,
    global_best_piece_count: &'a AtomicU64,
    seed_result_log_path: &'a Path,
    feature_log: Option<&'a Mutex<fs::File>>,
    feature_samples_per_step: usize,
    beam: &'a BeamConfig,
    candidates_pool_factor: usize,
    max_candidates_pool: Option<usize>,
    memory_budget: MemoryBudget,
    target_max: u32,
    early_exit: bool,
    stop_signal: &'a AtomicBool,
    trajectory_log: &'a Mutex<fs::File>,
    cond_hash: &'a str,
    canonicalize_attacker_goldish: bool,
    checkpoint_interval_secs: u64,
}

/// Loop-state seed values handed to `run_seed_loop`. For a fresh/non-split run
/// (or split prefix) these come from the checkpoint restore; for a split chunk
/// they are empty/default.
struct LoopInit {
    best_piece_count: u32,
    best_step: u16,
    best_positions: Vec<PositionAux>,
    adaptive_pool_factor: usize,
    ema_inv_survival: Option<f64>,
}

/// Result of one `run_seed_loop` invocation: the best found plus terminal stats.
struct LoopOutcome {
    best_piece_count: u32,
    best_step: u16,
    best_positions: Vec<PositionAux>,
    peak_frontier_size: usize,
    peak_memo_len: usize,
    total_seen_positions: u64,
    terminal_step: u16,
    termination_reason: TerminationReason,
    did_beam_filter: bool,
    /// `true` when the loop stopped because it reached `stop_at_step` (split
    /// prefix hand-off) rather than terminating naturally.
    split_reached: bool,
}

/// Hard upper bound on the Bottom-K candidate pool factor. See the call sites in
/// `run_seed_loop` for the priority order between the static
/// `--max-candidates-pool` ceiling, the `--memory-budget-pct` dynamic ceiling,
/// and the legacy 8×W fallback.
fn compute_max_pool_factor(
    width: usize,
    candidates_pool_factor: usize,
    max_candidates_pool: Option<usize>,
    memory_budget: MemoryBudget,
) -> usize {
    if width == 0 {
        return candidates_pool_factor;
    }
    let static_ceiling = max_candidates_pool
        .map(|cap| (cap / width).max(candidates_pool_factor))
        .unwrap_or(usize::MAX);
    let budget_ceiling = memory_budget.pool_factor_ceiling(width);
    let combined = if budget_ceiling > 0 && static_ceiling != usize::MAX {
        budget_ceiling.min(static_ceiling)
    } else if budget_ceiling > 0 {
        budget_ceiling
    } else if static_ceiling != usize::MAX {
        static_ceiling
    } else {
        // No budget probe and no --max-candidates-pool → legacy 8×W.
        candidates_pool_factor.max(8)
    };
    combined.max(candidates_pool_factor)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search_single_seed(
    seed_index: usize,
    seeds: &[PositionAux],
    max_step: Option<u16>,
    max_memo_entries: Option<usize>,
    constraints: SearchConstraints,
    parallel: usize,
    total_pending: usize,
    completed_in_run: &AtomicUsize,
    mem_trace: bool,
    global_best_piece_count: &AtomicU64,
    seed_result_log_path: &Path,
    feature_log: Option<&Mutex<fs::File>>,
    feature_samples_per_step: usize,
    beam: &BeamConfig,
    candidates_pool_factor: usize,
    max_candidates_pool: Option<usize>,
    memory_budget: MemoryBudget,
    target_max: u32,
    early_exit: bool,
    stop_signal: &AtomicBool,
    trajectory_log: &Mutex<fs::File>,
    cond_hash: &str,
    canonicalize_attacker_goldish: bool,
    checkpoint_interval_secs: u64,
    split: SplitConfig,
) -> anyhow::Result<SingleSeedResult> {
    if seeds.is_empty() {
        return Ok(zero_seed_result());
    }
    let representative = &seeds[0];
    let ctx = SeedLoopCtx {
        seed_index,
        representative_sfen: representative.sfen(),
        max_step,
        max_memo_entries,
        constraints,
        parallel,
        total_pending,
        completed_in_run,
        mem_trace,
        global_best_piece_count,
        seed_result_log_path,
        feature_log,
        feature_samples_per_step,
        beam,
        candidates_pool_factor,
        max_candidates_pool,
        memory_budget,
        target_max,
        early_exit,
        stop_signal,
        trajectory_log,
        cond_hash,
        canonicalize_attacker_goldish,
        checkpoint_interval_secs,
    };

    // canonicalize ON/OFF は path suffix + record フィールドで隔離されており、
    // 同 flag 同士の resume が可能。
    // 書き込みは beam.width.is_none() でガードしているので、.ckpt にあるのは必ず
    // 非 beam の厳密計算途中の frontier+memo。beam モードでもそれを load して
    // step N まで exact, それ以降だけ beam で進めるほうが、step 0 から beam で
    // 走るより必ず良い (memo hit 率も上がる)。
    let checkpoint = load_seed_checkpoint(
        seed_result_log_path,
        seed_index,
        &representative.sfen(),
        max_step,
        constraints,
        canonicalize_attacker_goldish,
    );

    let mut search = if canonicalize_attacker_goldish {
        let resumed = checkpoint.as_ref().and_then(|cp| {
            if !cp.frontier_bytes.is_empty() {
                BackwardSearch::from_resume_state_canonical_group_with_frontier_bytes(
                    &cp.resume_state,
                    &cp.frontier_bytes,
                    seeds,
                    1,
                )
                .ok()
            } else {
                BackwardSearch::from_resume_state_canonical_group(&cp.resume_state, seeds, 1).ok()
            }
        });
        let result = match resumed {
            Some(s) => Ok(s),
            None => BackwardSearch::new_canonical_group(seeds, 1),
        };
        match result {
            Ok(s) => s,
            Err(_) => {
                return Ok(zero_seed_result());
            }
        }
    } else {
        let resumed = checkpoint.as_ref().and_then(|cp| {
            if !cp.frontier_bytes.is_empty() {
                BackwardSearch::from_resume_state_with_frontier_bytes(
                    &cp.resume_state,
                    &cp.frontier_bytes,
                    1,
                )
                .ok()
            } else {
                BackwardSearch::from_resume_state(&cp.resume_state, 1).ok()
            }
        });
        match resumed
            .or_else(|| BackwardSearch::new_with_parallel(representative, false, 1, false).ok())
        {
            Some(s) => s,
            None => {
                return Ok(zero_seed_result());
            }
        }
    };
    if let Some(limit) = max_memo_entries {
        search.set_memo_entry_limit(Some(limit));
    }
    // When beam is active, bound Phase-1 candidates via Bottom-K Sampling.
    // pool_factor is the per-shard overshoot (W → W × factor / NUM_SHARDS):
    // Phase V can early-stop at W survivors as long as survival rate s ≥
    // 1/factor. We start at the user-given factor and grow it adaptively
    // based on observed survival, clamped by max_candidates_pool for OOM
    // safety.
    if let Some(width) = beam.width {
        search.set_candidates_limit(Some(width));
        search.set_candidates_pool_factor(candidates_pool_factor);
    }
    // Initial computation used by checkpoint restoration's clamp. Recomputed
    // each step inside the adaptive loop so it follows live RSS.
    let max_pool_factor = compute_max_pool_factor(
        beam.width.unwrap_or(0),
        candidates_pool_factor,
        max_candidates_pool,
        memory_budget,
    );
    // Currently-applied pool factor. Starts at the user value; tracked via an
    // EMA of 1/survival so step-to-step noise is smoothed out, then mapped
    // back to a target factor each step.
    let mut adaptive_pool_factor = candidates_pool_factor;
    let default_pool_factor = candidates_pool_factor;
    let mut ema_inv_survival: Option<f64> = None;
    // Restore adaptation state from checkpoint if present. This avoids a
    // cold-start (pool_factor = default) right after resume — which would
    // shrink |next| for several steps until the EMA caught up, and in the
    // worst case never recover because the smaller frontier feeds a smaller
    // mid in subsequent steps. Clamp to [default, max] in case CLI flags
    // changed since the checkpoint was written.
    if let (Some(cp), Some(_)) = (checkpoint.as_ref(), beam.width) {
        if let Some(pf) = cp.adaptive_pool_factor {
            adaptive_pool_factor = pf.clamp(default_pool_factor, max_pool_factor);
            search.set_candidates_pool_factor(adaptive_pool_factor);
        }
        if let Some(ema) = cp.ema_inv_survival {
            if ema.is_finite() && ema > 0.0 {
                ema_inv_survival = Some(ema);
            }
        }
    }
    search.set_delta_trace(mem_trace);
    search.set_canonicalize_attacker_goldish(canonicalize_attacker_goldish);
    mt(
        mem_trace,
        seed_index,
        &search,
        format_args!("start resumed={}", checkpoint.is_some()),
    );
    let mut best_piece_count = 0u32;
    // Output step at which `best_positions` were found. `best` is the
    // lexicographic max of `(best_piece_count, best_step)`.
    let mut best_step: u16 = 0;
    let mut best_positions: Vec<PositionAux> = vec![];
    if let Some(ref cp) = checkpoint {
        best_piece_count = cp.best_piece_count;
        best_step = cp.best_step;
        best_positions = if !cp.best_position_bytes.is_empty() {
            cp.best_position_bytes
                .chunks_exact(105)
                .map(|chunk| PositionAux::from_bytes(chunk.try_into().unwrap()))
                .collect()
        } else {
            cp.best_sfens
                .iter()
                .filter_map(|sfen| PositionAux::from_sfen(sfen).ok())
                .collect()
        };
    }

    let init = LoopInit {
        best_piece_count,
        best_step,
        best_positions,
        adaptive_pool_factor,
        ema_inv_survival,
    };

    // Split mode: run the prefix to the split step, then process the frontier in
    // bounded chunks one at a time. Beam/oracle are rejected upstream so the
    // adaptive-pool / candidate-limit machinery is inert here.
    if split.enabled() {
        return run_split(&ctx, seeds, split, search, init);
    }

    let outcome = run_seed_loop(&mut search, &ctx, init, None, true)?;

    mt(
        mem_trace,
        seed_index,
        &search,
        format_args!(
            "before_drop best_pieces={} positions={}",
            outcome.best_piece_count,
            outcome.best_positions.len()
        ),
    );
    drop(search);
    if mem_trace {
        eprintln!(
            "mem_trace seed={} after_drop best_pieces={} positions={} {}",
            seed_index,
            outcome.best_piece_count,
            outcome.best_positions.len(),
            ProcStatus::current()
        );
    }

    let best = finalize_seed_best(
        outcome.best_piece_count,
        outcome.best_step,
        outcome.best_positions,
        canonicalize_attacker_goldish,
    );
    Ok(SingleSeedResult {
        best,
        stats: SeedRunStats {
            peak_frontier_size: outcome.peak_frontier_size,
            peak_memo_len: outcome.peak_memo_len,
            total_seen_positions: outcome.total_seen_positions,
            terminal_step: outcome.terminal_step,
            termination_reason: outcome.termination_reason,
        },
        beam_filtered: outcome.did_beam_filter,
    })
}

/// Post-process the raw best positions into the returned `(pc, step, positions)`.
/// With canonicalize ON, false positives (unique under canonicalization but
/// non-unique / non-mate in the original position) are filtered out by re-running
/// `standard_solve` and keeping only genuinely unique-solution positions.
fn finalize_seed_best(
    best_piece_count: u32,
    best_step: u16,
    best_positions: Vec<PositionAux>,
    canonicalize_attacker_goldish: bool,
) -> Option<(u32, u16, Vec<PositionAux>)> {
    if best_positions.is_empty() {
        None
    } else if canonicalize_attacker_goldish {
        let verified: Vec<PositionAux> = best_positions
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
            Some((best_piece_count, best_step, verified))
        }
    } else {
        // 全 best positions を返す。output 集計側で SFEN HashSet で uniq 化されるので
        // 出力 line 数 = unique best position 数。テストでも実体ある count を見たい。
        Some((best_piece_count, best_step, best_positions))
    }
}

/// Merge a `(pc, step, positions)` candidate into the running accumulator using
/// the (#pieces, steps) lexicographic-max rule with union-on-tie, deduped by
/// digest. Used to fold split chunk results into one best.
fn merge_best(
    acc_pc: &mut u32,
    acc_step: &mut u16,
    acc_positions: &mut Vec<PositionAux>,
    pc: u32,
    step: u16,
    positions: Vec<PositionAux>,
) {
    use std::cmp::Ordering;
    if positions.is_empty() {
        return;
    }
    match (pc, step).cmp(&(*acc_pc, *acc_step)) {
        Ordering::Greater => {
            *acc_pc = pc;
            *acc_step = step;
            *acc_positions = positions;
        }
        Ordering::Equal => {
            acc_positions.extend(positions);
        }
        Ordering::Less => return,
    }
    let mut seen = fmrs_core::nohash::NoHashSet64::default();
    acc_positions.retain(|p| seen.insert(p.digest()));
}

/// Split-mode driver. `search`/`init` are the already-built prefix search/state.
/// Runs the prefix to the split step, snapshots and deterministically chunks the
/// frontier, then runs each chunk's BFS to completion sequentially, merging best
/// results and persisting chunk-granularity progress for resume.
fn run_split(
    ctx: &SeedLoopCtx,
    seeds: &[PositionAux],
    split: SplitConfig,
    mut search: BackwardSearch,
    init: LoopInit,
) -> anyhow::Result<SingleSeedResult> {
    let split_start_step = split.start_step.expect("split.enabled() checked");
    let chunk_size = split.chunk_size.expect("validated in ideal_backward");

    let mut peak_frontier = 0usize;
    let mut peak_memo = 0usize;
    let mut total_seen = 0u64;
    let mut terminal_step = 0u16;
    let mut termination_reason;

    // Prefix: exact BFS up to the split step (or natural termination).
    let prefix = run_seed_loop(&mut search, ctx, init, Some(split_start_step), true)?;
    let mut acc_pc = prefix.best_piece_count;
    let mut acc_step = prefix.best_step;
    let mut acc_positions = prefix.best_positions;
    peak_frontier = peak_frontier.max(prefix.peak_frontier_size);
    peak_memo = peak_memo.max(prefix.peak_memo_len);
    total_seen += prefix.total_seen_positions;
    terminal_step = terminal_step.max(prefix.terminal_step);
    termination_reason = prefix.termination_reason;

    if !prefix.split_reached {
        // Search ended (Completed / MaxStep / EarlyExit) before reaching the
        // split step — no chunks to process, prefix best is the answer.
        drop(search);
        let best =
            finalize_seed_best(acc_pc, acc_step, acc_positions, ctx.canonicalize_attacker_goldish);
        return Ok(SingleSeedResult {
            best,
            stats: SeedRunStats {
                peak_frontier_size: peak_frontier,
                peak_memo_len: peak_memo,
                total_seen_positions: total_seen,
                terminal_step,
                termination_reason,
            },
            beam_filtered: false,
        });
    }

    // Snapshot the frontier F at the split step, then release the prefix search.
    let header = search.resume_state_header();
    let frontier_bytes = search.frontier_to_binary();
    drop(search);

    // Canonicalize F's order (the frontier vector order is not stable across
    // parallel runs/resume) so shuffle(seed) yields identical chunk boundaries
    // every time, then shuffle deterministically and chunk.
    let mut frontier: Vec<[u8; 88]> = frontier_bytes
        .chunks_exact(88)
        .map(|c| c.try_into().unwrap())
        .collect();
    drop(frontier_bytes);
    frontier.sort_unstable();
    let mut rng = SmallRng::seed_from_u64(split.seed);
    frontier.shuffle(&mut rng);
    let num_chunks = frontier.len().div_ceil(chunk_size);

    // Resume: skip already-completed chunks and restore the accumulated best.
    let mut next_chunk = 0usize;
    if let Some(p) = load_split_progress(
        ctx.seed_result_log_path,
        ctx.seed_index,
        &ctx.representative_sfen,
        ctx.max_step,
        ctx.constraints,
        ctx.canonicalize_attacker_goldish,
    ) {
        if p.split_start_step == split_start_step
            && p.split_chunk_size == chunk_size
            && p.split_seed == split.seed
            && p.num_chunks == num_chunks
        {
            next_chunk = p.next_chunk.min(num_chunks);
            acc_pc = p.best_piece_count;
            acc_step = p.best_step;
            acc_positions = p
                .best_position_bytes
                .chunks_exact(105)
                .map(|c| PositionAux::from_bytes(c.try_into().unwrap()))
                .collect();
        }
    }

    eprintln!(
        "split seed={} start_step={} frontier={} chunk_size={} chunks={} resume_from_chunk={}",
        ctx.seed_index, split_start_step, frontier.len(), chunk_size, num_chunks, next_chunk
    );

    for chunk_index in next_chunk..num_chunks {
        if ctx.stop_signal.load(Ordering::Relaxed) {
            termination_reason = TerminationReason::EarlyExit;
            break;
        }
        let start = chunk_index * chunk_size;
        let end = ((chunk_index + 1) * chunk_size).min(frontier.len());
        let chunk_flat: Vec<u8> = frontier[start..end].iter().flatten().copied().collect();

        let mut chunk_search = if ctx.canonicalize_attacker_goldish {
            BackwardSearch::from_resume_state_canonical_group_with_frontier_bytes(
                &header,
                &chunk_flat,
                seeds,
                1,
            )?
        } else {
            BackwardSearch::from_resume_state_with_frontier_bytes(&header, &chunk_flat, 1)?
        };
        if let Some(limit) = ctx.max_memo_entries {
            chunk_search.set_memo_entry_limit(Some(limit));
        }
        chunk_search.set_delta_trace(ctx.mem_trace);
        chunk_search.set_canonicalize_attacker_goldish(ctx.canonicalize_attacker_goldish);

        let chunk_init = LoopInit {
            best_piece_count: 0,
            best_step: 0,
            best_positions: vec![],
            adaptive_pool_factor: ctx.candidates_pool_factor,
            ema_inv_survival: None,
        };
        // Per-chunk checkpointing is disabled (allow_step_checkpoint=false); a
        // crash re-runs at most one chunk (bounded by chunk_size). Chunk
        // boundaries / progress are persisted via SplitProgress below.
        let out = run_seed_loop(&mut chunk_search, ctx, chunk_init, None, false)?;
        drop(chunk_search);

        merge_best(
            &mut acc_pc,
            &mut acc_step,
            &mut acc_positions,
            out.best_piece_count,
            out.best_step,
            out.best_positions,
        );
        peak_frontier = peak_frontier.max(out.peak_frontier_size);
        peak_memo = peak_memo.max(out.peak_memo_len);
        total_seen += out.total_seen_positions;
        terminal_step = terminal_step.max(out.terminal_step);
        termination_reason = out.termination_reason;

        let _ = write_split_progress(
            ctx.seed_result_log_path,
            &SplitProgress {
                seed_index: ctx.seed_index,
                seed_sfen: ctx.representative_sfen.clone(),
                max_step: ctx.max_step,
                constraints: ctx.constraints,
                canonicalize_attacker_goldish: ctx.canonicalize_attacker_goldish,
                split_start_step,
                split_chunk_size: chunk_size,
                split_seed: split.seed,
                num_chunks,
                next_chunk: chunk_index + 1,
                best_piece_count: acc_pc,
                best_step: acc_step,
                best_position_bytes: acc_positions.iter().flat_map(|p| p.to_bytes()).collect(),
            },
        );
    }

    // Full completion: drop the marker. (The caller separately removes any prefix
    // checkpoint and appends the seed-result record.)
    if !ctx.stop_signal.load(Ordering::Relaxed) {
        remove_split_progress(
            ctx.seed_result_log_path,
            ctx.seed_index,
            ctx.max_step,
            ctx.constraints,
            ctx.canonicalize_attacker_goldish,
        );
    }

    let best =
        finalize_seed_best(acc_pc, acc_step, acc_positions, ctx.canonicalize_attacker_goldish);
    Ok(SingleSeedResult {
        best,
        stats: SeedRunStats {
            peak_frontier_size: peak_frontier,
            peak_memo_len: peak_memo,
            total_seen_positions: total_seen,
            terminal_step,
            termination_reason,
        },
        beam_filtered: false,
    })
}

/// The smoke backward-search BFS loop over an already-built `search`. Runs from
/// the search's current step until natural termination (Completed / MaxStep /
/// EarlyExit) or, when `stop_at_step` is set, until the frontier reaches that
/// step (split prefix hand-off — `LoopOutcome::split_reached` is then true and
/// the frontier is left at the split step for the caller to snapshot).
/// Per-step checkpoint writes are gated by `allow_step_checkpoint`.
fn run_seed_loop(
    search: &mut BackwardSearch,
    ctx: &SeedLoopCtx,
    init: LoopInit,
    stop_at_step: Option<u16>,
    allow_step_checkpoint: bool,
) -> anyhow::Result<LoopOutcome> {
    let &SeedLoopCtx {
        seed_index,
        ref representative_sfen,
        max_step,
        max_memo_entries,
        constraints,
        parallel,
        total_pending,
        completed_in_run,
        mem_trace,
        global_best_piece_count,
        seed_result_log_path,
        feature_log,
        feature_samples_per_step,
        beam,
        candidates_pool_factor,
        max_candidates_pool,
        memory_budget,
        target_max,
        early_exit,
        stop_signal,
        trajectory_log,
        cond_hash,
        canonicalize_attacker_goldish,
        checkpoint_interval_secs,
    } = ctx;

    let mut best_piece_count = init.best_piece_count;
    let mut best_step = init.best_step;
    let mut best_positions = init.best_positions;
    let mut adaptive_pool_factor = init.adaptive_pool_factor;
    let mut ema_inv_survival = init.ema_inv_survival;
    let default_pool_factor = candidates_pool_factor;
    /// Minimum mid size for an observation to count toward the EMA. Below
    /// this, the ratio next/mid is too noisy (e.g. mid=3, next=1 → s=0.33
    /// would jerk the factor around).
    const MIN_MID_FOR_OBSERVATION: usize = 100;
    /// EMA decay: weight on the previous value. Higher = smoother but slower
    /// to react to genuine survival changes.
    const EMA_ALPHA: f64 = 0.7;
    /// Safety margin on the target factor (pool ≈ safety × W / s).
    const POOL_SAFETY: f64 = 2.0;
    // Track the most recently applied dynamic memo limit so we only re-apply
    // when the per-seed budget grows (dropping `remaining` releases budget to
    // surviving seeds). `max_memo_entries` is the per-seed budget at peak
    // parallelism (`remaining = parallel`); total budget = base * parallel.
    let mut applied_memo_limit = max_memo_entries;

    let search_limit = max_step.map(|limit| {
        if limit % 2 == 0 {
            limit.saturating_sub(1)
        } else {
            limit
        }
    });
    // Per-seed terminal stats. Peaks are accumulated across the loop;
    // termination_reason is overwritten at the break that actually fires.
    let mut peak_frontier_size: usize = 0;
    let mut peak_memo_len: usize = 0;
    // 全 break 経路で上書き済み; loop が他経路で抜けないことの fallback として Unknown。
    #[allow(unused_assignments)]
    let mut termination_reason = TerminationReason::Unknown;
    let mut split_reached = false;
    // Checkpoint throttle: track when we last wrote so we don't checkpoint
    // every step on large-frontier searches (which generates huge I/O at scale).
    let checkpoint_interval = std::time::Duration::from_secs(checkpoint_interval_secs);
    let mut last_checkpoint_time: Option<Instant> = None;
    // Trajectory buffer: accumulate rows per-seed, flush once at the end to
    // avoid a mutex acquisition on every step across all parallel seeds.
    let mut trajectory_buf = String::new();
    // True once beam pruning actually reduced the frontier. While false, the
    // search is exact even when --beam-width is set.
    let mut did_beam_filter = false;
    let track_peaks =
        |peak_frontier_size: &mut usize, peak_memo_len: &mut usize, search: &BackwardSearch| {
            let s = search.stats();
            if s.positions_len > *peak_frontier_size {
                *peak_frontier_size = s.positions_len;
            }
            if s.memo_len > *peak_memo_len {
                *peak_memo_len = s.memo_len;
            }
        };
    track_peaks(&mut peak_frontier_size, &mut peak_memo_len, search);

    loop {
        if stop_signal.load(Ordering::Relaxed) {
            termination_reason = TerminationReason::EarlyExit;
            break;
        }
        // Split prefix hand-off: stop once the frontier reaches the split step,
        // leaving it in place for the caller to snapshot. Snaps to the first
        // search step >= stop (smoke advances in odd steps).
        if let Some(stop) = stop_at_step {
            if search.step() >= stop {
                split_reached = true;
                break;
            }
        }
        if search.step() == 0 || search.step() % 2 == 1 {
            let output_start = Instant::now();
            let (step, positions) = search.output_positions(true, false)?;
            let output_raw_positions = positions.len();
            if step > 0 && max_step.is_none_or(|limit| step <= limit) {
                // 仕様: LR canonicalization は seed 生成 (実行最初の final
                // positions 列挙) のみで使用。逆算中の出力 filter では使わない。
                let filtered = positions
                    .into_iter()
                    .filter(|position| {
                        satisfies_ideal_smoke_constraints(position, step, constraints)
                    })
                    .collect::<Vec<_>>();
                let filtered_len = filtered.len();
                let prev_positions_len = best_positions.len();
                let mut improved = false;
                for position in filtered {
                    let pc = board_piece_count(&position);
                    // best = (#pieces, steps) の辞書順最大。より大きい (pc, step)
                    // を見つけたら現在の best をすべて捨てる。
                    if (pc, step) > (best_piece_count, best_step) {
                        best_piece_count = pc;
                        best_step = step;
                        best_positions.clear();
                        improved = true;
                    }
                    if (pc, step) == (best_piece_count, best_step) {
                        best_positions.push(position);
                    }
                }
                if improved {
                    // dedup_positions 不要: improved 直前に best_positions.clear()
                    // しており、push されるのは単一の output_positions の filtered
                    // 結果のみ。output_positions は frontier (一意) に基づくので
                    // 重複は発生しない。
                    debug_assert!(
                        {
                            let mut seen = fmrs_core::nohash::NoHashSet64::default();
                            best_positions.iter().all(|p| seen.insert(p.digest()))
                        },
                        "best_positions has duplicates after improvement"
                    );
                }
                let positions_increased = best_positions.len() > prev_positions_len;
                if (improved || positions_increased) && best_piece_count >= 8 {
                    let url = best_positions[0].sfen_url();
                    let stats = search.stats();
                    log_global_best_if_improved(
                        global_best_piece_count,
                        seed_index,
                        best_piece_count,
                        best_step,
                        best_positions.len(),
                        &url,
                        stats,
                    );
                }
                if early_exit
                    && best_piece_count >= target_max
                    && !stop_signal.swap(true, Ordering::Relaxed)
                {
                    eprintln!(
                        "early_exit: target_max={} reached by seed={} (pieces={})",
                        target_max, seed_index, best_piece_count
                    );
                }
                mt(
                    mem_trace,
                    seed_index,
                    search,
                    format_args!(
                        "output step={} raw={} filtered={} elapsed_ms={}",
                        step,
                        output_raw_positions,
                        filtered_len,
                        output_start.elapsed().as_millis()
                    ),
                );
            } else {
                mt(
                    mem_trace,
                    seed_index,
                    search,
                    format_args!(
                        "output step={} raw={} filtered=skipped elapsed_ms={}",
                        step,
                        output_raw_positions,
                        output_start.elapsed().as_millis()
                    ),
                );
            }
        } else {
            // Even-step black output reconstructs the previous odd frontier from
            // white positions. For ideal smoke best tracking, the odd frontier was
            // already observed directly.
            mt(
                mem_trace,
                seed_index,
                search,
                format_args!("output skipped_even_search_step={}", search.step()),
            );
        }

        if beam.width.is_none() {
            if let Some(log) = feature_log {
                sample_features_to_log(log, feature_samples_per_step, seed_index, search);
            }
        }

        if search_limit.is_some_and(|limit| search.step() >= limit) {
            termination_reason = TerminationReason::MaxStep;
            break;
        }
        // Smoke outputs live on odd steps. From an odd step we do one fused
        // 2-ply advance (`advance_2ply_fused`, N→N+2): the intermediate
        // (white/even) ply is smoke-filtered (to stay bounded) but NOT
        // uniqueness-verified and never materialised as a Vec; the output ply
        // is filtered + verified (source of truth). From an even step (incl.
        // the step-0 bootstrap) we do a single 1-ply advance to reach odd
        // parity.
        let step_now = search.step();
        let two_ply = step_now % 2 == 1;
        let next_step = if two_ply {
            step_now + 2
        } else {
            step_now + 1
        };
        if search_limit.is_some_and(|limit| next_step > limit) {
            termination_reason = TerminationReason::MaxStep;
            break;
        }

        if let Some(width) = beam.width {
            did_beam_filter |= apply_beam(search, beam, width);
        }

        let advance_start = Instant::now();
        // Dynamic inner-parallel: divide the pool budget across seeds still
        // in flight (this seed itself is included in `remaining`). When the
        // tail shrinks, surviving seeds inherit the freed cores.
        let remaining = total_pending
            .saturating_sub(completed_in_run.load(Ordering::Relaxed))
            .max(1);
        let dynamic_inner = ((parallel + remaining - 1) / remaining).max(1);

        let frontier = search.stats().positions_len;
        let use_inner_parallel = frontier >= FRONTIER_PARALLEL_THRESHOLD;
        search.set_parallel(if use_inner_parallel { dynamic_inner } else { 1 });
        // Dynamic memo budget: as `remaining` drops, the surviving seed gets
        // a larger share of the total memo budget. Only grow (never shrink)
        // since shrinking would require evicting hot entries the seed is
        // already using.
        if let Some(base) = max_memo_entries {
            let dynamic_limit = base.saturating_mul(parallel) / remaining;
            if applied_memo_limit.is_none_or(|cur| dynamic_limit > cur) {
                search.set_memo_entry_limit_lazy(Some(dynamic_limit));
                applied_memo_limit = Some(dynamic_limit);
            }
        }
        let advanced = if two_ply {
            // Fused 2-ply (frontier N -> N+2). Mid ply filtered at step_now+1
            // (bounds the set) but NOT uniqueness-verified; out ply at
            // next_step is filtered + verified (source of truth). Same method
            // whether parallel or not — set_parallel(1) above for small
            // frontiers makes the inner par_chunks effectively serial.
            let mid_step = step_now + 1;
            let mid_candidate_filter = |position: &PositionAux, undo_move: &UndoMove| {
                satisfies_ideal_smoke_undo_candidate(position, undo_move, mid_step, constraints)
            };
            let mid_generation_filter = |position: &PositionAux| {
                satisfies_ideal_smoke_generation_constraints(position, mid_step, constraints)
            };
            let out_candidate_filter = |position: &PositionAux, undo_move: &UndoMove| {
                satisfies_ideal_smoke_undo_candidate(position, undo_move, next_step, constraints)
            };
            let out_generation_filter = |position: &PositionAux| {
                satisfies_ideal_smoke_generation_constraints(position, next_step, constraints)
            };
            search.advance_2ply_fused(
                &mid_candidate_filter,
                &mid_generation_filter,
                &out_candidate_filter,
                &out_generation_filter,
            )?
        } else {
            let candidate_filter = |position: &PositionAux, undo_move: &UndoMove| {
                satisfies_ideal_smoke_undo_candidate(position, undo_move, next_step, constraints)
            };
            let generation_filter = |position: &PositionAux| {
                satisfies_ideal_smoke_generation_constraints(position, next_step, constraints)
            };
            if use_inner_parallel {
                search.advance_parallel_filtered(&candidate_filter, &generation_filter)?
            } else {
                search.advance_upto_with_candidate_filter(
                    usize::MAX / 2,
                    candidate_filter,
                    generation_filter,
                )?
            }
        };
        let advance_elapsed_ms = advance_start.elapsed().as_millis();
        let inner_used = if use_inner_parallel { dynamic_inner } else { 1 };
        let sampled_now = search.last_sampled();
        let post_stats = search.stats();
        // Candidate sampling / Phase-V early-stop inside the advance also
        // produces a non-exact frontier; treat it the same as apply_beam
        // pruning so the checkpoint gate below refuses to persist it as exact.
        if sampled_now {
            did_beam_filter = true;
        }
        // Adaptive pool sizing.
        //
        // (a) Track an EMA of 1/survival across all steps with a usable
        //     observation. Smooths step-to-step noise — survival can spike or
        //     dip on a single step without violently moving the pool.
        // (b) Each step, derive a target factor = ceil(SAFETY × EMA).
        // (c) Slow decrease: only halve `adaptive_pool_factor` when the EMA
        //     target sits comfortably below half the current factor. Prevents
        //     a single lucky step from shrinking the pool and re-triggering
        //     the collapse path on the next high-mid step.
        if let Some(width) = beam.width {
            // Recompute the cap before each adjustment so it tracks live RSS.
            // Without this the cap would be frozen at startup, which is the
            // exact failure mode the memory-budget design replaces.
            let max_pool_factor = compute_max_pool_factor(
                width,
                candidates_pool_factor,
                max_candidates_pool,
                memory_budget,
            );
            let next_len = post_stats.positions_len;
            let mid_processed = post_stats
                .candidate_count
                .min(width.saturating_mul(adaptive_pool_factor).max(1));
            if mid_processed >= MIN_MID_FOR_OBSERVATION {
                let s_observed = (next_len as f64 / mid_processed as f64).max(1e-6);
                let inv_s = 1.0 / s_observed;
                let new_ema = match ema_inv_survival {
                    None => inv_s,
                    Some(prev) => EMA_ALPHA * prev + (1.0 - EMA_ALPHA) * inv_s,
                };
                ema_inv_survival = Some(new_ema);

                let target_factor = ((new_ema * POOL_SAFETY).ceil() as usize)
                    .clamp(default_pool_factor, max_pool_factor);

                let new_factor = if target_factor > adaptive_pool_factor {
                    // Grow immediately to keep up with worsening survival.
                    target_factor
                } else if target_factor * 2 < adaptive_pool_factor {
                    // EMA is comfortably below half of current → halve (still
                    // honoring default as the floor). One step at a time so
                    // we don't overshoot.
                    (adaptive_pool_factor / 2).max(default_pool_factor)
                } else {
                    adaptive_pool_factor
                };
                // Also enforce the (possibly shrunk) cap. If RSS climbed and
                // the budget-derived ceiling dropped below `adaptive_pool_factor`,
                // honour that by clamping down — this is the OOM-prevention
                // half of the budget model.
                let new_factor = new_factor.min(max_pool_factor).max(default_pool_factor);

                if new_factor != adaptive_pool_factor {
                    adaptive_pool_factor = new_factor;
                    search.set_candidates_pool_factor(adaptive_pool_factor);
                    eprintln!(
                        "adaptive_pool seed={} step={} s={:.4} ema_inv_s={:.2} target={} new_pool_factor={} (cap={} budget_avail_gb={:.1})",
                        seed_index,
                        next_step,
                        s_observed,
                        new_ema,
                        target_factor,
                        adaptive_pool_factor,
                        max_pool_factor,
                        memory_budget.available_bytes() as f64 / (1024.0 * 1024.0 * 1024.0),
                    );
                }
            }
        }
        mt(
            mem_trace,
            seed_index,
            search,
            format_args!(
                "advance next_step={} advanced={} sampled={} mid={} dead={} inner={} remaining={} frontier_in={} memo_limit={} elapsed_ms={} pool_factor={}",
                next_step,
                advanced,
                sampled_now,
                post_stats.candidate_count,
                post_stats.dead_end_count,
                inner_used,
                remaining,
                frontier,
                applied_memo_limit.map(|n| n as i64).unwrap_or(-1),
                advance_elapsed_ms,
                adaptive_pool_factor,
            ),
        );
        if !advanced {
            termination_reason = TerminationReason::Completed;
            break;
        }
        track_peaks(&mut peak_frontier_size, &mut peak_memo_len, search);
        push_trajectory_row(
            &mut trajectory_buf,
            cond_hash,
            seed_index,
            search,
            inner_used,
            advance_elapsed_ms,
        );

        if allow_step_checkpoint && (beam.width.is_none() || !did_beam_filter) {
            let should_checkpoint = match last_checkpoint_time {
                None => true,
                Some(t) => t.elapsed() >= checkpoint_interval,
            };
            if should_checkpoint {
                let _ = write_seed_checkpoint(
                    seed_result_log_path,
                    &SeedCheckpoint {
                        seed_index,
                        seed_sfen: representative_sfen.clone(),
                        max_step,
                        max_frontier: None,
                        constraints,
                        resume_state: search.resume_state_header(),
                        best_piece_count,
                        best_step,
                        best_sfens: vec![],
                        canonicalize_attacker_goldish,
                        adaptive_pool_factor: beam.width.map(|_| adaptive_pool_factor),
                        ema_inv_survival,
                        frontier_bytes: search.frontier_to_binary(),
                        best_position_bytes: best_positions
                            .iter()
                            .flat_map(|p| p.to_bytes())
                            .collect(),
                    },
                );
                last_checkpoint_time = Some(Instant::now());
            }
        }
    }
    // Flush buffered trajectory rows to the shared log in a single lock.
    if !trajectory_buf.is_empty() {
        let mut file = trajectory_log.lock().unwrap();
        let _ = file.write_all(trajectory_buf.as_bytes());
    }

    track_peaks(&mut peak_frontier_size, &mut peak_memo_len, search);
    let final_stats = search.stats();

    Ok(LoopOutcome {
        best_piece_count,
        best_step,
        best_positions,
        peak_frontier_size,
        peak_memo_len,
        total_seen_positions: final_stats.seen_positions as u64,
        terminal_step: final_stats.step,
        termination_reason,
        did_beam_filter,
        split_reached,
    })
}

pub(super) fn log_global_best_if_improved(
    global_best: &AtomicU64,
    seed_index: usize,
    piece_count: u32,
    step: u16,
    positions_len: usize,
    url: &str,
    stats: BackwardSearchStats,
) {
    // Pack (piece_count, step) into a u64: pieces in high 32 bits, step in low
    // 32 bits. A larger packed value is the (#pieces, steps) lexicographic max:
    // more pieces wins outright; equal pieces with more steps also wins. The
    // logged position is therefore always the current global lexicographic max.
    let new_packed = (piece_count as u64) << 32 | (step as u64);
    let mut current = global_best.load(Ordering::Relaxed);
    while new_packed > current {
        match global_best.compare_exchange(
            current,
            new_packed,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                eprintln!(
                    "global_best_pieces={} steps={} seed={} positions={} {} {} {}",
                    piece_count,
                    step,
                    seed_index,
                    positions_len,
                    url,
                    SearchStatsDisplay(stats),
                    ProcStatus::current()
                );
                return;
            }
            Err(next) => current = next,
        }
    }
}

/// Append one trajectory row to a per-seed buffer. The caller flushes the
/// buffer to the shared log file in a single lock acquisition at seed end,
/// avoiding a mutex contention on every step across all parallel seeds.
fn push_trajectory_row(
    buf: &mut String,
    cond_hash: &str,
    seed_index: usize,
    search: &BackwardSearch,
    inner: usize,
    elapsed_ms: u128,
) {
    let stats = search.stats();
    use std::fmt::Write as _;
    let _ = writeln!(
        buf,
        r#"{{"cond":"{cond}","seed":{seed},"step":{step},"frontier":{frontier},"memo":{memo},"inner":{inner},"ms":{ms},"fin":{fin},"dead":{dead},"cand":{cand}}}"#,
        cond = cond_hash,
        seed = seed_index,
        step = stats.step,
        frontier = stats.positions_len,
        memo = stats.memo_len,
        inner = inner,
        ms = elapsed_ms,
        fin = stats.frontier_in,
        dead = stats.dead_end_count,
        cand = stats.candidate_count,
    );
}

/// Emit a single mem_trace line. No-op when `enabled` is false. Search stats
/// and ProcStatus are appended automatically; pass the per-call detail via
/// `format_args!`.
fn mt(enabled: bool, seed_index: usize, search: &BackwardSearch, args: fmt::Arguments<'_>) {
    if !enabled {
        return;
    }
    eprintln!(
        "mem_trace seed={} {} {} {}",
        seed_index,
        args,
        SearchStatsDisplay(search.stats()),
        ProcStatus::current()
    );
}
