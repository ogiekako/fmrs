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
    /// Seed for the deterministic per-position Gumbel RNG (reproducible &
    /// parallel-safe: each position is seeded from its digest). Enables
    /// checkpoint/resume to continue identically.
    rng_seed: u64,
    /// Step at which beam filtering begins. `None` (or 0) = beam from step 0.
    /// Set to `--split-start-step` so the search is exact up to that step and
    /// only then switches to beam — the memory-bounded "exact core + beam tail"
    /// mode that lets a killed exact/split run resume under a width bound.
    pub(super) activate_step: Option<u16>,
}

impl BeamConfig {
    /// A short stable hash of the full beam configuration, used to namespace
    /// resume checkpoints so a beam run only resumes from a checkpoint written
    /// by an identical config (and never collides with the exact-run
    /// checkpoints). `None` when beam is off (→ exact checkpoint path unchanged).
    pub(super) fn checkpoint_key(&self) -> Option<String> {
        self.width?;
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.width.hash(&mut h);
        self.anchor_step.hash(&mut h);
        self.anchor_width.hash(&mut h);
        self.max_width.hash(&mut h);
        self.temperature.to_bits().hash(&mut h);
        self.stratify.hash(&mut h);
        self.rng_seed.hash(&mut h);
        self.activate_step.hash(&mut h);
        // Scorer fingerprint: kind + cheap content checksum so swapping the
        // model invalidates the checkpoint (safe: falls back to fresh).
        match &self.scorer {
            BeamScorer::Random => 0u8.hash(&mut h),
            BeamScorer::Handcraft => 1u8.hash(&mut h),
            BeamScorer::Model(m) => {
                2u8.hash(&mut h);
                m.weights.len().hash(&mut h);
                m.intercept.to_bits().hash(&mut h);
                for w in m.weights.iter().step_by((m.weights.len() / 16).max(1)) {
                    w.to_bits().hash(&mut h);
                }
            }
            BeamScorer::Gbdt(g) => {
                3u8.hash(&mut h);
                g.trees.len().hash(&mut h);
                g.baseline.to_bits().hash(&mut h);
                for t in g.trees.iter().step_by((g.trees.len() / 16).max(1)) {
                    t.len().hash(&mut h);
                    if let Some(n) = t.last() {
                        n.4.to_bits().hash(&mut h);
                    }
                }
            }
        }
        Some(format!("{:016x}", h.finish()))
    }

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
    rng_seed: u64,
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
            rng_seed,
            activate_step: None,
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
        rng_seed,
        activate_step: None,
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
            let step = search.step();
            let (_, positions) = search.positions();
            let n = positions.len();
            let mut indices: Vec<usize> = (0..n).collect();
            let mut rng = SmallRng::seed_from_u64(beam.rng_seed ^ (step as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
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
            let seed = beam.rng_seed;
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
                        // Deterministic per-position seed (digest-based): reproducible
                        // and parallel-safe (no shared RNG), and cheaper than
                        // from_entropy's per-call OS-entropy read.
                        let mut rng = SmallRng::seed_from_u64(
                            p.digest()
                                ^ (step as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                                ^ seed,
                        );
                        let u: f32 = rng.gen::<f32>().clamp(1e-9, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(width: Option<usize>) -> anyhow::Result<BeamConfig> {
        // Random scorer; vary only the fields we want to test below.
        build_beam_config(width, None, 10.0, false, false, None, 0, 0, 7)
    }

    #[test]
    fn checkpoint_key_none_when_beam_off() {
        assert!(cfg(None).unwrap().checkpoint_key().is_none());
    }

    #[test]
    fn checkpoint_key_stable_for_identical_config() {
        let a = cfg(Some(1000)).unwrap();
        let b = cfg(Some(1000)).unwrap();
        assert_eq!(a.checkpoint_key(), b.checkpoint_key());
        assert!(a.checkpoint_key().is_some());
    }

    #[test]
    fn checkpoint_key_differs_on_config_change() {
        let base = cfg(Some(1000)).unwrap().checkpoint_key();
        // width
        assert_ne!(base, cfg(Some(2000)).unwrap().checkpoint_key());
        // temperature
        let t = build_beam_config(Some(1000), None, 15.0, false, false, None, 0, 0, 7)
            .unwrap()
            .checkpoint_key();
        assert_ne!(base, t);
        // stratify
        let s = build_beam_config(Some(1000), None, 10.0, true, false, None, 0, 0, 7)
            .unwrap()
            .checkpoint_key();
        assert_ne!(base, s);
        // rng_seed
        let r = build_beam_config(Some(1000), None, 10.0, false, false, None, 0, 0, 99)
            .unwrap()
            .checkpoint_key();
        assert_ne!(base, r);
        // anchor (width ramp)
        let an = build_beam_config(Some(1000), None, 10.0, false, false, Some(50), 5000, 0, 7)
            .unwrap()
            .checkpoint_key();
        assert_ne!(base, an);
        // activate_step (split+beam exact-prefix boundary)
        let mut act = cfg(Some(1000)).unwrap();
        act.activate_step = Some(40);
        assert_ne!(base, act.checkpoint_key());
    }

    #[test]
    fn checkpoint_key_differs_by_scorer_kind() {
        let random = cfg(Some(1000)).unwrap().checkpoint_key();
        let handcraft =
            build_beam_config(Some(1000), Some("handcraft"), 10.0, false, false, None, 0, 0, 7)
                .unwrap()
                .checkpoint_key();
        assert_ne!(random, handcraft);
    }
}
