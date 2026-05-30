use anyhow::{bail, Context as _};
use fmrs_core::position::position::PositionAux;
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::super::smoke_constraints::{
    satisfies_mate_square, satisfies_search_constraints, theoretical_max_piece_count,
    validate_search_constraints, SearchConstraints,
};
use super::super::smoke_persistence::{
    append_seed_result_record, build_seed_result_record, condition_key, load_seed_result_log,
    merge_best_candidate, merge_seed_result_record, open_seed_result_log, remove_seed_checkpoint,
    trajectory_log_path, CrossSeedBest, SeedResultRecord,
    TerminationReason,
};
use rustc_hash::FxHashMap;
use super::beam::{open_feature_log, BeamConfig, FeatureLogConfig};
use super::enumerate::enumerate_final_2_positions;
use super::oracle::OracleModel;
use super::scheduler::run_with_oracle;
use super::search::search_single_seed;
use super::system::{MemoryBudget, ProcStatus};

#[allow(clippy::too_many_arguments)]
pub(super) fn ideal_backward(
    parallel: usize,
    seed_sfen: Option<String>,
    seed_limit: Option<usize>,
    seed_result_log: PathBuf,
    random_seed: Option<u64>,
    max_step: Option<u16>,
    fleet_index: Option<usize>,
    fleet_size: Option<usize>,
    max_memo_entries: Option<usize>,
    oracle_model: Option<PathBuf>,
    canonicalize_attacker_goldish: bool,
    constraints: SearchConstraints,
    mem_trace: bool,
    feature_log: FeatureLogConfig,
    beam: BeamConfig,
    candidates_pool_factor: usize,
    max_candidates_pool: Option<usize>,
    memory_budget_pct: u32,
    checkpoint_interval_secs: u64,
    early_exit: bool,
    progress_ticker: bool,
) -> anyhow::Result<()> {
    if parallel == 0 {
        bail!("parallel must be positive");
    }
    validate_search_constraints(constraints)?;
    let fleet_partition = match (fleet_index, fleet_size) {
        (Some(idx), Some(size)) => {
            if size == 0 {
                bail!("--fleet-size must be positive");
            }
            if idx >= size {
                bail!("--fleet-index ({idx}) must be < --fleet-size ({size})");
            }
            Some((idx, size))
        }
        (None, None) => None,
        _ => bail!("--fleet-index and --fleet-size must both be specified"),
    };
    // Step 1: enumerate + filter (decoupled from truncate so grouping can act on
    // the full population when canonicalize is on).
    let raw_enumerated: Vec<(usize, PositionAux)> = if let Some(sfen_like) = seed_sfen {
        let sfen = super::super::parse_to_sfen(&sfen_like)?;
        let position =
            PositionAux::from_sfen(&sfen).with_context(|| format!("invalid seed sfen: {sfen}"))?;
        vec![(0, position)]
    } else {
        enumerate_final_2_positions(parallel, constraints)?
            .into_iter()
            .enumerate()
            .filter(|(_, seed)| {
                satisfies_search_constraints(seed, constraints)
                    && satisfies_mate_square(seed, constraints.mate_squares)
            })
            .collect::<Vec<_>>()
    };
    let shuffle_seed = random_seed.unwrap_or_else(|| {
        if fleet_partition.is_some() {
            0
        } else {
            rand::thread_rng().gen()
        }
    });
    let mut rng = SmallRng::seed_from_u64(shuffle_seed);

    // Step 2: canonicalize ON では grouping を先に行う (seed_limit は group 単位)。
    //         OFF では 1 seed = 1 group とみなす。
    let groups_unshuffled: Vec<(usize, Vec<PositionAux>)> = if canonicalize_attacker_goldish {
        use rustc_hash::FxHashMap;
        let mut groups: FxHashMap<u64, (usize, Vec<PositionAux>)> = FxHashMap::default();
        for (seed_index, seed) in raw_enumerated {
            let key = fmrs_core::search::canonicalize::canonical_digest_for_smoke(&seed);
            let entry = groups.entry(key).or_insert((seed_index, Vec::new()));
            if seed_index < entry.0 {
                entry.0 = seed_index;
            }
            entry.1.push(seed);
        }
        let mut v: Vec<(usize, Vec<PositionAux>)> = groups.into_values().collect();
        v.sort_by_key(|(idx, _)| *idx);
        v
    } else {
        raw_enumerated
            .into_iter()
            .map(|(idx, s)| (idx, vec![s]))
            .collect()
    };

    // Step 3: shuffle + fleet partition + truncate を group 単位で適用。
    let mut grouped_seeds: Vec<(usize, Vec<PositionAux>)> = groups_unshuffled;
    grouped_seeds.shuffle(&mut rng);
    if let Some((idx, size)) = fleet_partition {
        grouped_seeds = grouped_seeds
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % size == idx)
            .map(|(_, g)| g)
            .collect();
    }
    if let Some(limit) = seed_limit {
        grouped_seeds.truncate(limit);
    }
    let total_seed_count: usize = grouped_seeds.iter().map(|(_, g)| g.len()).sum();
    if canonicalize_attacker_goldish {
        eprintln!(
            "canonicalize: {} groups (avg group size {:.2}, max {})",
            grouped_seeds.len(),
            if grouped_seeds.is_empty() {
                0.0
            } else {
                total_seed_count as f64 / grouped_seeds.len() as f64
            },
            grouped_seeds
                .iter()
                .map(|(_, g)| g.len())
                .max()
                .unwrap_or(0)
        );
    }
    // 書き込みは beam.width.is_none() でガードしているので、log にあるレコードは
    // 必ず非 beam の厳密結果 = beam 結果より弱くなることはない。よって beam モード
    // でも同じ log を読み込んで既存 seed をスキップしてよい。
    // canonicalize ON/OFF はファイル / レコード上で隔離されている (path suffix と
    // record の `canonicalize_attacker_goldish` フィールド)。同 flag の run 同士は
    // 中断後に再開できる。
    let seed_records = load_seed_result_log(
        &seed_result_log,
        max_step,
        constraints,
        canonicalize_attacker_goldish,
    )?;
    let (pending_seeds, initial_best, loaded_records) =
        partition_against_existing_records(grouped_seeds, &seed_records);
    let total_seeds = loaded_records + pending_seeds.len();
    let target_max = theoretical_max_piece_count(constraints);
    eprintln!(
        "seeds={} pending={} loaded_seed_results={} target_max={} seed_result_log={}",
        total_seeds,
        pending_seeds.len(),
        loaded_records,
        target_max,
        seed_result_log.display()
    );
    let stop_signal = AtomicBool::new(early_exit && initial_best.0 >= target_max);
    if stop_signal.load(Ordering::Relaxed) {
        eprintln!(
            "early_exit: target_max={} already reached by loaded records (best={})",
            target_max, initial_best.0
        );
    }
    let seed_result_log_path = seed_result_log.clone();
    let trajectory_path = trajectory_log_path(&seed_result_log_path);
    let trajectory_log = Mutex::new(open_seed_result_log(&trajectory_path)?);
    let cond_hash = condition_key(max_step, constraints);
    let seed_result_log = Mutex::new(open_seed_result_log(&seed_result_log)?);
    let feature_log_handle = match feature_log.path.as_deref() {
        Some(path) => Some(Mutex::new(open_feature_log(path)?)),
        None => None,
    };
    let feature_samples_per_step = feature_log.samples_per_step;

    // Oracle path: PQ-based scheduler. Replaces the per-seed par_iter loop.
    if let Some(oracle_path) = oracle_model {
        if beam.width.is_some() {
            bail!("--oracle-model is incompatible with --beam-width (use one or the other)");
        }
        let oracle = OracleModel::load(&oracle_path)
            .with_context(|| format!("loading oracle model from {}", oracle_path.display()))?;
        eprintln!(
            "oracle: loaded {} ({} features) from {}",
            oracle.model_type,
            oracle.feature_names.len(),
            oracle_path.display()
        );
        let stop_signal = Arc::new(stop_signal);
        let final_best = run_with_oracle(
            pending_seeds,
            constraints,
            max_step,
            max_memo_entries,
            parallel,
            seed_result_log_path,
            seed_result_log,
            trajectory_log,
            oracle,
            target_max,
            stop_signal,
            initial_best,
            canonicalize_attacker_goldish,
            checkpoint_interval_secs,
            early_exit,
        )?;
        return finalize_output(final_best);
    }

    // Pool size = parallel (= total cores allocated to this run).
    // inner_parallel is no longer multiplied in: each seed dynamically picks
    // how many threads to use within `advance_*` based on how many other seeds
    // are still in flight. See `pending_remaining` in search_single_seed.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel)
        .build()
        .context("failed to build rayon thread pool")?;
    let total_pending = pending_seeds.len();
    let completed_in_run = AtomicUsize::new(0);
    // Memory budget for adaptive pool sizing. `pct=0` disables (falls back
    // to legacy --max-candidates-pool / 8×W default).
    let memory_budget = MemoryBudget::from_pct(memory_budget_pct);
    if memory_budget.budget_bytes() > 0 {
        eprintln!(
            "memory_budget pct={} budget_gb={:.1}",
            memory_budget_pct,
            memory_budget.budget_bytes() as f64 / (1024.0 * 1024.0 * 1024.0),
        );
    }
    let completed = AtomicUsize::new(loaded_records);
    let next_heartbeat_index = AtomicUsize::new(0);
    let global_best_piece_count = AtomicU64::new(0);
    let heartbeat_marks = [1usize, 2, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    let best = Mutex::new(initial_best);

    // Out-of-band progress heartbeat (on by default; `--no-progress-ticker`
    // disables). One thread that, every 5s, prints the current
    // advance_parallel_filtered sub-phase char (P/C/V/F, `.`=idle) with no
    // newline, so a single slow step in the deep tail no longer looks frozen.
    // A newline + timestamp every ~60s wraps the line and forces the tee/pipe
    // buffer to flush.
    let ticker_stop = Arc::new(AtomicBool::new(false));
    let ticker_handle = spawn_progress_ticker(progress_ticker, ticker_stop.clone());

    let install_result = pool.install(|| -> anyhow::Result<()> {
        pending_seeds
            .par_iter()
            .try_for_each(|seed_entry| -> anyhow::Result<()> {
                let (seed_index, group) = seed_entry;
                let representative = &group[0];
                let result = search_single_seed(
                    *seed_index,
                    group.as_slice(),
                    max_step,
                    max_memo_entries,
                    constraints,
                    parallel,
                    total_pending,
                    &completed_in_run,
                    mem_trace,
                    &global_best_piece_count,
                    &seed_result_log_path,
                    feature_log_handle.as_ref(),
                    feature_samples_per_step,
                    &beam,
                    candidates_pool_factor,
                    max_candidates_pool,
                    memory_budget,
                    target_max,
                    early_exit,
                    &stop_signal,
                    &trajectory_log,
                    &cond_hash,
                    canonicalize_attacker_goldish,
                    checkpoint_interval_secs,
                );
                completed_in_run.fetch_add(1, Ordering::Relaxed);
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                loop {
                    let idx = next_heartbeat_index.load(Ordering::Relaxed);
                    if idx >= heartbeat_marks.len() {
                        break;
                    }
                    let mark = heartbeat_marks[idx];
                    if done * 100 < total_seeds * mark {
                        break;
                    }
                    if next_heartbeat_index
                        .compare_exchange(idx, idx + 1, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                    {
                        eprintln!(
                            "progress: {}% ({}/{}) {}",
                            mark,
                            done,
                            total_seeds,
                            ProcStatus::current()
                        );
                    }
                }
                let result = result?;
                // EarlyExit で best が target_max 未満なら部分結果。再実行時に
                // 続きが取れるよう record も checkpoint も触らない。
                // 自身が target_max に到達した seed は EarlyExit でも保存する。
                let early_exited_partial = early_exit
                    && result.stats.termination_reason == TerminationReason::EarlyExit
                    && result.best.as_ref().is_none_or(|(pc, _, _)| *pc < target_max);
                if (beam.width.is_none() || !result.beam_filtered) && !early_exited_partial {
                    append_seed_result_record(
                        &mut seed_result_log.lock().unwrap(),
                        build_seed_result_record(
                            *seed_index,
                            representative,
                            max_step,
                            constraints,
                            &result.best,
                            result.stats,
                            canonicalize_attacker_goldish,
                        ),
                    )?;
                    remove_seed_checkpoint(
                        &seed_result_log_path,
                        *seed_index,
                        max_step,
                        constraints,
                        canonicalize_attacker_goldish,
                    );
                }
                if let Some((piece_count, step, positions)) = result.best {
                    let mut best = best.lock().unwrap();
                    merge_best_candidate(
                        &mut best,
                        piece_count,
                        step,
                        positions.iter().map(PositionAux::sfen),
                    );
                }
                Ok(())
            })
    });

    ticker_stop.store(true, Ordering::Relaxed);
    if let Some(h) = ticker_handle {
        let _ = h.join();
    }
    install_result?;

    let final_best = best.into_inner().unwrap();
    finalize_output(final_best)
}

/// Spawn the progress heartbeat thread when `enabled`. Returns `None` (no
/// thread) otherwise. The thread checks the stop flag every second so it exits
/// promptly when the run finishes.
fn spawn_progress_ticker(
    enabled: bool,
    stop: Arc<AtomicBool>,
) -> Option<std::thread::JoinHandle<()>> {
    if !enabled {
        return None;
    }
    Some(std::thread::spawn(move || {
        use std::io::Write as _;
        const TICK_SECS: u64 = 5;
        // `tee` to a file only flushes on newline, so wrap (newline + flush)
        // every 12 ticks ≈ 60s: the stream stays visible within ~1 min via
        // `gcp-spot.sh tail` while the log stays compact (~12 chars per line).
        const WRAP_TICKS: u64 = 12;
        {
            let mut e = std::io::stderr().lock();
            let _ = writeln!(
                e,
                "[progress] heartbeat every {TICK_SECS}s (advance_parallel_filtered sub-phase): \
                 P=candidate-gen C=collect-candidates V=verify-uniqueness F=finalize .=idle"
            );
            let _ = e.flush();
        }
        let mut ticks: u64 = 0;
        loop {
            // Sleep TICK_SECS in 1s steps so a finished run stops the ticker
            // within ~1s instead of waiting a full interval.
            for _ in 0..TICK_SECS {
                if stop.load(Ordering::Relaxed) {
                    let mut e = std::io::stderr().lock();
                    let _ = writeln!(e, " [progress] done");
                    let _ = e.flush();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            let ch = fmrs_core::search::backward::progress_phase_char();
            let mut e = std::io::stderr().lock();
            ticks += 1;
            if ticks % WRAP_TICKS == 0 {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = write!(e, "{ch}\n[progress t={secs}] ");
            } else {
                let _ = write!(e, "{ch}");
            }
            let _ = e.flush();
        }
    }))
}

fn finalize_output(best: CrossSeedBest) -> anyhow::Result<()> {
    let (best_piece_count, best_step, best_positions, succeeded) = best;
    if best_positions.is_empty() {
        bail!("No single-king smoke backward result");
    }
    let mut positions = best_positions.into_iter().collect::<Vec<_>>();
    positions.sort();
    eprintln!(
        "best_pieces={} best_steps={}: positions={} succeeded_seeds={}",
        best_piece_count,
        best_step,
        positions.len(),
        succeeded
    );
    for sfen in positions {
        println!("{sfen}");
    }
    Ok(())
}

/// Split enumerated seeds against existing exact records: seeds whose index +
/// representative sfen match an entry in `seed_records` are dropped from the
/// pending list and merged into `initial_best` instead. Returns
/// `(pending, initial_best, loaded_count)`.
///
/// Records in `seed_records` are assumed to come from non-beam (exact) runs —
/// `append_seed_result_record` is gated on `beam.width.is_none()` in the
/// caller, so beam runs never write to the log and reusing what's there is
/// always at least as good as re-running with beam.
fn partition_against_existing_records(
    grouped_seeds: Vec<(usize, Vec<PositionAux>)>,
    seed_records: &FxHashMap<usize, SeedResultRecord>,
) -> (Vec<(usize, Vec<PositionAux>)>, CrossSeedBest, usize) {
    let mut pending: Vec<(usize, Vec<PositionAux>)> = Vec::with_capacity(grouped_seeds.len());
    let mut initial_best: CrossSeedBest = (0u32, 0u16, FxHashSet::default(), 0usize);
    let mut loaded = 0usize;
    for (seed_index, group) in grouped_seeds {
        // canon OFF: group size = 1。canon ON: group[0] は raw_enumerated 順での
        // 最初の seed (確定的)。書き込み時と読み込み時で同じ representative。
        let representative = &group[0];
        if let Some(record) = seed_records
            .get(&seed_index)
            .filter(|record| record.seed_sfen == representative.sfen())
        {
            loaded += 1;
            merge_seed_result_record(&mut initial_best, record);
        } else {
            pending.push((seed_index, group));
        }
    }
    (pending, initial_best, loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::smoke_persistence::IDEAL_BACKWARD_SEED_LOG_VERSION;

    fn seed(sfen: &str) -> PositionAux {
        PositionAux::from_sfen(sfen).unwrap()
    }

    fn record(seed_index: usize, seed_sfen: &str, best_piece_count: u32, best_step: u16) -> SeedResultRecord {
        SeedResultRecord {
            version: IDEAL_BACKWARD_SEED_LOG_VERSION,
            max_step: None,
            max_frontier: None,
            constraints: SearchConstraints::default(),
            seed_index,
            seed_sfen: seed_sfen.to_string(),
            best_step,
            best_piece_count,
            positions: 1,
            representative_sfen: Some(seed_sfen.to_string()),
            skipped: false,
            peak_frontier_size: 0,
            peak_memo_len: 0,
            total_seen_positions: 0,
            terminal_step: best_step,
            termination_reason: TerminationReason::Completed,
            canonicalize_attacker_goldish: false,
        }
    }

    #[test]
    fn empty_records_leaves_all_seeds_pending() {
        let sfen_a = "9/9/9/9/9/9/9/9/G6k1 b - 1";
        let sfen_b = "9/9/9/9/9/9/9/9/+P6k1 b - 1";
        let grouped = vec![(0, vec![seed(sfen_a)]), (1, vec![seed(sfen_b)])];
        let records = FxHashMap::default();

        let (pending, best, loaded) = partition_against_existing_records(grouped, &records);

        assert_eq!(loaded, 0);
        assert_eq!(pending.len(), 2);
        assert_eq!(best.0, 0);
        assert_eq!(best.3, 0);
    }

    #[test]
    fn matching_record_skips_seed_and_merges_into_best() {
        let sfen_a = "9/9/9/9/9/9/9/9/G6k1 b - 1";
        let sfen_b = "9/9/9/9/9/9/9/9/+P6k1 b - 1";
        let grouped = vec![(0, vec![seed(sfen_a)]), (1, vec![seed(sfen_b)])];
        let mut records = FxHashMap::default();
        records.insert(0, record(0, &seed(sfen_a).sfen(), 17, 9));

        let (pending, best, loaded) = partition_against_existing_records(grouped, &records);

        assert_eq!(loaded, 1);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, 1, "seed 0 should have been filtered out");
        assert_eq!(best.0, 17, "best piece count should pick up the record's bpc");
        assert_eq!(best.1, 9, "best step should pick up the record's step");
        assert_eq!(best.3, 1, "one succeeded seed merged into best");
    }

    #[test]
    fn sfen_mismatch_keeps_seed_pending() {
        // 同じ seed_index でも sfen が違えば古いレコード扱いで skip しない
        // (seed の再列挙順が変わった等のケース)。
        let sfen_actual = "9/9/9/9/9/9/9/9/G6k1 b - 1";
        let sfen_recorded = "9/9/9/9/9/9/9/9/+P6k1 b - 1";
        let grouped = vec![(0, vec![seed(sfen_actual)])];
        let mut records = FxHashMap::default();
        records.insert(0, record(0, &seed(sfen_recorded).sfen(), 17, 9));

        let (pending, best, loaded) = partition_against_existing_records(grouped, &records);

        assert_eq!(loaded, 0);
        assert_eq!(pending.len(), 1);
        assert_eq!(best.0, 0);
    }
}
