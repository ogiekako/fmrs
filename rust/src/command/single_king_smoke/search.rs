use fmrs_core::{
    position::{position::PositionAux, BitBoard, Position, UndoMove},
    search::backward::{BackwardSearch, BackwardSearchStats},
};
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Frontier size threshold below which inner-parallel advance is skipped.
/// par_chunks dispatch overhead exceeds the work for small frontiers; this
/// threshold gates the path switch between single-threaded
/// `advance_upto_with_candidate_filter` and parallel
/// `advance_parallel_filtered`. Tuned conservatively — the cost of running
/// single-thread on a slightly-over-threshold frontier is small, while the
/// cost of running parallel on a tiny frontier is dominated by dispatch.
const FRONTIER_PARALLEL_THRESHOLD: usize = 1024;

use super::super::smoke_constraints::{
    board_piece_count, satisfies_ideal_smoke_constraints,
    satisfies_ideal_smoke_generation_constraints, satisfies_ideal_smoke_undo_candidate,
    KillerSeedLimits, SearchConstraints,
};
use super::super::smoke_persistence::{
    load_seed_checkpoint, write_seed_checkpoint, SeedCheckpoint, SeedRunStats, TerminationReason,
};
use super::beam::{apply_beam, sample_features_to_log, BeamConfig};
use super::system::{ProcStatus, SearchStatsDisplay};

pub(super) struct SingleSeedResult {
    pub(super) best: Option<(u32, Vec<PositionAux>)>, // (piece_count, positions)
    pub(super) killer: Option<KillerSeed>,
    pub(super) stats: SeedRunStats,
}

#[derive(Clone)]
pub(super) struct KillerSeed {
    pub(super) seed_index: usize,
    best_piece_count: u32,
    best_positions: usize,
    reason: KillerReason,
    stats: BackwardSearchStats,
    proc_status: ProcStatus,
    seed_sfen: String,
}

#[derive(Clone)]
struct KillerReason {
    actual: usize,
    limit: usize,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search_single_seed(
    seed_index: usize,
    seed: &PositionAux,
    max_step: Option<u16>,
    limits: KillerSeedLimits,
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
    stop_signal: &AtomicBool,
) -> anyhow::Result<SingleSeedResult> {
    let checkpoint = if beam.width.is_some() {
        None
    } else {
        load_seed_checkpoint(
            seed_result_log_path,
            seed_index,
            &seed.sfen(),
            max_step,
            limits.max_frontier,
            constraints,
        )
    };

    let mut search = match checkpoint
        .as_ref()
        .and_then(|cp| BackwardSearch::from_resume_state(&cp.resume_state, 1).ok())
        .or_else(|| BackwardSearch::new_with_parallel(seed, false, 1, false).ok())
    {
        Some(s) => s,
        None => {
            return Ok(SingleSeedResult {
                best: None,
                killer: None,
                stats: SeedRunStats {
                    peak_frontier_size: 0,
                    peak_memo_len: 0,
                    total_seen_positions: 0,
                    terminal_step: 0,
                    termination_reason: TerminationReason::Unknown,
                },
            })
        }
    };
    if let Some(max_memo_entries) = limits.max_memo_entries {
        search.set_memo_entry_limit(Some(max_memo_entries));
    }
    // Track the most recently applied dynamic memo limit so we only re-apply
    // when the per-seed budget grows (dropping `remaining` releases budget to
    // surviving seeds). `limits.max_memo_entries` is the per-seed budget at
    // peak parallelism (`remaining = parallel`); total budget = base * parallel.
    let mut applied_memo_limit = limits.max_memo_entries;
    search.set_delta_trace(mem_trace);
    mt(mem_trace, seed_index, &search, format_args!("start resumed={}", checkpoint.is_some()));
    let mut best_piece_count = 0u32;
    let mut best_positions: Vec<PositionAux> = vec![];
    if let Some(ref cp) = checkpoint {
        best_piece_count = cp.best_piece_count;
        best_positions = cp
            .best_sfens
            .iter()
            .filter_map(|sfen| PositionAux::from_sfen(sfen).ok())
            .collect();
    }
    let mut killer = None;
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
                    if pc > best_piece_count {
                        best_piece_count = pc;
                        best_positions.clear();
                        improved = true;
                    }
                    if pc == best_piece_count {
                        best_positions.push(position);
                    }
                }
                if improved {
                    // dedup_positions 不要: improved 直前に best_positions.clear()
                    // しており、push されるのは単一の output_positions の filtered
                    // 結果のみ。output_positions は frontier (一意) に基づくので
                    // 重複は発生しない。
                    debug_assert!({
                        let mut seen = fmrs_core::nohash::NoHashSet64::default();
                        best_positions.iter().all(|p| seen.insert(p.digest()))
                    }, "best_positions has duplicates after improvement");
                }
                let positions_increased = best_positions.len() > prev_positions_len;
                if (improved || positions_increased) && best_piece_count >= 8 {
                    let url = best_positions[0].sfen_url();
                    let stats = search.stats();
                    log_global_best_if_improved(
                        global_best_piece_count,
                        seed_index,
                        best_piece_count,
                        best_positions.len(),
                        &url,
                        stats,
                    );
                }
                if best_piece_count >= target_max && !stop_signal.swap(true, Ordering::Relaxed) {
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

        if let Some(detected) = detect_killer_seed(
            seed_index,
            seed,
            best_piece_count,
            best_positions.len(),
            &search,
            limits,
        ) {
            eprintln!("skip_seed {}", KillerSeedDisplay(detected.clone()));
            killer = Some(detected);
            termination_reason = TerminationReason::MaxFrontier;
            break;
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
        let next_step = search.step() + 1;
        if search_limit.is_some_and(|limit| next_step > limit) {
            termination_reason = TerminationReason::MaxStep;
            break;
        }

        if let Some(width) = beam.width {
            apply_beam(&mut search, beam, width);
        }

        let advance_start = Instant::now();
        let candidate_filter = |position: &PositionAux, undo_move: &UndoMove| {
            satisfies_ideal_smoke_undo_candidate(position, undo_move, next_step, constraints)
        };
        let generation_filter = |core: &Position, stone: Option<BitBoard>| {
            let position = PositionAux::new(core.clone(), stone);
            satisfies_ideal_smoke_generation_constraints(&position, next_step, constraints)
        };
        // Dynamic inner-parallel: divide the pool budget across seeds still
        // in flight (this seed itself is included in `remaining`). When the
        // tail shrinks, surviving seeds inherit the freed cores.
        let remaining = total_pending
            .saturating_sub(completed_in_run.load(Ordering::Relaxed))
            .max(1);
        let dynamic_inner = (parallel / remaining).max(1);
        let frontier = search.stats().positions_len;
        let use_inner_parallel =
            dynamic_inner > 1 && frontier >= FRONTIER_PARALLEL_THRESHOLD;
        search.set_parallel(if use_inner_parallel { dynamic_inner } else { 1 });
        // Dynamic memo budget: as `remaining` drops, the surviving seed gets
        // a larger share of the total memo budget. Only grow (never shrink)
        // since shrinking would require evicting hot entries the seed is
        // already using.
        if let Some(base) = limits.max_memo_entries {
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
        mt(
            mem_trace,
            seed_index,
            &search,
            format_args!(
                "advance next_step={} advanced={} inner={} remaining={} frontier={} memo_limit={} elapsed_ms={}",
                next_step,
                advanced,
                if use_inner_parallel { dynamic_inner } else { 1 },
                remaining,
                frontier,
                applied_memo_limit.map(|n| n as i64).unwrap_or(-1),
                advance_start.elapsed().as_millis()
            ),
        );
        if !advanced {
            termination_reason = TerminationReason::Completed;
            break;
        }
        track_peaks(&mut peak_frontier_size, &mut peak_memo_len, &search);

        if beam.width.is_none() {
            let _ = write_seed_checkpoint(
                seed_result_log_path,
                &SeedCheckpoint {
                    seed_index,
                    seed_sfen: seed.sfen(),
                    max_step,
                    max_frontier: limits.max_frontier,
                    constraints,
                    resume_state: search.resume_state(),
                    best_piece_count,
                    best_sfens: best_positions.iter().map(PositionAux::sfen).collect(),
                },
            );
        }

        if let Some(detected) = detect_killer_seed(
            seed_index,
            seed,
            best_piece_count,
            best_positions.len(),
            &search,
            limits,
        ) {
            eprintln!("skip_seed {}", KillerSeedDisplay(detected.clone()));
            killer = Some(detected);
            termination_reason = TerminationReason::MaxFrontier;
            break;
        }
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
    } else {
        // 全 best positions を返す。output 集計側で SFEN HashSet で uniq 化されるので
        // 出力 line 数 = unique best position 数。テストでも実体ある count を見たい。
        Some((best_piece_count, best_positions))
    };
    Ok(SingleSeedResult { best, killer, stats })
}

fn detect_killer_seed(
    seed_index: usize,
    seed: &PositionAux,
    best_piece_count: u32,
    best_positions: usize,
    search: &BackwardSearch,
    limits: KillerSeedLimits,
) -> Option<KillerSeed> {
    let stats = search.stats();
    let limit = limits.max_frontier?;
    if stats.positions_len <= limit {
        return None;
    }
    Some(KillerSeed {
        seed_index,
        best_piece_count,
        best_positions,
        reason: KillerReason {
            actual: stats.positions_len,
            limit,
        },
        stats,
        proc_status: ProcStatus::current(),
        seed_sfen: seed.sfen(),
    })
}

pub(super) fn log_global_best_if_improved(
    global_best: &AtomicU64,
    seed_index: usize,
    piece_count: u32,
    positions_len: usize,
    url: &str,
    stats: BackwardSearchStats,
) {
    // Pack (piece_count, positions_len) into a u64: pieces in high 32 bits,
    // positions in low 32 bits. A larger packed value is strictly better:
    // more pieces wins outright; equal pieces with more positions also wins.
    let new_packed = (piece_count as u64) << 32 | (positions_len as u64).min(u32::MAX as u64);
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
                    "global_best_pieces={} seed={} positions={} {} {} {}",
                    piece_count,
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

pub(super) struct KillerSeedDisplay(pub(super) KillerSeed);

impl fmt::Display for KillerSeedDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let killer = &self.0;
        write!(
            f,
            "seed={} best_pieces={} positions={} reason=frontier({}>{}) {} {} sfen={}",
            killer.seed_index,
            killer.best_piece_count,
            killer.best_positions,
            killer.reason.actual,
            killer.reason.limit,
            SearchStatsDisplay(killer.stats),
            killer.proc_status,
            killer.seed_sfen
        )
    }
}
