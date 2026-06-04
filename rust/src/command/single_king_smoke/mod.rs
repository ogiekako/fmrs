use clap::Subcommand;
use std::path::PathBuf;

use super::smoke_constraints::{parse_allowed_kinds, parse_mate_squares, SearchConstraints};

mod beam;
mod enumerate;
mod ideal_backward;
mod oracle;
mod scheduler;
mod search;
mod system;
mod train;

use beam::{build_beam_config, FeatureLogConfig};
use ideal_backward::SplitConfig;
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
        /// Search step at/above which the cross-step memo is retained (and
        /// bounded by --max-memo-entries) instead of discarded each step.
        /// Default 10. Set above --max-step to always discard the memo each
        /// step, minimizing memo memory (OOM escape hatch) at the cost of
        /// cross-step cache hits. Pairs with --split-* to bound both frontier
        /// and memo memory.
        #[arg(long, default_value_t = 10)]
        memo_retain_from_step: u16,
        /// Disable the mid-ply uniqueness prune (on by default). In the fused
        /// 2-ply advance, intermediate (even) positions are verified for
        /// uniqueness when they produce at least one filtered out-candidate;
        /// non-unique intermediates are dropped early. Frontier-preserving:
        /// a non-unique even ply can't yield a unique odd ply. Typically
        /// +35–48% faster; set this flag to disable if needed.
        #[arg(long, default_value_t = false)]
        no_mid_uniqueness_prune: bool,
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
        /// 双玉: final mate position has both kings (white king + black king +
        /// one black piece; miyako 双玉: + two pieces).
        #[arg(long, default_value_t = false)]
        double_king: bool,
        /// 黒の自陣 (rank 7-9) の goldish 駒優先順位制約:
        /// ProLance は白持駒に Pawn がない場合のみ、
        /// ProKnight は Pawn も Lance もない場合のみ、
        /// ProSilver は Pawn も Lance も Knight もない場合のみ配置可。
        #[arg(long, default_value_t = false)]
        goldish_priority: bool,
        /// Allow bishop/rook (incl. promoted) on the board only past this
        /// piece-count threshold. Pieces in play (board + black hand) must
        /// be ≥ this value for any rook/bishop to appear. Below the
        /// threshold no rook/bishop is permitted. Omit (or set to 0) to
        /// disable the constraint.
        #[arg(long)]
        rook_bishop_allow_start: Option<u32>,
        /// Additional rook/bishop allowed for every increment of this many
        /// pieces in play past `--rook-bishop-allow-start`. E.g. start=20
        /// step=5 → 0 below 20, 1 at 20–24, 2 at 25–29, etc.
        #[arg(long, default_value_t = 5)]
        rook_bishop_allow_step: u32,
        /// Allow lance/knight (incl. promoted: Lance/ProLance/Knight/ProKnight)
        /// on the board only past this piece-count threshold. Pieces in play
        /// (board + black hand) must be ≥ this value for any lance/knight to
        /// appear. Below the threshold none are permitted. Like
        /// --rook-bishop-allow-start, this prunes both generated frontier and
        /// final output. Omit to disable.
        #[arg(long)]
        lance_knight_allow_start: Option<u32>,
        /// Additional lance/knight allowed for every increment of this many
        /// pieces in play past `--lance-knight-allow-start` (same formula as
        /// --rook-bishop-allow-step).
        #[arg(long, default_value_t = 5)]
        lance_knight_allow_step: u32,
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
        /// Softmax selection temperature (0 = greedy top-K). Larger = more
        /// diversity/exploration; very large approaches the random beam.
        #[arg(long, default_value_t = 0.0)]
        beam_temperature: f32,
        /// Use the embedded SOTA beam model + tuned temperature (one flag).
        #[arg(long, default_value_t = false)]
        beam_sota: bool,
        /// Round-robin select across piece-count buckets (diversity floor).
        #[arg(long, default_value_t = false)]
        beam_stratify: bool,
        /// Initial Bottom-K Sampling pool overshoot factor (default 4). After
        /// each step, automatically grows toward 1/observed_survival so Phase V
        /// can early-stop at W survivors. Always clamped by
        /// --max-candidates-pool for OOM safety.
        #[arg(long, default_value_t = 4)]
        candidates_pool_factor: usize,
        /// Hard upper bound on the Bottom-K mid pool size, in candidates.
        /// When omitted, the cap is derived dynamically from
        /// `--memory-budget-pct` instead (live RSS-aware). When set, this
        /// static cap takes precedence over the budget-derived ceiling.
        #[arg(long)]
        max_candidates_pool: Option<usize>,
        /// Memory budget for adaptive pool sizing, as a percentage of
        /// `MemTotal`. The Phase-1 candidate pool grows until projected
        /// usage exceeds this budget (recomputed each step from live RSS).
        /// Replaces the need to set `--max-candidates-pool` manually — with
        /// the default, the run uses as much memory as it can without
        /// risking OOM, so frontier stays at `--beam-width` as long as the
        /// machine has the RAM. Set to 0 to fall back to the legacy 8× W
        /// static cap.
        #[arg(long, default_value_t = 80)]
        memory_budget_pct: u32,
        /// Fleet partitioning: this instance's 0-based index.
        #[arg(long)]
        fleet_index: Option<usize>,
        /// Fleet partitioning: total number of instances.
        #[arg(long)]
        fleet_size: Option<usize>,
        /// Path to a trained oracle model (standardized_ridge_v1 JSON, as
        /// emitted by `scripts/oracle_baseline.py --out-dir`). When given,
        /// switches the seed schedule to a priority queue ordered by the
        /// oracle's predicted bpc.
        #[arg(long)]
        oracle_model: Option<PathBuf>,
        /// Smoke 用の正規化を uniqueness 判定境界で適用する (実験的)。
        /// 黒 goldish (≠ ProPawn) を ProPawn 化し、駒種情報を白持駒へ移すことで
        /// 同 goldish 占有マス集合の異種別配置を canonical に潰し memo 共有率を
        /// 上げる。合駒局面など稀なケースで false positive がありうるため、
        /// best_positions は最後に standard_solve で再検証される。
        #[arg(long, default_value_t = false)]
        canonicalize_attacker_goldish: bool,
        /// Minimum seconds between checkpoint writes per seed.
        /// Checkpointing every step generates large I/O at scale (many parallel
        /// seeds × large frontiers). Setting this to e.g. 60 reduces checkpoint
        /// writes ~60× with at most 60 seconds of lost progress on crash.
        /// Set to 0 to restore the old every-step behaviour.
        #[arg(long, default_value_t = 60)]
        checkpoint_interval_secs: u64,
        /// Stop the whole run as soon as any seed reaches the theoretical max
        /// piece count. Off by default: with the (#pieces, steps) goal,
        /// reaching max pieces is not the end (a longer-step solution may
        /// still appear), so the search keeps running unless this is set.
        #[arg(long, default_value_t = false)]
        early_exit: bool,
        /// Disable the progress heartbeat. By default a thread prints the
        /// current advance sub-phase char (P/C/V/F, `.`=idle) every 5s with
        /// no newline so a single slow step in the deep tail does not look
        /// frozen. The cost is one mostly-sleeping thread + a few relaxed
        /// atomic stores per step, so it is on by default.
        #[arg(long, default_value_t = false)]
        no_progress_ticker: bool,
        /// Memory-bounded split mode: run exact BFS to this search step, then
        /// partition the frontier into fixed-size chunks and run each chunk's
        /// BFS to completion sequentially (bounding peak memory). Exact;
        /// duplicate work across chunks is accepted. Snaps to the first search
        /// step >= this value (smoke advances in odd steps). Incompatible with
        /// --beam-width / --oracle-model. Omit to disable.
        #[arg(long)]
        split_start_step: Option<u16>,
        /// Max frontier positions per chunk in split mode. Required (and must be
        /// > 0) when --split-start-step is set.
        #[arg(long)]
        split_chunk_size: Option<usize>,
        /// Deterministic shuffle seed for split-mode chunking. Stable across
        /// resume so chunk boundaries do not change. Default 0.
        #[arg(long, default_value_t = 0)]
        split_seed: u64,
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
    /// Convert an analysis/smoke_cone dataset.csv into a training CSV
    /// (label,group,live_deeper,<features...>) via extract_features.
    #[command(name = "cone-features")]
    ConeFeatures {
        /// Input dataset CSV (analysis/smoke_cone/data/dataset.csv).
        #[arg(long)]
        dataset: PathBuf,
        /// Output training CSV.
        #[arg(long, short = 'o')]
        out: PathBuf,
        /// Dataset column to use as the regression label.
        #[arg(long, default_value = "best_piece_reachable")]
        label: String,
    },
    /// Train a beam model from the seed result log (no --feature-log needed).
    ///
    /// Solves each representative_sfen to collect intermediate positions,
    /// extracts features, and writes a CSV + trained model JSON.
    #[command(name = "train-model")]
    TrainModel {
        /// Seed result log (jsonl).
        #[arg(
            long,
            default_value = "target/single-king-smoke-ideal-backward-seeds.jsonl"
        )]
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
            memo_retain_from_step,
            no_mid_uniqueness_prune,
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
            double_king,
            goldish_priority,
            rook_bishop_allow_start,
            rook_bishop_allow_step,
            lance_knight_allow_start,
            lance_knight_allow_step,
            feature_log,
            feature_sample_per_step,
            beam_width,
            beam_model,
            beam_temperature,
            beam_stratify,
            beam_sota,
            candidates_pool_factor,
            max_candidates_pool,
            memory_budget_pct,
            fleet_index,
            fleet_size,
            oracle_model,
            canonicalize_attacker_goldish,
            checkpoint_interval_secs,
            early_exit,
            no_progress_ticker,
            split_start_step,
            split_chunk_size,
            split_seed,
        } => {
            let max_memo_entries = parse_max_memo_entries(&max_memo_entries, parallel)?;
            let beam = build_beam_config(beam_width, beam_model.as_deref(), beam_temperature, beam_stratify, beam_sota)?;
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
                max_memo_entries,
                oracle_model,
                canonicalize_attacker_goldish,
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
                    double_king,
                    goldish_priority,
                    rook_bishop_allow_start,
                    rook_bishop_allow_step,
                    lance_knight_allow_start,
                    // Normalize step to 0 when the family is unused so the new
                    // fields are skipped during serialization, keeping the
                    // condition_key (and existing checkpoints) unchanged for runs
                    // that do not use --lance-knight-allow-start.
                    lance_knight_allow_step: if lance_knight_allow_start.is_some() {
                        lance_knight_allow_step
                    } else {
                        0
                    },
                },
                mem_trace,
                FeatureLogConfig {
                    path: feature_log,
                    samples_per_step: feature_sample_per_step,
                },
                beam,
                candidates_pool_factor,
                max_candidates_pool,
                memory_budget_pct,
                checkpoint_interval_secs,
                early_exit,
                !no_progress_ticker,
                SplitConfig {
                    start_step: split_start_step,
                    chunk_size: split_chunk_size,
                    seed: split_seed,
                },
                memo_retain_from_step,
                !no_mid_uniqueness_prune,
            )
        }
        SingleKingSmokeCommand::ExportFeatures {
            feature_log,
            seed_result_log,
            out,
            min_label,
        } => train::export_features(&feature_log, &seed_result_log, &out, min_label),
        SingleKingSmokeCommand::ConeFeatures { dataset, out, label } => {
            train::export_cone_features(&dataset, &out, &label)
        }
        SingleKingSmokeCommand::TrainModel {
            seed_result_log,
            out,
            min_label,
        } => train::train_model(&seed_result_log, &out, min_label),
    }
}
