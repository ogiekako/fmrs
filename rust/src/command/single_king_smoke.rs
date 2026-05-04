use anyhow::{bail, Context as _};
use clap::Subcommand;
use fmrs_core::{
    piece::{Color, Kind, KINDS, NUM_HAND_KIND},
    position::{
        advance::{advance::advance_aux, AdvanceOptions},
        position::PositionAux,
        Square, UndoMove,
    },
    search::backward::{BackwardSearch, BackwardSearchStats},
};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const IDEAL_BACKWARD_SEED_LOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Subcommand)]
pub enum SingleKingSmokeCommand {
    #[command(name = "final-2")]
    Final2 {
        #[arg(long)]
        parallel: Option<usize>,
        #[arg(long)]
        max_file: Option<u8>,
    },
    #[command(name = "ideal-backward")]
    IdealBackward {
        #[arg(long)]
        parallel: Option<usize>,
        #[arg(long)]
        seed_limit: Option<usize>,
        #[arg(
            long,
            default_value = "target/single-king-smoke-ideal-backward-seeds.jsonl"
        )]
        seed_result_log: PathBuf,
        #[arg(long)]
        random_seed: Option<u64>,
        #[arg(long)]
        max_step: Option<u16>,
        #[arg(long, default_value_t = 50_000_000)]
        max_memo_entries: usize,
        #[arg(long)]
        max_frontier: Option<usize>,
        #[arg(long, default_value_t = false)]
        no_gold: bool,
        #[arg(long)]
        max_file: Option<u8>,
        #[arg(long, default_value_t = false)]
        mem_trace: bool,
    },
}

pub fn single_king_smoke(cmd: SingleKingSmokeCommand) -> anyhow::Result<()> {
    match cmd {
        SingleKingSmokeCommand::Final2 { parallel, max_file } => enumerate_final_2(
            parallel,
            SearchConstraints {
                no_gold: false,
                max_file,
            },
        ),
        SingleKingSmokeCommand::IdealBackward {
            parallel,
            seed_limit,
            seed_result_log,
            random_seed,
            max_step,
            max_memo_entries,
            max_frontier,
            no_gold,
            max_file,
            mem_trace,
        } => ideal_backward(
            parallel,
            seed_limit,
            seed_result_log,
            random_seed,
            max_step,
            KillerSeedLimits {
                max_memo_entries,
                max_frontier,
            },
            SearchConstraints { no_gold, max_file },
            mem_trace,
        ),
    }
}

fn enumerate_final_2(
    parallel: Option<usize>,
    constraints: SearchConstraints,
) -> anyhow::Result<()> {
    let parallel = parallel.unwrap_or_else(default_parallelism);
    if parallel == 0 {
        bail!("parallel must be positive");
    }
    validate_search_constraints(constraints)?;
    let sfens = enumerate_final_2_sfens(parallel, constraints)?;
    eprintln!("count: {}", sfens.len());
    for sfen in sfens {
        println!("{sfen}");
    }
    Ok(())
}

fn ideal_backward(
    parallel: Option<usize>,
    seed_limit: Option<usize>,
    seed_result_log: PathBuf,
    random_seed: Option<u64>,
    max_step: Option<u16>,
    limits: KillerSeedLimits,
    constraints: SearchConstraints,
    mem_trace: bool,
) -> anyhow::Result<()> {
    let parallel = parallel.unwrap_or_else(default_parallelism);
    if parallel == 0 {
        bail!("parallel must be positive");
    }
    validate_search_constraints(constraints)?;
    let shuffle_seed = random_seed.unwrap_or_else(|| rand::thread_rng().gen());
    let mut rng = SmallRng::seed_from_u64(shuffle_seed);
    let mut seeds = enumerate_final_2_positions(parallel, constraints)?
        .into_iter()
        .enumerate()
        .filter(|(_, seed)| satisfies_search_constraints(seed, constraints))
        .collect::<Vec<_>>();
    seeds.shuffle(&mut rng);
    if let Some(limit) = seed_limit {
        seeds.truncate(limit);
    }
    let seed_records =
        load_seed_result_log(&seed_result_log, max_step, limits.max_frontier, constraints)?;
    let mut pending_seeds = Vec::with_capacity(seeds.len());
    let mut initial_best = (0u16, FxHashSet::default(), 0usize);
    let mut loaded_records = 0usize;
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
    let total_seeds = loaded_records + pending_seeds.len();
    eprintln!(
        "seeds={} pending={} loaded_seed_results={} random_seed={} seed_result_log={}",
        total_seeds,
        pending_seeds.len(),
        loaded_records,
        shuffle_seed,
        seed_result_log.display()
    );
    let seed_result_log = Mutex::new(open_seed_result_log(&seed_result_log)?);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel)
        .build()
        .context("failed to build rayon thread pool")?;
    let completed = AtomicUsize::new(loaded_records);
    let next_heartbeat_index = AtomicUsize::new(0);
    let global_best_step = AtomicUsize::new(0);
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
                    mem_trace,
                    &global_best_step,
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
                append_seed_result_record(
                    &mut seed_result_log.lock().unwrap(),
                    seed_result_record(
                        *seed_index,
                        seed,
                        max_step,
                        limits.max_frontier,
                        constraints,
                        &result,
                    ),
                )?;
                if let Some((step, positions)) = result.best {
                    let mut best = best.lock().unwrap();
                    best.2 += 1;
                    if step > best.0 {
                        best.0 = step;
                        best.1.clear();
                    }
                    if step == best.0 {
                        for position in positions {
                            best.1.insert(position.sfen());
                        }
                    }
                }
                Ok(())
            })
    })?;

    let (best_step, best_positions, succeeded) = best.into_inner().unwrap();
    let mut skipped = skipped.into_inner().unwrap();
    skipped.sort_by_key(|killer| killer.seed_index);

    if best_positions.is_empty() {
        bail!("No single-king smoke backward result");
    }

    let mut positions = best_positions.into_iter().collect::<Vec<_>>();
    positions.sort();
    eprintln!(
        "mate in {}: positions={} succeeded_seeds={}",
        best_step,
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

fn enumerate_final_2_sfens(
    parallel: usize,
    constraints: SearchConstraints,
) -> anyhow::Result<Vec<String>> {
    let kind_pairs = black_piece_kind_pairs();
    let positions = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel)
        .build()
        .context("failed to build rayon thread pool")?
        .install(|| {
            Square::iter()
                .collect::<Vec<_>>()
                .into_par_iter()
                .map(|white_king| enumerate_for_white_king(white_king, &kind_pairs, constraints))
                .reduce(FxHashSet::default, |mut acc, set| {
                    acc.extend(set);
                    acc
                })
        });
    let mut sfens = positions.into_iter().collect::<Vec<_>>();
    sfens.sort();
    sfens.dedup();
    Ok(sfens)
}

fn enumerate_final_2_positions(
    parallel: usize,
    constraints: SearchConstraints,
) -> anyhow::Result<Vec<PositionAux>> {
    enumerate_final_2_sfens(parallel, constraints)?
        .into_iter()
        .map(|sfen| PositionAux::from_sfen(&sfen))
        .collect()
}

fn enumerate_for_white_king(
    white_king: Square,
    kind_pairs: &[(Kind, Kind)],
    constraints: SearchConstraints,
) -> FxHashSet<String> {
    let mut results = FxHashSet::default();
    let mut movements = Vec::new();
    let mate_options = AdvanceOptions {
        max_allowed_branches: Some(0),
    };

    if !square_satisfies_file_constraint(white_king, constraints.max_file) {
        return results;
    }
    for &(kind1, kind2) in kind_pairs {
        let squares1 = legal_black_piece_squares(kind1);
        let squares2 = legal_black_piece_squares(kind2);
        for (i, &sq1) in squares1.iter().enumerate() {
            if sq1 == white_king {
                continue;
            }
            if !square_satisfies_file_constraint(sq1, constraints.max_file) {
                continue;
            }
            let sq2_iter: Box<dyn Iterator<Item = Square>> = if kind1 == kind2 {
                Box::new(squares2.iter().skip(i + 1).copied())
            } else {
                Box::new(squares2.iter().copied())
            };
            for sq2 in sq2_iter {
                if sq2 == white_king || sq2 == sq1 {
                    continue;
                }
                if !square_satisfies_file_constraint(sq2, constraints.max_file) {
                    continue;
                }
                if kind1 == Kind::Pawn && kind2 == Kind::Pawn && sq1.col() == sq2.col() {
                    continue;
                }

                let mut position = PositionAux::default();
                position.set_turn(Color::WHITE);
                position.set(white_king, Color::WHITE, Kind::King);
                position.set(sq1, Color::BLACK, kind1);
                position.set(sq2, Color::BLACK, kind2);
                let mut position = with_white_complement(&position);

                if !position.checked_slow(Color::WHITE) {
                    continue;
                }
                if !satisfies_search_constraints(&position, constraints) {
                    continue;
                }
                movements.clear();
                if matches!(
                    advance_aux(&mut position, &mate_options, &mut movements),
                    Ok(true)
                ) {
                    results.insert(canonical_sfen(&position, constraints));
                }
            }
        }
    }
    results
}

fn black_piece_kind_pairs() -> Vec<(Kind, Kind)> {
    let kinds = KINDS
        .iter()
        .copied()
        .filter(|&kind| kind != Kind::King)
        .collect::<Vec<_>>();
    let mut res = vec![];
    for (i, kind1) in kinds.iter().copied().enumerate() {
        for kind2 in kinds[i..].iter().copied() {
            res.push((kind1, kind2));
        }
    }
    res
}

fn legal_black_piece_squares(kind: Kind) -> Vec<Square> {
    Square::iter()
        .filter(|&sq| black_piece_can_stand_on(kind, sq))
        .collect()
}

fn black_piece_can_stand_on(kind: Kind, sq: Square) -> bool {
    match kind {
        Kind::Pawn | Kind::Lance => sq.row() != 0,
        Kind::Knight => sq.row() >= 2,
        _ => true,
    }
}

fn canonical_lr_sfen(position: &PositionAux) -> String {
    let sfen = position.sfen();
    let reflected = reflect_left_right(position).sfen();
    if sfen <= reflected {
        sfen
    } else {
        reflected
    }
}

fn canonical_sfen(position: &PositionAux, constraints: SearchConstraints) -> String {
    if constraints.breaks_lr_symmetry() {
        position.sfen()
    } else {
        canonical_lr_sfen(position)
    }
}

fn reflect_left_right(position: &PositionAux) -> PositionAux {
    let mut reflected = PositionAux::default();
    reflected.set_turn(position.turn());
    reflected.set_pawn_drop(position.pawn_drop());
    for color in Color::iter() {
        for kind in KINDS[..NUM_HAND_KIND].iter().copied() {
            reflected
                .hands_mut()
                .add_n(color, kind, position.hands().count(color, kind));
        }
    }
    for sq in Square::iter() {
        if let Some((color, kind)) = position.get(sq) {
            reflected.set(Square::new(8 - sq.col(), sq.row()), color, kind);
        }
    }
    reflected
}

fn canonical_lr_position(position: &PositionAux) -> PositionAux {
    let reflected = reflect_left_right(position);
    if position.sfen() <= reflected.sfen() {
        position.clone()
    } else {
        reflected
    }
}

fn canonical_position(position: &PositionAux, constraints: SearchConstraints) -> PositionAux {
    if constraints.breaks_lr_symmetry() {
        position.clone()
    } else {
        canonical_lr_position(position)
    }
}

fn dedup_positions(positions: Vec<PositionAux>) -> Vec<PositionAux> {
    let mut seen = FxHashSet::default();
    let mut deduped = vec![];
    for position in positions {
        let key = position.sfen();
        if seen.insert(key) {
            deduped.push(position);
        }
    }
    deduped.sort_by_key(PositionAux::sfen);
    deduped
}

fn board_piece_count(position: &PositionAux) -> u32 {
    position.occupied_bb().count_ones()
}

#[derive(Clone, Copy)]
struct KillerSeedLimits {
    max_memo_entries: usize,
    max_frontier: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SearchConstraints {
    no_gold: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_file: Option<u8>,
}

impl SearchConstraints {
    fn breaks_lr_symmetry(self) -> bool {
        self.max_file.is_some()
    }
}

struct SingleSeedResult {
    best: Option<(u16, Vec<PositionAux>)>,
    killer: Option<KillerSeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeedResultRecord {
    version: u32,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    #[serde(default)]
    constraints: SearchConstraints,
    seed_index: usize,
    seed_sfen: String,
    best_step: u16,
    positions: usize,
    representative_sfen: Option<String>,
    skipped: bool,
}

fn load_seed_result_log(
    path: &Path,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
) -> anyhow::Result<FxHashMap<usize, SeedResultRecord>> {
    let Ok(file) = fs::File::open(path) else {
        return Ok(FxHashMap::default());
    };
    let mut records = FxHashMap::default();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<SeedResultRecord>(&line) else {
            eprintln!(
                "warning: ignoring malformed seed result log line {} in {}",
                line_index + 1,
                path.display()
            );
            continue;
        };
        if record.version == IDEAL_BACKWARD_SEED_LOG_VERSION
            && record.max_step == max_step
            && record.max_frontier == max_frontier
            && record.constraints == constraints
        {
            records.insert(record.seed_index, record);
        }
    }
    Ok(records)
}

fn open_seed_result_log(path: &Path) -> anyhow::Result<fs::File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn append_seed_result_record(file: &mut fs::File, record: SeedResultRecord) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *file, &record)?;
    writeln!(file)?;
    file.flush()?;
    Ok(())
}

fn seed_result_record(
    seed_index: usize,
    seed: &PositionAux,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
    result: &SingleSeedResult,
) -> SeedResultRecord {
    let (best_step, positions, representative_sfen) =
        if let Some((best_step, positions)) = result.best.as_ref() {
            let mut sfens = positions.iter().map(PositionAux::sfen).collect::<Vec<_>>();
            sfens.sort();
            (*best_step, sfens.len(), sfens.into_iter().next())
        } else {
            (0, 0, None)
        };
    SeedResultRecord {
        version: IDEAL_BACKWARD_SEED_LOG_VERSION,
        max_step,
        max_frontier,
        constraints,
        seed_index,
        seed_sfen: seed.sfen(),
        best_step,
        positions,
        representative_sfen,
        skipped: result.killer.is_some(),
    }
}

fn merge_seed_result_record(best: &mut (u16, FxHashSet<String>, usize), record: &SeedResultRecord) {
    let Some(sfen) = record.representative_sfen.as_ref() else {
        return;
    };
    best.2 += 1;
    if record.best_step > best.0 {
        best.0 = record.best_step;
        best.1.clear();
    }
    if record.best_step == best.0 {
        best.1.insert(sfen.clone());
    }
}

#[derive(Clone)]
struct KillerSeed {
    seed_index: usize,
    best_step: u16,
    best_positions: usize,
    reason: KillerReason,
    stats: BackwardSearchStats,
    proc_status: ProcStatus,
    seed_sfen: String,
}

#[derive(Clone)]
enum KillerReason {
    Frontier { actual: usize, limit: usize },
}

fn search_single_seed(
    seed_index: usize,
    seed: &PositionAux,
    max_step: Option<u16>,
    limits: KillerSeedLimits,
    constraints: SearchConstraints,
    mem_trace: bool,
    global_best_step: &AtomicUsize,
) -> anyhow::Result<SingleSeedResult> {
    let mut search = match BackwardSearch::new_with_parallel(seed, false, 1, false) {
        Ok(search) => search,
        Err(_) => {
            return Ok(SingleSeedResult {
                best: None,
                killer: None,
            });
        }
    };
    if limits.max_memo_entries > 0 {
        search.set_memo_entry_limit(Some(limits.max_memo_entries));
    }
    if mem_trace {
        eprintln!(
            "mem_trace seed={} start {} {}",
            seed_index,
            SearchStatsDisplay(search.stats()),
            ProcStatus::current()
        );
    }
    let mut best_step = 0u16;
    let mut best_positions = vec![];
    let mut killer = None;
    let search_limit = max_step.map(|limit| {
        if limit % 2 == 0 {
            limit.saturating_sub(1)
        } else {
            limit
        }
    });

    loop {
        if search.step() == 0 || search.step() % 2 == 1 {
            let output_start = Instant::now();
            let (step, positions) = search.output_positions(true, false)?;
            let output_raw_positions = positions.len();
            if step > 0 && max_step.is_none_or(|limit| step <= limit) {
                let filtered = positions
                    .into_iter()
                    .map(|position| canonical_position(&position, constraints))
                    .filter(|position| {
                        satisfies_ideal_smoke_constraints(position, step, constraints)
                    })
                    .collect::<Vec<_>>();
                let filtered_len = filtered.len();
                if !filtered.is_empty() && step >= best_step {
                    let improved = step > best_step;
                    if step > best_step {
                        best_step = step;
                        best_positions.clear();
                    }
                    best_positions.extend(filtered);
                    best_positions = dedup_positions(best_positions);
                    if improved && step >= 11 && step % 2 == 1 {
                        let url = best_positions[0].sfen_url();
                        let stats = search.stats();
                        log_global_best_if_improved(
                            global_best_step,
                            seed_index,
                            step,
                            best_positions.len(),
                            &url,
                            stats,
                        );
                    }
                }
                if mem_trace {
                    eprintln!(
                        "mem_trace seed={} output step={} raw={} filtered={} elapsed_ms={} {} {}",
                        seed_index,
                        step,
                        output_raw_positions,
                        filtered_len,
                        output_start.elapsed().as_millis(),
                        SearchStatsDisplay(search.stats()),
                        ProcStatus::current()
                    );
                }
            } else if mem_trace {
                eprintln!(
                    "mem_trace seed={} output step={} raw={} filtered=skipped elapsed_ms={} {} {}",
                    seed_index,
                    step,
                    output_raw_positions,
                    output_start.elapsed().as_millis(),
                    SearchStatsDisplay(search.stats()),
                    ProcStatus::current()
                );
            }
        } else if mem_trace {
            // Even-step black output reconstructs the previous odd frontier from
            // white positions. For ideal smoke best tracking, the odd frontier was
            // already observed directly.
            eprintln!(
                "mem_trace seed={} output skipped_even_search_step={} {} {}",
                seed_index,
                search.step(),
                SearchStatsDisplay(search.stats()),
                ProcStatus::current()
            );
        }

        if let Some(detected) = detect_killer_seed(
            seed_index,
            seed,
            best_step,
            best_positions.len(),
            &search,
            limits,
        ) {
            eprintln!("skip_seed {}", KillerSeedDisplay(detected.clone()));
            killer = Some(detected);
            break;
        }

        if search_limit.is_some_and(|limit| search.step() >= limit) {
            break;
        }
        let next_step = search.step() + 1;
        if search_limit.is_some_and(|limit| next_step > limit) {
            break;
        }
        let advance_start = Instant::now();
        let advanced = search.advance_upto_with_candidate_filter(
            usize::MAX / 2,
            |position, undo_move| {
                satisfies_ideal_smoke_undo_candidate(position, undo_move, next_step, constraints)
            },
            |core, stone| {
                let position = PositionAux::new(core.clone(), stone);
                satisfies_ideal_smoke_generation_constraints(&position, next_step, constraints)
            },
        )?;
        if mem_trace {
            eprintln!(
                "mem_trace seed={} advance next_step={} advanced={} elapsed_ms={} {} {}",
                seed_index,
                next_step,
                advanced,
                advance_start.elapsed().as_millis(),
                SearchStatsDisplay(search.stats()),
                ProcStatus::current()
            );
        }
        if !advanced {
            break;
        }

        if let Some(detected) = detect_killer_seed(
            seed_index,
            seed,
            best_step,
            best_positions.len(),
            &search,
            limits,
        ) {
            eprintln!("skip_seed {}", KillerSeedDisplay(detected.clone()));
            killer = Some(detected);
            break;
        }
    }

    if mem_trace {
        eprintln!(
            "mem_trace seed={} before_drop best={} positions={} {} {}",
            seed_index,
            best_step,
            best_positions.len(),
            SearchStatsDisplay(search.stats()),
            ProcStatus::current()
        );
    }
    drop(search);
    if mem_trace {
        eprintln!(
            "mem_trace seed={} after_drop best={} positions={} {}",
            seed_index,
            best_step,
            best_positions.len(),
            ProcStatus::current()
        );
    }

    let best = if best_positions.is_empty() {
        None
    } else {
        Some((best_step, best_positions))
    };
    Ok(SingleSeedResult { best, killer })
}

fn detect_killer_seed(
    seed_index: usize,
    seed: &PositionAux,
    best_step: u16,
    best_positions: usize,
    search: &BackwardSearch,
    limits: KillerSeedLimits,
) -> Option<KillerSeed> {
    let stats = search.stats();
    let reason = if let Some(limit) = limits.max_frontier {
        if stats.positions_len > limit {
            Some(KillerReason::Frontier {
                actual: stats.positions_len,
                limit,
            })
        } else {
            None
        }
    } else {
        None
    }?;

    Some(KillerSeed {
        seed_index,
        best_step,
        best_positions,
        reason,
        stats,
        proc_status: ProcStatus::current(),
        seed_sfen: seed.sfen(),
    })
}

fn satisfies_ideal_smoke_constraints(
    position: &PositionAux,
    step: u16,
    constraints: SearchConstraints,
) -> bool {
    if step == 0 || step % 2 == 0 {
        return false;
    }
    if position.turn() != Color::BLACK {
        return false;
    }
    satisfies_ideal_smoke_generation_constraints(position, step, constraints)
}

fn satisfies_ideal_smoke_generation_constraints(
    position: &PositionAux,
    step: u16,
    constraints: SearchConstraints,
) -> bool {
    if step == 0 {
        return satisfies_search_constraints(position, constraints);
    }
    if !position.hands().is_empty(Color::BLACK) {
        return false;
    }
    if board_piece_count(position) != step as u32 / 2 + 3 {
        return false;
    }
    satisfies_search_constraints(position, constraints)
}

fn satisfies_ideal_smoke_undo_candidate(
    position: &PositionAux,
    undo_move: &UndoMove,
    next_step: u16,
    constraints: SearchConstraints,
) -> bool {
    if next_step == 0 {
        return true;
    }
    if undo_spawns_white_piece(position, undo_move) {
        return false;
    }
    if constraints.no_gold && undo_creates_gold(position, undo_move) {
        return false;
    }
    if undo_creates_out_of_file_piece(undo_move, constraints.max_file) {
        return false;
    }
    if board_piece_count_after_undo(position, undo_move) != next_step as u32 / 2 + 3 {
        return false;
    }
    black_hand_empty_after_undo(position, undo_move)
}

fn validate_search_constraints(constraints: SearchConstraints) -> anyhow::Result<()> {
    if let Some(max_file) = constraints.max_file {
        if !(1..=9).contains(&max_file) {
            bail!("max-file must be between 1 and 9");
        }
    }
    Ok(())
}

fn satisfies_search_constraints(position: &PositionAux, constraints: SearchConstraints) -> bool {
    if constraints.no_gold && board_gold_count(position) != 0 {
        return false;
    }
    for square in Square::iter() {
        if position.get(square).is_some()
            && !square_satisfies_file_constraint(square, constraints.max_file)
        {
            return false;
        }
    }
    true
}

fn square_satisfies_file_constraint(square: Square, max_file: Option<u8>) -> bool {
    max_file.is_none_or(|max_file| square.col() < max_file as usize)
}

fn board_gold_count(position: &PositionAux) -> u32 {
    position.bitboard(Color::BLACK, Kind::Gold).count_ones()
        + position.bitboard(Color::WHITE, Kind::Gold).count_ones()
}

fn undo_creates_gold(position: &PositionAux, undo_move: &UndoMove) -> bool {
    match undo_move {
        UndoMove::UnDrop(square, _) => position
            .get(*square)
            .is_some_and(|(_, kind)| kind == Kind::Gold),
        UndoMove::UnMove {
            dest,
            promote,
            capture,
            ..
        } => {
            capture.is_some_and(|kind| kind == Kind::Gold)
                || position.get(*dest).is_some_and(|(_, kind)| {
                    let previous_kind = if *promote {
                        kind.unpromote().unwrap()
                    } else {
                        kind
                    };
                    previous_kind == Kind::Gold
                })
        }
    }
}

fn undo_creates_out_of_file_piece(undo_move: &UndoMove, max_file: Option<u8>) -> bool {
    match undo_move {
        UndoMove::UnDrop(_, _) => false,
        UndoMove::UnMove { source, .. } => !square_satisfies_file_constraint(*source, max_file),
    }
}

fn undo_spawns_white_piece(position: &PositionAux, undo_move: &UndoMove) -> bool {
    matches!(
        undo_move,
        UndoMove::UnMove {
            capture: Some(_),
            ..
        } if position.turn() == Color::WHITE
    )
}

fn board_piece_count_after_undo(position: &PositionAux, undo_move: &UndoMove) -> u32 {
    let count = board_piece_count(position);
    match undo_move {
        UndoMove::UnDrop(_, _) => count - 1,
        UndoMove::UnMove {
            capture: Some(_), ..
        } => count + 1,
        UndoMove::UnMove { capture: None, .. } => count,
    }
}

fn black_hand_empty_after_undo(position: &PositionAux, undo_move: &UndoMove) -> bool {
    let prev_turn = position.turn().opposite();
    match undo_move {
        UndoMove::UnDrop(_, _) => {
            prev_turn != Color::BLACK && position.hands().is_empty(Color::BLACK)
        }
        UndoMove::UnMove {
            capture: Some(capture),
            ..
        } if prev_turn == Color::BLACK => {
            black_hand_is_exactly(position, capture.maybe_unpromote())
        }
        UndoMove::UnMove { .. } => position.hands().is_empty(Color::BLACK),
    }
}

fn black_hand_is_exactly(position: &PositionAux, expected: Kind) -> bool {
    for &kind in &KINDS[..NUM_HAND_KIND] {
        let count = position.hands().count(Color::BLACK, kind);
        if kind == expected {
            if count != 1 {
                return false;
            }
        } else if count != 0 {
            return false;
        }
    }
    true
}

fn log_global_best_if_improved(
    global_best_step: &AtomicUsize,
    seed_index: usize,
    step: u16,
    positions_len: usize,
    url: &str,
    stats: BackwardSearchStats,
) {
    let step_usize = step as usize;
    let mut current = global_best_step.load(Ordering::Relaxed);
    while step_usize > current {
        match global_best_step.compare_exchange(
            current,
            step_usize,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                eprintln!(
                    "global_best={} seed={} positions={} {} {} {}",
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

#[cfg(test)]
fn white_hands_are_complement(position: &PositionAux) -> bool {
    KINDS[..NUM_HAND_KIND].iter().copied().all(|kind| {
        let board_used = count_kind_on_board(position, kind);
        let black_hands = position.hands().count(Color::BLACK, kind) as u32;
        let white_hands = position.hands().count(Color::WHITE, kind) as u32;
        board_used + black_hands + white_hands == kind.max_count()
            && white_hands == kind.max_count() - board_used - black_hands
    })
}

fn with_white_complement(position: &PositionAux) -> PositionAux {
    let mut position = position.clone();
    for kind in KINDS[..NUM_HAND_KIND].iter().copied() {
        let board_used = count_kind_on_board(&position, kind);
        let black_hands = position.hands().count(Color::BLACK, kind) as u32;
        let white_hands = position.hands().count(Color::WHITE, kind) as u32;
        let total_used = board_used + black_hands + white_hands;
        let missing = kind
            .max_count()
            .checked_sub(total_used)
            .expect("piece count should not exceed max");
        position
            .hands_mut()
            .add_n(Color::WHITE, kind, missing as usize);
    }
    position
}

fn count_kind_on_board(position: &PositionAux, kind: Kind) -> u32 {
    let mut count = position.bitboard(Color::BLACK, kind).count_ones()
        + position.bitboard(Color::WHITE, kind).count_ones();
    if let Some(promoted) = kind.promote() {
        count += position.bitboard(Color::BLACK, promoted).count_ones()
            + position.bitboard(Color::WHITE, promoted).count_ones();
    }
    count
}

fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[derive(Clone, Default)]
struct ProcStatus {
    vm_rss_kib: Option<usize>,
    vm_size_kib: Option<usize>,
    threads: Option<usize>,
}

struct SearchStatsDisplay(BackwardSearchStats);

impl fmt::Display for SearchStatsDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0;
        write!(
            f,
            "search(step={} seen={} frontier={} prev_frontier={} memo={} prev_memo={})",
            s.step,
            s.seen_positions,
            s.positions_len,
            s.prev_positions_len,
            s.memo_len,
            s.prev_memo_len
        )
    }
}

struct KillerSeedDisplay(KillerSeed);

impl fmt::Display for KillerSeedDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let killer = &self.0;
        write!(
            f,
            "seed={} best={} positions={} reason={} {} {} sfen={}",
            killer.seed_index,
            killer.best_step,
            killer.best_positions,
            KillerReasonDisplay(&killer.reason),
            SearchStatsDisplay(killer.stats),
            killer.proc_status,
            killer.seed_sfen
        )
    }
}

struct KillerReasonDisplay<'a>(&'a KillerReason);

impl fmt::Display for KillerReasonDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            KillerReason::Frontier { actual, limit } => write!(f, "frontier({actual}>{limit})"),
        }
    }
}

impl ProcStatus {
    fn current() -> Self {
        let Ok(status) = fs::read_to_string("/proc/self/status") else {
            return Self::default();
        };
        let mut res = Self::default();
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                res.vm_rss_kib = parse_kib_field(value);
            } else if let Some(value) = line.strip_prefix("VmSize:") {
                res.vm_size_kib = parse_kib_field(value);
            } else if let Some(value) = line.strip_prefix("Threads:") {
                res.threads = value.trim().parse().ok();
            }
        }
        res
    }
}

impl fmt::Display for ProcStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rss={}KiB vmsize={}KiB threads={}",
            self.vm_rss_kib
                .map(|x| x.to_string())
                .unwrap_or_else(|| "?".to_string()),
            self.vm_size_kib
                .map(|x| x.to_string())
                .unwrap_or_else(|| "?".to_string()),
            self.threads
                .map(|x| x.to_string())
                .unwrap_or_else(|| "?".to_string())
        )
    }
}

fn parse_kib_field(value: &str) -> Option<usize> {
    value.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{
        board_piece_count, count_kind_on_board, enumerate_final_2_sfens, reflect_left_right,
        satisfies_ideal_smoke_constraints, satisfies_ideal_smoke_generation_constraints,
        satisfies_ideal_smoke_undo_candidate, satisfies_search_constraints, undo_creates_gold,
        undo_spawns_white_piece, white_hands_are_complement, with_white_complement,
        SearchConstraints,
    };
    use fmrs_core::{
        piece::{Color, Kind},
        position::{position::PositionAux, previous, Square, UndoMove},
    };

    #[test]
    fn reflect_left_right_is_involution() {
        let mut position = PositionAux::default();
        position.set_turn(Color::WHITE);
        position.set(Square::S19, Color::WHITE, Kind::King);
        position.set(Square::S38, Color::BLACK, Kind::ProRook);
        position.set(Square::S72, Color::BLACK, Kind::Silver);

        assert_eq!(
            reflect_left_right(&reflect_left_right(&position)).sfen(),
            position.sfen()
        );
    }

    #[test]
    fn enumerate_final_2_contains_known_single_king_smoke_final() {
        let sfens = enumerate_final_2_sfens(1, SearchConstraints::default()).unwrap();
        assert!(sfens.contains(&"+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1".to_string()));
        assert!(!sfens.contains(&"R7k/R8/9/9/9/9/9/9/9 w 2b4g4s4n4l18p 1".to_string()));
        assert_eq!(
            with_white_complement(
                &PositionAux::from_sfen("6k1+R/4R4/9/9/9/9/9/9/9 w - 1").unwrap()
            )
            .sfen(),
            "6k1+R/4R4/9/9/9/9/9/9/9 w 2b4g4s4n4l18p 1"
        );
    }

    #[test]
    fn with_white_complement_fills_remaining_pieces_to_white_hand() {
        let position = PositionAux::from_sfen("+R1k6/4R4/9/9/9/9/9/9/9 w - 1").unwrap();
        let position = with_white_complement(&position);
        assert!(position.hands().is_empty(Color::BLACK));
        assert!(white_hands_are_complement(&position));
        assert_eq!(count_kind_on_board(&position, Kind::Rook), 2);
        assert_eq!(position.hands().count(Color::WHITE, Kind::Rook), 0);
        assert_eq!(position.hands().count(Color::WHITE, Kind::Pawn), 18);
    }

    #[test]
    fn smoke_constraint_rejects_even_step() {
        let position = PositionAux::from_sfen("+R1k6/4R4/9/9/9/9/9/9/9 b - 1").unwrap();
        assert_eq!(board_piece_count(&position), 3);
        assert!(!satisfies_ideal_smoke_constraints(
            &position,
            2,
            SearchConstraints::default()
        ));
    }

    #[test]
    fn smoke_undo_prefilter_matches_full_generation_constraint() {
        let mut position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let mut undo_moves = vec![];
        previous(&mut position, false, &mut undo_moves);

        for undo_move in undo_moves {
            let mut previous_position = position.clone();
            previous_position.undo_move(&undo_move);
            assert_eq!(
                satisfies_ideal_smoke_undo_candidate(
                    &position,
                    &undo_move,
                    1,
                    SearchConstraints::default()
                ),
                satisfies_ideal_smoke_generation_constraints(
                    &previous_position,
                    1,
                    SearchConstraints::default()
                ),
                "{undo_move:?}"
            );
        }
    }

    #[test]
    fn smoke_undo_prefilter_rejects_white_piece_spawn() {
        let position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let undo_move = UndoMove::UnMove {
            source: Square::S11,
            dest: Square::S19,
            promote: false,
            capture: Some(Kind::Pawn),
            pawn_drop: false,
        };
        assert!(undo_spawns_white_piece(&position, &undo_move));
        assert!(!satisfies_ideal_smoke_undo_candidate(
            &position,
            &undo_move,
            3,
            SearchConstraints::default()
        ));
    }

    #[test]
    fn no_gold_rejects_gold_but_allows_promoted_goldish() {
        let constraints = SearchConstraints {
            no_gold: true,
            max_file: None,
        };
        let gold = PositionAux::from_sfen("9/9/9/9/9/9/9/9/G6k1 b - 1").unwrap();
        let pro_pawn = PositionAux::from_sfen("9/9/9/9/9/9/9/9/+P6k1 b - 1").unwrap();

        assert!(!satisfies_search_constraints(&gold, constraints));
        assert!(satisfies_search_constraints(&pro_pawn, constraints));
    }

    #[test]
    fn no_gold_undo_prefilter_rejects_gold_creation() {
        let constraints = SearchConstraints {
            no_gold: true,
            max_file: None,
        };
        let position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let undo_move = UndoMove::UnMove {
            source: Square::S11,
            dest: Square::S19,
            promote: false,
            capture: Some(Kind::Gold),
            pawn_drop: false,
        };

        assert!(undo_creates_gold(&position, &undo_move));
        assert!(!satisfies_ideal_smoke_undo_candidate(
            &position,
            &undo_move,
            3,
            constraints
        ));
    }

    #[test]
    fn max_file_constraint_restricts_board_squares() {
        let constraints = SearchConstraints {
            no_gold: false,
            max_file: Some(4),
        };
        let mut inside = PositionAux::default();
        inside.set(Square::S11, Color::BLACK, Kind::Bishop);
        inside.set(Square::S41, Color::BLACK, Kind::Bishop);
        inside.set(Square::S19, Color::WHITE, Kind::King);
        let mut outside = inside.clone();
        outside.set(Square::S51, Color::BLACK, Kind::Bishop);

        assert!(satisfies_search_constraints(&inside, constraints));
        assert!(!satisfies_search_constraints(&outside, constraints));
    }

    #[test]
    fn seed_log_constraints_treat_missing_and_null_max_file_as_none() {
        let missing = serde_json::from_str::<SearchConstraints>(r#"{"no_gold":true}"#).unwrap();
        let null = serde_json::from_str::<SearchConstraints>(r#"{"no_gold":true,"max_file":null}"#)
            .unwrap();
        let explicit = SearchConstraints {
            no_gold: true,
            max_file: None,
        };

        assert_eq!(missing, explicit);
        assert_eq!(null, explicit);
        assert_eq!(
            serde_json::to_value(explicit).unwrap(),
            serde_json::json!({"no_gold": true})
        );
    }
}
