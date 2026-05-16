use fmrs_core::{
    position::{position::PositionAux, BitBoard, Position, UndoMove},
    search::backward::{BackwardSearch, BackwardSearchStats},
};
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
    load_seed_checkpoint, write_seed_checkpoint, SeedCheckpoint, SeedRunStats, TerminationReason,
};
use super::beam::{apply_beam, sample_features_to_log, BeamConfig};
use super::system::{ProcStatus, SearchStatsDisplay};

pub(super) struct SingleSeedResult {
    pub(super) best: Option<(u32, u16, Vec<PositionAux>)>, // (piece_count, step, positions)
    pub(super) stats: SeedRunStats,
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
    target_max: u32,
    early_exit: bool,
    stop_signal: &AtomicBool,
    trajectory_log: &Mutex<fs::File>,
    cond_hash: &str,
    canonicalize_attacker_goldish: bool,
    checkpoint_interval_secs: u64,
) -> anyhow::Result<SingleSeedResult> {
    if seeds.is_empty() {
        return Ok(SingleSeedResult {
            best: None,
            stats: SeedRunStats {
                peak_frontier_size: 0,
                peak_memo_len: 0,
                total_seen_positions: 0,
                terminal_step: 0,
                termination_reason: TerminationReason::Unknown,
            },
        });
    }
    let representative = &seeds[0];
    // canonicalize ON/OFF は path suffix + record フィールドで隔離されており、
    // 同 flag 同士の resume が可能。
    let checkpoint = if beam.width.is_some() {
        None
    } else {
        load_seed_checkpoint(
            seed_result_log_path,
            seed_index,
            &representative.sfen(),
            max_step,
            constraints,
            canonicalize_attacker_goldish,
        )
    };

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
                return Ok(SingleSeedResult {
                    best: None,
                    stats: SeedRunStats {
                        peak_frontier_size: 0,
                        peak_memo_len: 0,
                        total_seen_positions: 0,
                        terminal_step: 0,
                        termination_reason: TerminationReason::Unknown,
                    },
                });
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
                return Ok(SingleSeedResult {
                    best: None,
                    stats: SeedRunStats {
                        peak_frontier_size: 0,
                        peak_memo_len: 0,
                        total_seen_positions: 0,
                        terminal_step: 0,
                        termination_reason: TerminationReason::Unknown,
                    },
                })
            }
        }
    };
    if let Some(limit) = max_memo_entries {
        search.set_memo_entry_limit(Some(limit));
    }
    // Track the most recently applied dynamic memo limit so we only re-apply
    // when the per-seed budget grows (dropping `remaining` releases budget to
    // surviving seeds). `max_memo_entries` is the per-seed budget at peak
    // parallelism (`remaining = parallel`); total budget = base * parallel.
    let mut applied_memo_limit = max_memo_entries;
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
    // Checkpoint throttle: track when we last wrote so we don't checkpoint
    // every step on large-frontier searches (which generates huge I/O at scale).
    let checkpoint_interval = std::time::Duration::from_secs(checkpoint_interval_secs);
    let mut last_checkpoint_time: Option<Instant> = None;
    // Trajectory buffer: accumulate rows per-seed, flush once at the end to
    // avoid a mutex acquisition on every step across all parallel seeds.
    let mut trajectory_buf = String::new();
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
    track_peaks(&mut peak_frontier_size, &mut peak_memo_len, &search);

    loop {
        if stop_signal.load(Ordering::Relaxed) {
            termination_reason = TerminationReason::EarlyExit;
            break;
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
                    &search,
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
                    &search,
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
                &search,
                format_args!("output skipped_even_search_step={}", search.step()),
            );
        }

        if beam.width.is_none() {
            if let Some(log) = feature_log {
                sample_features_to_log(log, feature_samples_per_step, seed_index, &search);
            }
        }

        if search_limit.is_some_and(|limit| search.step() >= limit) {
            termination_reason = TerminationReason::MaxStep;
            break;
        }
        // Fused 2-ply. Smoke outputs live on odd steps; the even
        // (white/intermediate) layer never produces output and — per spec —
        // may violate the per-step constraints, so it needs neither smoke
        // filtering nor uniqueness verification. From an odd step we
        // therefore materialise all legal predecessors unverified
        // (`advance_collect_predecessors`, odd→even) and then run the
        // filtered+verified ply (even→odd) which is the source of truth.
        // From an even step (incl. the step-0 bootstrap) we just do the
        // single filtered ply to reach odd parity.
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
            apply_beam(&mut search, beam, width);
        }

        let advance_start = Instant::now();
        // Dynamic inner-parallel: divide the pool budget across seeds still
        // in flight (this seed itself is included in `remaining`). When the
        // tail shrinks, surviving seeds inherit the freed cores.
        let remaining = total_pending
            .saturating_sub(completed_in_run.load(Ordering::Relaxed))
            .max(1);
        let dynamic_inner = ((parallel + remaining - 1) / remaining).max(1);

        if two_ply {
            search.set_parallel(dynamic_inner);
            if !search.advance_collect_predecessors()? {
                termination_reason = TerminationReason::Completed;
                break;
            }
            if let Some(width) = beam.width {
                apply_beam(&mut search, beam, width);
            }
        }

        let candidate_filter = |position: &PositionAux, undo_move: &UndoMove| {
            satisfies_ideal_smoke_undo_candidate(position, undo_move, next_step, constraints)
        };
        let generation_filter = |core: &Position, stone: Option<BitBoard>| {
            let position = PositionAux::new(core.clone(), stone);
            satisfies_ideal_smoke_generation_constraints(&position, next_step, constraints)
        };
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
        let advanced = if use_inner_parallel {
            search.advance_parallel_filtered(&candidate_filter, &generation_filter)?
        } else {
            search.advance_upto_with_candidate_filter(
                usize::MAX / 2,
                candidate_filter,
                generation_filter,
            )?
        };
        let advance_elapsed_ms = advance_start.elapsed().as_millis();
        let inner_used = if use_inner_parallel { dynamic_inner } else { 1 };
        mt(
            mem_trace,
            seed_index,
            &search,
            format_args!(
                "advance next_step={} advanced={} inner={} remaining={} frontier={} memo_limit={} elapsed_ms={}",
                next_step,
                advanced,
                inner_used,
                remaining,
                frontier,
                applied_memo_limit.map(|n| n as i64).unwrap_or(-1),
                advance_elapsed_ms
            ),
        );
        if !advanced {
            termination_reason = TerminationReason::Completed;
            break;
        }
        track_peaks(&mut peak_frontier_size, &mut peak_memo_len, &search);
        push_trajectory_row(
            &mut trajectory_buf,
            cond_hash,
            seed_index,
            &search,
            inner_used,
            advance_elapsed_ms,
        );

        if beam.width.is_none() {
            let should_checkpoint = match last_checkpoint_time {
                None => true,
                Some(t) => t.elapsed() >= checkpoint_interval,
            };
            if should_checkpoint {
                let _ = write_seed_checkpoint(
                    seed_result_log_path,
                    &SeedCheckpoint {
                        seed_index,
                        seed_sfen: representative.sfen(),
                        max_step,
                        max_frontier: None,
                        constraints,
                        resume_state: search.resume_state_header(),
                        best_piece_count,
                        best_step,
                        best_sfens: vec![],
                        canonicalize_attacker_goldish,
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

    track_peaks(&mut peak_frontier_size, &mut peak_memo_len, &search);
    let final_stats = search.stats();
    let stats = SeedRunStats {
        peak_frontier_size,
        peak_memo_len,
        total_seen_positions: final_stats.seen_positions as u64,
        terminal_step: final_stats.step,
        termination_reason,
    };

    mt(
        mem_trace,
        seed_index,
        &search,
        format_args!(
            "before_drop best_pieces={} positions={}",
            best_piece_count,
            best_positions.len()
        ),
    );
    drop(search);
    if mem_trace {
        eprintln!(
            "mem_trace seed={} after_drop best_pieces={} positions={} {}",
            seed_index,
            best_piece_count,
            best_positions.len(),
            ProcStatus::current()
        );
    }

    let best = if best_positions.is_empty() {
        None
    } else if canonicalize_attacker_goldish {
        // canonicalize ON のとき false positive (canonical で唯一だが原局面で非唯一/不詰)
        // が発生しうる。原局面で改めて standard_solve を走らせ、唯一解だけ残す。
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
    };
    Ok(SingleSeedResult { best, stats })
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
