use anyhow::Context as _;
use fmrs_core::{
    position::{position::PositionAux, Position},
    search::backward::BackwardSearch,
};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::super::smoke_features::{extract_features, GbdtModel, LinearModel};

#[derive(Clone, Default)]
pub(super) struct FeatureLogConfig {
    pub(super) path: Option<PathBuf>,
    pub(super) samples_per_step: usize,
}

#[derive(Clone)]
enum BeamScorer {
    Random,
    Handcraft,
    Model(LinearModel),
    Gbdt(GbdtModel),
}

pub(super) struct BeamConfig {
    pub(super) width: Option<usize>,
    scorer: BeamScorer,
    /// Softmax temperature for selection. 0 = deterministic top-K (greedy,
    /// low diversity). Larger = more exploration; T→∞ approaches the uniform
    /// (random) beam. Implemented via the Gumbel-top-K trick (sampling K
    /// without replacement ∝ exp(score/T)), which keeps high-value positions
    /// while preserving diversity — the lever that lets a value model beat the
    /// pure-random beam.
    temperature: f32,
    /// Round-robin select across piece-count buckets (diversity floor that
    /// prevents the high-piece front from crowding out longer-surviving lines).
    stratify: bool,
    /// Geometric width schedule: `width` is W0 (at step 0); at `anchor_step` the
    /// width is `anchor_width`; interpolated geometrically (log-linear) since the
    /// frontier grows geometrically, so this keeps the kept-fraction's decay
    /// controlled. Beyond the anchor it keeps growing geometrically, capped by
    /// `max_width`. None anchor = constant width (legacy).
    anchor_step: Option<u16>,
    anchor_width: usize,
    max_width: usize, // 0 = uncapped
}

impl BeamConfig {
    /// Beam width to use at `step` (geometric interpolation between the base
    /// width at step 0 and `anchor_width` at `anchor_step`, growing past the
    /// anchor, capped by `max_width`). `None` if beam is off.
    pub(super) fn width_at(&self, step: u16) -> Option<usize> {
        let w0 = self.width?;
        let Some(s1) = self.anchor_step.filter(|&s| s > 0) else {
            return Some(w0);
        };
        let ratio = self.anchor_width as f64 / w0 as f64;
        let w = (w0 as f64 * ratio.powf(step as f64 / s1 as f64)).round() as usize;
        let w = w.max(1);
        Some(if self.max_width > 0 { w.min(self.max_width) } else { w })
    }
}

impl BeamConfig {
    /// True when a scorer (model or handcraft) actually ranks positions, as
    /// opposed to the random/digest beam. When true the Phase-1 candidate pool
    /// is kept `BEAM_SCORE_POOL`× wider than `width` so `apply_beam` has a pool
    /// to score-select `width` from (otherwise the deterministic bottom-K-by-
    /// digest truncation in `advance` already cuts to `width` and the scorer
    /// never runs).
    pub(super) fn uses_scorer(&self) -> bool {
        matches!(
            self.scorer,
            BeamScorer::Model(_) | BeamScorer::Handcraft | BeamScorer::Gbdt(_)
        )
    }
}

/// How many ×width candidates to retain in the Phase-1 pool when a scorer is
/// active, so the scorer picks the best `width` from a `width × POOL` sample.
pub(super) const BEAM_SCORE_POOL: usize = 16;

/// SOTA beam model (GBDT trained on exact value-DP labels), embedded so
/// `--beam-sota` works with no external file. See analysis/smoke_cone.
const SOTA_MODEL_JSON: &str = include_str!("../../../models/beam_sota.json");
/// Default selection temperature paired with the SOTA model.
pub(super) const SOTA_TEMPERATURE: f32 = 15.0;

pub(super) fn build_beam_config(
    width: Option<usize>,
    model_spec: Option<&str>,
    temperature: f32,
    stratify: bool,
    sota: bool,
    anchor_step: Option<u16>,
    anchor_width: usize,
    max_width: usize,
) -> anyhow::Result<BeamConfig> {
    // --beam-sota: use the embedded SOTA GBDT (unless an explicit --beam-model
    // overrides it) and default the temperature to the tuned value.
    if sota && model_spec.is_none() {
        let temp = if temperature > 0.0 { temperature } else { SOTA_TEMPERATURE };
        return Ok(BeamConfig {
            width,
            scorer: BeamScorer::Gbdt(GbdtModel::from_json_str(SOTA_MODEL_JSON)?),
            temperature: temp,
            stratify,
            anchor_step,
            anchor_width,
            max_width,
        });
    }
    let scorer = match model_spec {
        None => BeamScorer::Random,
        Some("handcraft") => BeamScorer::Handcraft,
        Some(path) => {
            // GBDT JSON has a "trees" field; linear has "weights".
            let data = std::fs::read_to_string(path)
                .with_context(|| format!("read beam model {path}"))?;
            if data.contains("\"trees\"") {
                BeamScorer::Gbdt(GbdtModel::load(Path::new(path))?)
            } else {
                BeamScorer::Model(LinearModel::load(Path::new(path))?)
            }
        }
    };
    Ok(BeamConfig {
        width,
        scorer,
        temperature,
        stratify,
        anchor_step,
        anchor_width,
        max_width,
    })
}

pub(super) fn open_feature_log(path: &Path) -> anyhow::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open feature log {}", path.display()))
}

/// Returns `true` if filtering actually reduced the frontier, `false` if the
/// frontier was already within `width` (no pruning occurred).
pub(super) fn apply_beam(search: &mut BackwardSearch, beam: &BeamConfig, width: usize) -> bool {
    let (_, positions) = search.positions();
    if positions.len() <= width || width == 0 {
        return false;
    }
    match &beam.scorer {
        BeamScorer::Random => {
            let (_, positions) = search.positions();
            let n = positions.len();
            let mut indices: Vec<usize> = (0..n).collect();
            let mut rng = SmallRng::from_entropy();
            indices.partial_shuffle(&mut rng, width);
            let kept: Vec<Position> = indices[..width]
                .iter()
                .map(|&i| positions[i].clone())
                .collect();
            search.replace_positions(kept);
        }
        scorer => {
            let step = search.step();
            let temp = beam.temperature;
            let (stone, positions) = search.positions();
            let mut scored: Vec<(f32, u32, Position)> = positions
                .par_iter()
                .map(|p| {
                    let aux = PositionAux::new(p.clone(), stone);
                    let features = extract_features(&aux, step);
                    let mut score = match scorer {
                        BeamScorer::Model(m) => m.score(&features),
                        BeamScorer::Gbdt(g) => g.score(&features),
                        _ => handcraft_beam_score(&features),
                    };
                    // Gumbel-top-K: perturbing by T·Gumbel and taking top-K
                    // samples K without replacement ∝ exp(score/T), keeping
                    // value while preserving diversity.
                    if temp > 0.0 {
                        let u: f32 = SmallRng::from_entropy().gen::<f32>().clamp(1e-9, 1.0);
                        score += temp * -(-u.ln()).ln();
                    }
                    let pieces = aux.occupied_bb().count_ones();
                    (score, pieces, p.clone())
                })
                .collect();
            let truncated: Vec<Position> = if beam.stratify {
                // Stratified: keep a balanced spread across piece counts by
                // round-robin taking the best-scoring position from each
                // piece-count bucket. Guarantees lower-piece (often longer-
                // surviving) lines aren't crowded out by the high-piece front,
                // which is what makes a pure value/temperature beam collapse.
                let mut buckets: std::collections::BTreeMap<u32, Vec<(f32, Position)>> =
                    std::collections::BTreeMap::new();
                for (s, pc, p) in scored {
                    buckets.entry(pc).or_default().push((s, p));
                }
                for v in buckets.values_mut() {
                    v.sort_unstable_by(|a, b| {
                        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                let mut cursors: Vec<(u32, usize)> = buckets.keys().map(|&k| (k, 0usize)).collect();
                let mut kept = Vec::with_capacity(width);
                while kept.len() < width {
                    let mut any = false;
                    for (k, cur) in cursors.iter_mut() {
                        let b = &buckets[k];
                        if *cur < b.len() {
                            kept.push(b[*cur].1.clone());
                            *cur += 1;
                            any = true;
                            if kept.len() >= width {
                                break;
                            }
                        }
                    }
                    if !any {
                        break;
                    }
                }
                kept
            } else {
                scored.select_nth_unstable_by(width - 1, |a, b| {
                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                });
                scored.into_iter().take(width).map(|(_, _, p)| p).collect()
            };
            search.replace_positions(truncated);
        }
    }
    true
}

fn handcraft_beam_score(features: &[f32]) -> f32 {
    let names = super::super::smoke_features::feature_names();
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

pub(super) fn sample_features_to_log(
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
        let features = extract_features(&aux, step);
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
