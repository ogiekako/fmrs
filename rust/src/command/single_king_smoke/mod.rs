use clap::Subcommand;
use std::path::PathBuf;

use super::smoke_constraints::{parse_allowed_kinds, parse_mate_squares, KillerSeedLimits, SearchConstraints};

mod beam;
mod enumerate;
mod ideal_backward;
mod search;
mod system;
mod train;

use beam::{build_beam_config, FeatureLogConfig};
use system::parse_max_memo_entries;

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
        /// Min % of pawns among pieces in play (board + black hand) (0–100), enforced at
        /// steps >= --min-pawn-pct-after-step.  E.g. --min-pawn-pct 30
        #[arg(long)]
        min_pawn_pct: Option<u16>,
        /// Step threshold for --min-pawn-pct (default: 6).
        #[arg(long, default_value_t = 6)]
        min_pawn_pct_after_step: u16,
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
        /// 都詰: allow 4-piece mate on the center square (5五).
        #[arg(long, default_value_t = false)]
        miyako: bool,
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
        /// Fleet partitioning: this instance's 0-based index.
        #[arg(long)]
        fleet_index: Option<usize>,
        /// Fleet partitioning: total number of instances.
        #[arg(long)]
        fleet_size: Option<usize>,
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
            min_pawn_pct,
            min_pawn_pct_after_step,
            mem_trace,
            slack,
            mate_square,
            miyako,
            feature_log,
            feature_sample_per_step,
            beam_width,
            beam_model,
            fleet_index,
            fleet_size,
        } => {
            let max_memo_entries = parse_max_memo_entries(&max_memo_entries, parallel)?;
            let beam = build_beam_config(beam_width, beam_model.as_deref())?;
            let allowed_kinds_mask = match allowed_kinds {
                Some(names) => Some(parse_allowed_kinds(&names)?),
                None => None,
            };
            let mate_squares = parse_mate_squares(&mate_square)?;
            ideal_backward::ideal_backward(
                parallel,
                seed_sfen,
                seed_limit,
                seed_result_log,
                random_seed,
                max_step,
                fleet_index,
                fleet_size,
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
                    min_pawn_pct,
                    min_pawn_pct_after_step,
                    mate_squares,
                    miyako,
                },
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
        } => train::export_features(&feature_log, &seed_result_log, &out, min_label),
        SingleKingSmokeCommand::TrainModel {
            seed_result_log,
            out,
            min_label,
        } => train::train_model(&seed_result_log, &out, min_label),
    }
}
