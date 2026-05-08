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

use super::super::smoke_features::{extract_features, LinearModel};

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
}

pub(super) struct BeamConfig {
    pub(super) width: Option<usize>,
    scorer: BeamScorer,
}

pub(super) fn build_beam_config(
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

pub(super) fn open_feature_log(path: &Path) -> anyhow::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open feature log {}", path.display()))
}

pub(super) fn apply_beam(search: &mut BackwardSearch, beam: &BeamConfig, width: usize) {
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
