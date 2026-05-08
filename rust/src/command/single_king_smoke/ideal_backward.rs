use anyhow::{bail, Context as _};
use fmrs_core::position::position::PositionAux;
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::super::smoke_constraints::{
    satisfies_mate_square, satisfies_search_constraints, validate_search_constraints,
    KillerSeedLimits, SearchConstraints,
};
use super::super::smoke_persistence::{
    append_seed_result_record, build_seed_result_record, load_seed_result_log,
    merge_seed_result_record, open_seed_result_log, remove_seed_checkpoint,
};
use super::beam::{open_feature_log, BeamConfig, FeatureLogConfig};
use super::enumerate::enumerate_final_2_positions;
use super::search::{search_single_seed, KillerSeedDisplay};
use super::system::ProcStatus;

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
    limits: KillerSeedLimits,
    constraints: SearchConstraints,
    inner_parallel: usize,
    mem_trace: bool,
    feature_log: FeatureLogConfig,
    beam: BeamConfig,
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
    let seeds = if let Some(sfen_like) = seed_sfen {
        let sfen = super::super::parse_to_sfen(&sfen_like)?;
        let position = PositionAux::from_sfen(&sfen)
            .with_context(|| format!("invalid seed sfen: {sfen}"))?;
        vec![(0, position)]
    } else {
        let shuffle_seed = random_seed.unwrap_or_else(|| {
            if fleet_partition.is_some() {
                0
            } else {
                rand::thread_rng().gen()
            }
        });
        let mut rng = SmallRng::seed_from_u64(shuffle_seed);
        let mut seeds = enumerate_final_2_positions(parallel * inner_parallel.max(1), constraints)?
            .into_iter()
            .enumerate()
            .filter(|(_, seed)| {
                satisfies_search_constraints(seed, constraints)
                    && satisfies_mate_square(seed, constraints.mate_squares)
            })
            .collect::<Vec<_>>();
        seeds.shuffle(&mut rng);
        if let Some((idx, size)) = fleet_partition {
            seeds = seeds
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i % size == idx)
                .map(|(_, s)| s)
                .collect();
        }
        if let Some(limit) = seed_limit {
            seeds.truncate(limit);
        }
        seeds
    };
    let mut pending_seeds = Vec::with_capacity(seeds.len());
    let mut initial_best = (0u32, FxHashSet::default(), 0usize);
    let mut loaded_records = 0usize;
    if beam.width.is_some() {
        for (seed_index, seed) in seeds {
            pending_seeds.push((seed_index, seed));
        }
    } else {
        let seed_records =
            load_seed_result_log(&seed_result_log, max_step, limits.max_frontier, constraints)?;
        for (seed_index, seed) in seeds {
            if let Some(record) = seed_records
                .get(&seed_index)
                .filter(|record| record.seed_sfen == seed.sfen())
            {
                loaded_records += 1;
                merge_seed_result_record(&mut initial_best, record);
            } else {
                pending_seeds.push((seed_index, seed));
            }
        }
    }
    let total_seeds = loaded_records + pending_seeds.len();
    eprintln!(
        "seeds={} pending={} loaded_seed_results={} seed_result_log={}",
        total_seeds,
        pending_seeds.len(),
        loaded_records,
        seed_result_log.display()
    );
    let seed_result_log_path = seed_result_log.clone();
    let seed_result_log = Mutex::new(open_seed_result_log(&seed_result_log)?);
    let feature_log_handle = match feature_log.path.as_deref() {
        Some(path) => Some(Mutex::new(open_feature_log(path)?)),
        None => None,
    };
    let feature_samples_per_step = feature_log.samples_per_step;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel * inner_parallel.max(1))
        .build()
        .context("failed to build rayon thread pool")?;
    let completed = AtomicUsize::new(loaded_records);
    let next_heartbeat_index = AtomicUsize::new(0);
    let global_best_piece_count = AtomicU64::new(0);
    let heartbeat_marks = [1usize, 2, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    let best = Mutex::new(initial_best);
    let skipped = Mutex::new(Vec::new());
    pool.install(|| -> anyhow::Result<()> {
        pending_seeds
            .par_iter()
            .try_for_each(|seed_entry| -> anyhow::Result<()> {
                let (seed_index, seed) = seed_entry;
                let result = search_single_seed(
                    *seed_index,
                    seed,
                    max_step,
                    limits,
                    constraints,
                    inner_parallel,
                    mem_trace,
                    &global_best_piece_count,
                    &seed_result_log_path,
                    feature_log_handle.as_ref(),
                    feature_samples_per_step,
                    &beam,
                );
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
                if let Some(killer) = result.killer.as_ref() {
                    skipped.lock().unwrap().push(killer.clone());
                }
                if beam.width.is_none() {
                    append_seed_result_record(
                        &mut seed_result_log.lock().unwrap(),
                        build_seed_result_record(
                            *seed_index,
                            seed,
                            max_step,
                            limits.max_frontier,
                            constraints,
                            &result.best,
                            result.killer.is_some(),
                        ),
                    )?;
                    remove_seed_checkpoint(
                        &seed_result_log_path,
                        *seed_index,
                        max_step,
                        limits.max_frontier,
                        constraints,
                    );
                }
                if let Some((piece_count, positions)) = result.best {
                    let mut best = best.lock().unwrap();
                    best.2 += 1;
                    if piece_count > best.0 {
                        best.0 = piece_count;
                        best.1.clear();
                    }
                    if piece_count == best.0 {
                        for position in positions {
                            best.1.insert(position.sfen());
                        }
                    }
                }
                Ok(())
            })
    })?;

    let (best_piece_count, best_positions, succeeded) = best.into_inner().unwrap();
    let mut skipped = skipped.into_inner().unwrap();
    skipped.sort_by_key(|killer| killer.seed_index);

    if best_positions.is_empty() {
        bail!("No single-king smoke backward result");
    }

    let mut positions = best_positions.into_iter().collect::<Vec<_>>();
    positions.sort();
    eprintln!(
        "best_pieces={}: positions={} succeeded_seeds={}",
        best_piece_count,
        positions.len(),
        succeeded
    );
    if !skipped.is_empty() {
        eprintln!(
            "INCOMPLETE: skipped {} killer seeds (max_frontier={:?})",
            skipped.len(),
            limits.max_frontier
        );
        for killer in skipped {
            eprintln!("skipped {}", KillerSeedDisplay(killer));
        }
    }
    for sfen in positions {
        println!("{sfen}");
    }
    Ok(())
}
