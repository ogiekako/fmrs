use anyhow::{bail, Context as _};
use clap::Subcommand;
use fmrs_core::{
    piece::{Color, Kind, KINDS},
    position::{
        advance::{advance::advance_aux, AdvanceOptions},
        position::PositionAux,
        BitBoard, Position, Square, UndoMove,
    },
    search::backward::{BackwardSearch, BackwardSearchStats},
};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::smoke_features::{extract_features, LinearModel};
use super::smoke_constraints::{
    board_piece_count, canonical_position, canonical_sfen, parse_allowed_kinds,
    parse_mate_squares, satisfies_ideal_smoke_constraints, satisfies_mate_square,
    satisfies_ideal_smoke_generation_constraints, satisfies_ideal_smoke_undo_candidate,
    satisfies_search_constraints, square_in_bounds, validate_search_constraints,
    with_white_complement, KillerSeedLimits, SearchConstraints,
};
use super::smoke_persistence::{
    append_seed_result_record, build_seed_result_record, load_seed_checkpoint,
    load_seed_result_log, merge_seed_result_record, open_seed_result_log, remove_seed_checkpoint,
    write_seed_checkpoint, SeedCheckpoint,
};

#[derive(Debug, Clone, Subcommand)]
pub enum SingleKingSmokeCommand {
    /// Backward-search for ideal-smoke initial positions.
    ///
    /// Beam-search workflow (data-driven):
    ///   # 1) Collect training samples while running normally:
    ///   cargo run --release -- single-king-smoke ideal-backward \
    ///       --feature-log target/features.jsonl ...
    ///   # 2) Convert samples + seed results to a CSV (filter by best_piece_count):
    ///   cargo run --release -- single-king-smoke export-features \
    ///       --feature-log target/features.jsonl \
    ///       --seed-result-log target/single-king-smoke-ideal-backward-seeds.jsonl \
    ///       -o target/training.csv --min-label 16
    ///   # 3) Train the linear model offline:
    ///   python3 scripts/train_beam_model.py \
    ///       --csv target/training.csv --out target/beam_model.json --standardize
    ///   # 4) Re-run with beam pruning:
    ///   cargo run --release -- single-king-smoke ideal-backward \
    ///       --beam-width 1000 --beam-model target/beam_model.json ...
    #[command(name = "ideal-backward")]
    IdealBackward {
        #[arg(long, default_value_t = 1)]
        parallel: usize,
        #[arg(long)]
        seed_sfen: Option<String>,
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
        /// Memo entry limit per seed. "auto" (default) = memory/cores,
        /// "full" = memory/parallel, "none" = unlimited, or a number.
        #[arg(long, default_value = "auto")]
        max_memo_entries: String,
        #[arg(long)]
        max_frontier: Option<usize>,
        #[arg(long, default_value_t = false)]
        no_gold: bool,
        #[arg(long, default_value_t = false)]
        no_pawn: bool,
        /// 豆腐図式: only Pawn/ProPawn (+ King) allowed on board.
        #[arg(long, default_value_t = false)]
        only_pawn: bool,
        /// Comma-separated list of allowed piece kinds on the board (+ King always allowed).
        /// E.g. --allowed-kinds pawn,lance,knight. Overrides --no-gold/--no-pawn/--only-pawn.
        #[arg(long, value_delimiter = ',')]
        allowed_kinds: Option<Vec<String>>,
        /// Enforce per-kind piece count limits (board + black hand):
        /// R,B <= 1; L,N,S,G <= 2; P <= 9.
        #[arg(long, default_value_t = false)]
        natural_piece_limit: bool,
        #[arg(long)]
        max_file: Option<u8>,
        #[arg(long)]
        max_rank: Option<u8>,
        #[arg(long, default_value_t = false)]
        allow_white_pieces: bool,
        /// Max % of promoted pieces on the board (0–100), enforced at
        /// steps >= --max-promoted-pct-after-step.  E.g. --max-promoted-pct 20
        #[arg(long)]
        max_promoted_pct: Option<u16>,
        /// Step threshold for --max-promoted-pct (default: 6 ≈ 7手詰以上).
        #[arg(long, default_value_t = 6)]
        max_promoted_pct_after_step: u16,
        #[arg(long, default_value_t = 1)]
        inner_parallel: usize,
        #[arg(long, default_value_t = false)]
        mem_trace: bool,
        #[arg(long, default_value_t = 0)]
        slack: u16,
        /// Filter seeds by white king position at mate. Shogi notation:
        /// first digit = file (筋, 1=right .. 9=left), second digit = rank
        /// (段, 1=top .. 9=bottom). E.g. 11 = 1一, 55 = 5五.
        /// Multiple squares can be specified: --mate-square 11 --mate-square 19
        #[arg(long)]
        mate_square: Vec<String>,
        /// Append per-step frontier samples (with extracted features) to
        /// this JSONL file. Used to build training data for the beam model.
        #[arg(long)]
        feature_log: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        feature_sample_per_step: usize,
        /// Beam width: after each search step, keep only the top K frontier
        /// positions ranked by `--beam-model` (or a default heuristic).
        #[arg(long)]
        beam_width: Option<usize>,
        /// Beam scoring: path to model JSON, or "handcraft". Omit for random.
        #[arg(long)]
        beam_model: Option<String>,
    },
    /// Join feature samples with seed results to produce a CSV for offline training.
    #[command(name = "export-features")]
    ExportFeatures {
        /// Feature log produced by --feature-log during ideal-backward.
        #[arg(long)]
        feature_log: PathBuf,
        /// Seed result log (jsonl) — used to look up best_piece_count per seed.
        #[arg(long)]
        seed_result_log: PathBuf,
        /// Output CSV path.
        #[arg(long, short = 'o')]
        out: PathBuf,
        /// Only include seeds whose best_piece_count >= this threshold.
        #[arg(long, default_value_t = 16)]
        min_label: u32,
    },
    /// Train a beam model from the seed result log (no --feature-log needed).
    ///
    /// Solves each representative_sfen to collect intermediate positions,
    /// extracts features, and writes a CSV + trained model JSON.
    #[command(name = "train-model")]
    TrainModel {
        /// Seed result log (jsonl).
        #[arg(long, default_value = "target/single-king-smoke-ideal-backward-seeds.jsonl")]
        seed_result_log: PathBuf,
        /// Output model JSON path.
        #[arg(long, short = 'o', default_value = "models/beam_model.json")]
        out: PathBuf,
        /// Only include seeds whose best_piece_count >= this threshold.
        #[arg(long, default_value_t = 0)]
        min_label: u32,
    },
}

pub fn single_king_smoke(cmd: SingleKingSmokeCommand) -> anyhow::Result<()> {
    match cmd {
        SingleKingSmokeCommand::IdealBackward {
            parallel,
            seed_sfen,
            seed_limit,
            seed_result_log,
            random_seed,
            max_step,
            max_memo_entries,
            max_frontier,
            no_gold,
            no_pawn,
            only_pawn,
            allowed_kinds,
            natural_piece_limit,
            max_file,
            max_rank,
            allow_white_pieces,
            max_promoted_pct,
            max_promoted_pct_after_step,
            inner_parallel,
            mem_trace,
            slack,
            mate_square,
            feature_log,
            feature_sample_per_step,
            beam_width,
            beam_model,
        } => {
            let max_memo_entries =
                parse_max_memo_entries(&max_memo_entries, parallel, inner_parallel)?;
            let beam = build_beam_config(beam_width, beam_model.as_deref())?;
            let allowed_kinds_mask = match allowed_kinds {
                Some(names) => Some(parse_allowed_kinds(&names)?),
                None => None,
            };
            let mate_squares = parse_mate_squares(&mate_square)?;
            ideal_backward(
                parallel,
                seed_sfen,
                seed_limit,
                seed_result_log,
                random_seed,
                max_step,
                KillerSeedLimits {
                    max_memo_entries,
                    max_frontier,
                },
                SearchConstraints {
                    no_gold,
                    no_pawn,
                    only_pawn,
                    allowed_kinds_mask,
                    natural_piece_limit,
                    max_file,
                    max_rank,
                    allow_white_pieces,
                    slack,
                    max_promoted_pct,
                    max_promoted_pct_after_step,
                    mate_squares,
                },
                inner_parallel,
                mem_trace,
                FeatureLogConfig {
                    path: feature_log,
                    samples_per_step: feature_sample_per_step,
                },
                beam,
            )
        }
        SingleKingSmokeCommand::ExportFeatures {
            feature_log,
            seed_result_log,
            out,
            min_label,
        } => export_features(&feature_log, &seed_result_log, &out, min_label),
        SingleKingSmokeCommand::TrainModel {
            seed_result_log,
            out,
            min_label,
        } => train_model(&seed_result_log, &out, min_label),
    }
}

fn export_features(
    feature_log: &Path,
    seed_result_log: &Path,
    out: &Path,
    min_label: u32,
) -> anyhow::Result<()> {
    use super::smoke_features::feature_names;
    use super::smoke_persistence::SeedResultRecord;

    // Build seed_index → best_piece_count map from seed result log.
    let mut labels: FxHashMap<usize, u32> = FxHashMap::default();
    let file = fs::File::open(seed_result_log)
        .with_context(|| format!("open seed_result_log {}", seed_result_log.display()))?;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: SeedResultRecord = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skip malformed seed_result line {}: {e}", i + 1);
                continue;
            }
        };
        let piece_count = if record.best_piece_count > 0 {
            record.best_piece_count
        } else {
            record.best_step as u32 / 2 + 3
        };
        // Keep the max in case of duplicates.
        let entry = labels.entry(record.seed_index).or_insert(0);
        if piece_count > *entry {
            *entry = piece_count;
        }
    }

    let names = feature_names();
    let mut writer = std::io::BufWriter::new(
        fs::File::create(out).with_context(|| format!("create {}", out.display()))?,
    );
    write!(writer, "seed_index,step,label")?;
    for n in names.iter() {
        write!(writer, ",{}", n)?;
    }
    writeln!(writer)?;

    let file = fs::File::open(feature_log)
        .with_context(|| format!("open feature_log {}", feature_log.display()))?;
    let mut total = 0usize;
    let mut kept = 0usize;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip malformed feature line {}: {e}", i + 1);
                continue;
            }
        };
        let seed_index = v["seed_index"].as_u64().unwrap_or(0) as usize;
        let step = v["step"].as_u64().unwrap_or(0) as u16;
        let Some(&label) = labels.get(&seed_index) else {
            continue;
        };
        if label < min_label {
            continue;
        }
        let features = match v["features"].as_array() {
            Some(arr) => arr,
            None => continue,
        };
        if features.len() != names.len() {
            eprintln!(
                "skip line {}: expected {} features, got {}",
                i + 1,
                names.len(),
                features.len()
            );
            continue;
        }
        write!(writer, "{seed_index},{step},{label}")?;
        for f in features {
            write!(writer, ",{}", f.as_f64().unwrap_or(0.0))?;
        }
        writeln!(writer)?;
        kept += 1;
    }
    writer.flush()?;
    eprintln!(
        "export-features: wrote {} rows (out of {} samples) to {}",
        kept,
        total,
        out.display()
    );
    Ok(())
}

fn train_model(seed_result_log: &Path, model_out: &Path, min_label: u32) -> anyhow::Result<()> {
    use super::smoke_features::feature_names;
    use super::smoke_persistence::SeedResultRecord;
    use fmrs_core::solve::standard_solve::standard_solve;

    let file = fs::File::open(seed_result_log)
        .with_context(|| format!("open {}", seed_result_log.display()))?;
    let mut records: Vec<SeedResultRecord> = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<SeedResultRecord>(&line) else {
            continue;
        };
        if record.best_piece_count >= min_label && record.representative_sfen.is_some() {
            records.push(record);
        }
    }
    records.sort_by(|a, b| b.best_piece_count.cmp(&a.best_piece_count));
    records.dedup_by_key(|r| r.seed_index);
    eprintln!(
        "train-model: {} seeds with best_piece_count >= {}",
        records.len(),
        min_label
    );

    let names = feature_names();
    let csv_path = model_out.with_extension("csv");
    let mut writer = std::io::BufWriter::new(
        fs::File::create(&csv_path)
            .with_context(|| format!("create {}", csv_path.display()))?,
    );
    write!(writer, "seed_index,step,label")?;
    for n in names.iter() {
        write!(writer, ",{n}")?;
    }
    writeln!(writer)?;

    let mut total_rows = 0usize;
    let mut solved = 0usize;
    let mut failed = 0usize;
    for record in &records {
        let sfen = record.representative_sfen.as_deref().unwrap();
        let Ok(pos) = PositionAux::from_sfen(sfen) else {
            failed += 1;
            continue;
        };
        let Ok(reconstructor) = standard_solve(pos.clone(), 1, true) else {
            failed += 1;
            continue;
        };
        let solutions = reconstructor.solutions();
        let Some(movements) = solutions.first() else {
            failed += 1;
            continue;
        };
        solved += 1;

        let mut pos = pos;
        let label = record.best_piece_count;
        for (i, mv) in movements.iter().enumerate() {
            if pos.turn().is_black() {
                let features = extract_features(&pos);
                write!(writer, "{},{i},{label}", record.seed_index)?;
                for f in &features {
                    write!(writer, ",{f}")?;
                }
                writeln!(writer)?;
                total_rows += 1;
            }
            pos.do_move(mv);
        }
        if pos.turn().is_black() {
            let features = extract_features(&pos);
            write!(writer, "{},{},{label}", record.seed_index, movements.len())?;
            for f in &features {
                write!(writer, ",{f}")?;
            }
            writeln!(writer)?;
            total_rows += 1;
        }
    }
    writer.flush()?;
    eprintln!(
        "train-model: {} rows from {} solved seeds ({} failed) → {}",
        total_rows,
        solved,
        failed,
        csv_path.display()
    );

    let script = Path::new("scripts/train_beam_model.py");
    if !script.exists() {
        bail!("training script not found: {}", script.display());
    }
    if let Some(parent) = model_out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new("python3")
        .arg(script)
        .arg("--csv")
        .arg(&csv_path)
        .arg("--out")
        .arg(model_out)
        .arg("--standardize")
        .status()
        .context("failed to run python3")?;
    if !status.success() {
        bail!("training script failed with {status}");
    }
    eprintln!("train-model: model saved to {}", model_out.display());
    Ok(())
}

fn ideal_backward(
    parallel: usize,
    seed_sfen: Option<String>,
    seed_limit: Option<usize>,
    seed_result_log: PathBuf,
    random_seed: Option<u64>,
    max_step: Option<u16>,
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
    let seeds = if let Some(sfen_like) = seed_sfen {
        let sfen = super::parse_to_sfen(&sfen_like)?;
        let position = PositionAux::from_sfen(&sfen)
            .with_context(|| format!("invalid seed sfen: {sfen}"))?;
        vec![(0, position)]
    } else {
        let shuffle_seed = random_seed.unwrap_or_else(|| rand::thread_rng().gen());
        let mut rng = SmallRng::seed_from_u64(shuffle_seed);
        let mut seeds = enumerate_final_2_positions(parallel, constraints)?
            .into_iter()
            .enumerate()
            .filter(|(_, seed)| {
                satisfies_search_constraints(seed, constraints)
                    && satisfies_mate_square(seed, constraints.mate_squares)
            })
            .collect::<Vec<_>>();
        seeds.shuffle(&mut rng);
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
        .num_threads(parallel)
        .build()
        .context("failed to build rayon thread pool")?;
    let completed = AtomicUsize::new(loaded_records);
    let next_heartbeat_index = AtomicUsize::new(0);
    let global_best_piece_count = AtomicUsize::new(0);
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

    if !square_in_bounds(white_king, constraints) {
        return results;
    }
    if constraints.mate_squares != 0
        && (constraints.mate_squares >> white_king.index()) & 1 == 0
    {
        return results;
    }
    for &(kind1, kind2) in kind_pairs {
        let squares1 = legal_black_piece_squares(kind1);
        let squares2 = legal_black_piece_squares(kind2);
        for (i, &sq1) in squares1.iter().enumerate() {
            if sq1 == white_king {
                continue;
            }
            if !square_in_bounds(sq1, constraints) {
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
                if !square_in_bounds(sq2, constraints) {
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

#[derive(Clone, Default)]
struct FeatureLogConfig {
    path: Option<PathBuf>,
    samples_per_step: usize,
}

#[derive(Clone)]
enum BeamScorer {
    Random,
    Handcraft,
    Model(LinearModel),
}

struct BeamConfig {
    width: Option<usize>,
    scorer: BeamScorer,
}

fn build_beam_config(
    width: Option<usize>,
    model_spec: Option<&str>,
) -> anyhow::Result<BeamConfig> {
    let scorer = match model_spec {
        None => BeamScorer::Random,
        Some("handcraft") => BeamScorer::Handcraft,
        Some(path) => BeamScorer::Model(LinearModel::load(Path::new(path))?),
    };
    Ok(BeamConfig { width, scorer })
}

fn open_feature_log(path: &Path) -> anyhow::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open feature log {}", path.display()))
}

fn apply_beam(search: &mut BackwardSearch, beam: &BeamConfig, width: usize) {
    let (_, positions) = search.positions();
    if positions.len() <= width || width == 0 {
        return;
    }
    match &beam.scorer {
        BeamScorer::Random => {
            let (_, positions) = search.positions();
            let n = positions.len();
            let mut indices: Vec<usize> = (0..n).collect();
            let mut rng = SmallRng::from_entropy();
            indices.partial_shuffle(&mut rng, width);
            let kept: Vec<Position> =
                indices[..width].iter().map(|&i| positions[i].clone()).collect();
            search.replace_positions(kept);
        }
        scorer => {
            let (stone, positions) = search.positions();
            let mut scored: Vec<(f32, Position)> = positions
                .par_iter()
                .map(|p| {
                    let aux = PositionAux::new(p.clone(), stone);
                    let features = extract_features(&aux);
                    let score = match scorer {
                        BeamScorer::Model(m) => m.score(&features),
                        _ => handcraft_beam_score(&features),
                    };
                    (score, p.clone())
                })
                .collect();
            scored.select_nth_unstable_by(width - 1, |a, b| {
                b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            let truncated: Vec<Position> =
                scored.into_iter().take(width).map(|(_, p)| p).collect();
            search.replace_positions(truncated);
        }
    }
}

fn handcraft_beam_score(features: &[f32]) -> f32 {
    let names = super::smoke_features::feature_names();
    let get = |n: &str| -> f32 {
        names
            .iter()
            .position(|x| *x == n)
            .map(|i| features[i])
            .unwrap_or(0.0)
    };
    2.0 * get("board_total")
        + 0.5 * get("hand_black_total")
        + 0.05 * get("total_black_kiki")
        + 0.3 * get("king_white_neighbors_attacked")
        - 0.2 * get("king_white_min_edge_dist")
}

fn sample_features_to_log(
    log: &Mutex<fs::File>,
    samples_per_step: usize,
    seed_index: usize,
    search: &BackwardSearch,
) {
    if samples_per_step == 0 {
        return;
    }
    let step = search.step();
    if step == 0 || step % 2 == 0 {
        // Sample only black-to-move frontiers (== smoke initial positions).
        return;
    }
    let (stone, positions) = search.positions();
    if positions.is_empty() {
        return;
    }
    let n = positions.len();
    let k = samples_per_step.min(n);
    let mut rng = SmallRng::seed_from_u64(
        (seed_index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ step as u64,
    );
    let mut lines = Vec::with_capacity(k);
    for _ in 0..k {
        let idx = rng.gen_range(0..n);
        let aux = PositionAux::new(positions[idx].clone(), stone);
        let features = extract_features(&aux);
        let sfen = aux.sfen();
        let line = serde_json::json!({
            "seed_index": seed_index,
            "step": step,
            "sfen": sfen,
            "features": features,
        })
        .to_string();
        lines.push(line);
    }
    let mut file = log.lock().unwrap();
    for line in lines {
        let _ = writeln!(file, "{}", line);
    }
}

struct SingleSeedResult {
    best: Option<(u32, Vec<PositionAux>)>, // (piece_count, positions)
    killer: Option<KillerSeed>,
}

#[derive(Clone)]
struct KillerSeed {
    seed_index: usize,
    best_piece_count: u32,
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
    inner_parallel: usize,
    mem_trace: bool,
    global_best_piece_count: &AtomicUsize,
    seed_result_log_path: &Path,
    feature_log: Option<&Mutex<fs::File>>,
    feature_samples_per_step: usize,
    beam: &BeamConfig,
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

    let mut search = if let Some(ref cp) = checkpoint {
        match BackwardSearch::from_resume_state(&cp.resume_state, inner_parallel) {
            Ok(search) => search,
            Err(_) => {
                // Checkpoint is stale or corrupt; start fresh
                match BackwardSearch::new_with_parallel(seed, false, inner_parallel, false) {
                    Ok(search) => search,
                    Err(_) => {
                        return Ok(SingleSeedResult {
                            best: None,
                            killer: None,
                        });
                    }
                }
            }
        }
    } else {
        match BackwardSearch::new_with_parallel(seed, false, inner_parallel, false) {
            Ok(search) => search,
            Err(_) => {
                return Ok(SingleSeedResult {
                    best: None,
                    killer: None,
                });
            }
        }
    };
    if let Some(max_memo_entries) = limits.max_memo_entries {
        search.set_memo_entry_limit(Some(max_memo_entries));
    }
    search.set_delta_trace(mem_trace);
    if mem_trace {
        eprintln!(
            "mem_trace seed={} start resumed={} {} {}",
            seed_index,
            checkpoint.is_some(),
            SearchStatsDisplay(search.stats()),
            ProcStatus::current()
        );
    }
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
                    best_positions = dedup_positions(best_positions);
                    if best_piece_count >= 8 {
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
            best_piece_count,
            best_positions.len(),
            &search,
            limits,
        ) {
            eprintln!("skip_seed {}", KillerSeedDisplay(detected.clone()));
            killer = Some(detected);
            break;
        }

        if beam.width.is_none() {
            if let Some(log) = feature_log {
                sample_features_to_log(log, feature_samples_per_step, seed_index, &search);
            }
        }

        if search_limit.is_some_and(|limit| search.step() >= limit) {
            break;
        }
        let next_step = search.step() + 1;
        if search_limit.is_some_and(|limit| next_step > limit) {
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
        let advanced = if inner_parallel > 1 {
            search.advance_parallel_filtered(&candidate_filter, &generation_filter)?
        } else {
            search.advance_upto_with_candidate_filter(
                usize::MAX / 2,
                candidate_filter,
                generation_filter,
            )?
        };
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
            break;
        }
    }

    if mem_trace {
        eprintln!(
            "mem_trace seed={} before_drop best_pieces={} positions={} {} {}",
            seed_index,
            best_piece_count,
            best_positions.len(),
            SearchStatsDisplay(search.stats()),
            ProcStatus::current()
        );
    }
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
        let mut sfens = best_positions.iter().map(PositionAux::sfen).collect::<Vec<_>>();
        sfens.sort();
        let representative = sfens.into_iter().next().unwrap();
        let representative_pos = best_positions
            .into_iter()
            .find(|p| p.sfen() == representative)
            .unwrap();
        Some((best_piece_count, vec![representative_pos]))
    };
    Ok(SingleSeedResult { best, killer })
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
        best_piece_count,
        best_positions,
        reason,
        stats,
        proc_status: ProcStatus::current(),
        seed_sfen: seed.sfen(),
    })
}

fn log_global_best_if_improved(
    global_best_piece_count: &AtomicUsize,
    seed_index: usize,
    piece_count: u32,
    positions_len: usize,
    url: &str,
    stats: BackwardSearchStats,
) {
    let pc = piece_count as usize;
    let mut current = global_best_piece_count.load(Ordering::Relaxed);
    while pc > current {
        match global_best_piece_count.compare_exchange(
            current,
            pc,
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
            "seed={} best_pieces={} positions={} reason={} {} {} sfen={}",
            killer.seed_index,
            killer.best_piece_count,
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

fn parse_max_memo_entries(
    value: &str,
    parallel: usize,
    inner_parallel: usize,
) -> anyhow::Result<Option<usize>> {
    match value {
        "auto" => {
            let total_cores = default_parallelism();
            let divisor = (parallel * inner_parallel).min(total_cores);
            let entries = memo_entries_for_memory(divisor);
            eprintln!(
                "auto max_memo_entries={} (parallel={} inner_parallel={} total_cores={} divisor={})",
                entries, parallel, inner_parallel, total_cores, divisor
            );
            Ok(Some(entries))
        }
        "full" => {
            let divisor = (parallel * inner_parallel).min(default_parallelism());
            let entries = memo_entries_for_memory(divisor);
            eprintln!(
                "full max_memo_entries={} (parallel={} inner_parallel={})",
                entries, parallel, inner_parallel
            );
            Ok(Some(entries))
        }
        "none" => Ok(None),
        s => Ok(Some(s.parse::<usize>().context(
            "max-memo-entries must be a number, \"auto\", \"full\", or \"none\"",
        )?)),
    }
}

fn memo_entries_for_memory(divisor: usize) -> usize {
    let total_bytes = total_memory_bytes();
    // Reserve 20% for OS, frontier, positions, etc.
    let available = total_bytes * 4 / 5;
    let per_worker = available / divisor.max(1);
    // Each entry: u64 key (8B) + StepRange (8B) + SwissTable overhead ≈ 24B.
    // memo + prev_memo coexist (×2 = 48B), plus rehash peak (×1.5 ≈ 72B),
    // plus frontier vecs and candidate buffers.
    let bytes_per_entry = 128;
    per_worker / bytes_per_entry
}

fn total_memory_bytes() -> usize {
    let Ok(content) = fs::read_to_string("/proc/meminfo") else {
        return 8 * 1024 * 1024 * 1024; // fallback 8GB
    };
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            if let Some(kb) = parse_kib_field(value) {
                return kb * 1024;
            }
        }
    }
    8 * 1024 * 1024 * 1024
}

fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::super::smoke_constraints::with_white_complement;
    use super::{enumerate_final_2_sfens, SearchConstraints};
    use fmrs_core::position::position::PositionAux;

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
}
