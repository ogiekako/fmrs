//! Feature extraction and a tiny linear scoring model for beam search and
//! offline analysis of single-king-smoke positions.
//!
//! `extract_features` returns a Vec<f32> of fixed length matching
//! `feature_names()`. The order is stable; a unit test enforces the lengths
//! agree.

use std::path::Path;

use anyhow::Context as _;
use fmrs_core::piece::{Color, Kind, KINDS, NUM_HAND_KIND};
use fmrs_core::position::bitboard::rule::{king_power, reachable_sub};
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{BitBoard, Square};
use serde::{Deserialize, Serialize};

const PER_KIND_NAMES: [(Kind, &str); 14] = [
    (Kind::Pawn, "pawn"),
    (Kind::Lance, "lance"),
    (Kind::Knight, "knight"),
    (Kind::Silver, "silver"),
    (Kind::Gold, "gold"),
    (Kind::Bishop, "bishop"),
    (Kind::Rook, "rook"),
    (Kind::King, "king"),
    (Kind::ProPawn, "ppawn"),
    (Kind::ProLance, "plance"),
    (Kind::ProKnight, "pknight"),
    (Kind::ProSilver, "psilver"),
    (Kind::ProBishop, "pbishop"),
    (Kind::ProRook, "prook"),
];

/// Returns feature names in extraction order.
pub fn feature_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("board_total");
    v.push("board_black");
    v.push("board_white");
    v.push("hand_black_total");
    v.push("hand_white_total");
    for &(_, n) in PER_KIND_NAMES.iter() {
        v.push(static_concat("board_black_", n));
    }
    for &(_, n) in PER_KIND_NAMES.iter() {
        v.push(static_concat("board_white_", n));
    }
    for &(_, n) in PER_KIND_NAMES[..NUM_HAND_KIND].iter() {
        v.push(static_concat("hand_black_", n));
    }
    for &(_, n) in PER_KIND_NAMES[..NUM_HAND_KIND].iter() {
        v.push(static_concat("hand_white_", n));
    }
    v.push("king_white_row");
    v.push("king_white_col");
    v.push("king_white_min_edge_dist");
    v.push("king_white_neighbors_black");
    v.push("king_white_neighbors_white");
    v.push("king_white_2ring_black");
    v.push("king_white_attackers");
    v.push("king_white_neighbors_attacked");
    v.push("total_black_kiki");
    v.push("overcovered_squares");
    v.push("empty_squares");
    v.push("board_min_row");
    v.push("board_max_row");
    v.push("board_min_col");
    v.push("board_max_col");
    v.push("board_row_spread");
    v.push("board_col_spread");
    v.push("black_pawn_columns");
    v
}

// Concatenate two static strings into a static. Done at runtime via leak;
// only called once per process from feature_names()/feature_index_map().
fn static_concat(a: &str, b: &str) -> &'static str {
    let s = format!("{a}{b}");
    Box::leak(s.into_boxed_str())
}

/// Computes a feature vector for the given position (white = mate target).
/// The output's length equals `feature_names().len()`.
pub fn extract_features(position: &PositionAux) -> Vec<f32> {
    let mut f: Vec<f32> = Vec::with_capacity(80);
    let occupied = position.occupied_bb();
    let black = position.black_bb();
    let white = position.white_bb();
    let hands = position.hands();

    f.push(occupied.count_ones() as f32);
    f.push(black.count_ones() as f32);
    f.push(white.count_ones() as f32);
    let hand_black_total: u32 = KINDS[..NUM_HAND_KIND]
        .iter()
        .map(|&k| hands.count(Color::BLACK, k) as u32)
        .sum();
    let hand_white_total: u32 = KINDS[..NUM_HAND_KIND]
        .iter()
        .map(|&k| hands.count(Color::WHITE, k) as u32)
        .sum();
    f.push(hand_black_total as f32);
    f.push(hand_white_total as f32);

    for &(kind, _) in PER_KIND_NAMES.iter() {
        f.push(position.bitboard(Color::BLACK, kind).count_ones() as f32);
    }
    for &(kind, _) in PER_KIND_NAMES.iter() {
        f.push(position.bitboard(Color::WHITE, kind).count_ones() as f32);
    }
    for &(kind, _) in PER_KIND_NAMES[..NUM_HAND_KIND].iter() {
        f.push(hands.count(Color::BLACK, kind) as f32);
    }
    for &(kind, _) in PER_KIND_NAMES[..NUM_HAND_KIND].iter() {
        f.push(hands.count(Color::WHITE, kind) as f32);
    }

    // King-relative (white king is the mate target).
    let mut king_bb = position.bitboard(Color::WHITE, Kind::King);
    let king_pos_opt: Option<Square> = if king_bb.count_ones() == 1 {
        Some(king_bb.singleton())
    } else {
        // Should not happen in well-formed smoke positions, but guard.
        king_bb.next()
    };

    if let Some(kp) = king_pos_opt {
        f.push(kp.row() as f32);
        f.push(kp.col() as f32);
        let r = kp.row();
        let c = kp.col();
        let min_edge =
            r.min(8 - r).min(c.min(8 - c)) as f32;
        f.push(min_edge);

        let ring1 = king_power(kp);
        let ring1_black = (ring1 & black).count_ones();
        let ring1_white = (ring1 & white).count_ones();
        f.push(ring1_black as f32);
        f.push(ring1_white as f32);

        let ring2 = ring2_around(kp);
        f.push((ring2 & black).count_ones() as f32);

        // Attacker count and kiki on king ring.
        let (total_kiki, attacker_count_per_square) =
            black_kiki_per_square(position);
        let attackers_on_king = attacker_count_per_square[kp.index()];
        f.push(attackers_on_king as f32);

        let mut neighbors_attacked: u32 = 0;
        for s in ring1 {
            if attacker_count_per_square[s.index()] > 0 {
                neighbors_attacked += 1;
            }
        }
        f.push(neighbors_attacked as f32);

        f.push(total_kiki as f32);

        let overcovered = attacker_count_per_square
            .iter()
            .filter(|&&n| n >= 2)
            .count() as f32;
        f.push(overcovered);
    } else {
        // No white king: pad with zeros to keep schema stable.
        for _ in 0..10 {
            f.push(0.0);
        }
    }

    let empty = 81 - occupied.count_ones();
    f.push(empty as f32);

    let mut min_r = 9i32;
    let mut max_r = -1i32;
    let mut min_c = 9i32;
    let mut max_c = -1i32;
    for s in occupied {
        let r = s.row() as i32;
        let c = s.col() as i32;
        if r < min_r {
            min_r = r;
        }
        if r > max_r {
            max_r = r;
        }
        if c < min_c {
            min_c = c;
        }
        if c > max_c {
            max_c = c;
        }
    }
    if max_r < 0 {
        // Empty board.
        f.push(0.0);
        f.push(0.0);
        f.push(0.0);
        f.push(0.0);
        f.push(0.0);
        f.push(0.0);
    } else {
        f.push(min_r as f32);
        f.push(max_r as f32);
        f.push(min_c as f32);
        f.push(max_c as f32);
        f.push((max_r - min_r) as f32);
        f.push((max_c - min_c) as f32);
    }

    let pawn_bb = position.bitboard(Color::BLACK, Kind::Pawn);
    let mut col_mask: u32 = 0;
    for s in pawn_bb {
        col_mask |= 1u32 << s.col();
    }
    f.push(col_mask.count_ones() as f32);

    debug_assert_eq!(f.len(), feature_names().len());
    f
}

fn ring2_around(king: Square) -> BitBoard {
    let kr = king.row() as i32;
    let kc = king.col() as i32;
    let mut bb = BitBoard::default();
    for dr in -2..=2 {
        for dc in -2..=2 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let r = kr + dr;
            let c = kc + dc;
            if (0..9).contains(&r) && (0..9).contains(&c) {
                bb.set(Square::new(c as usize, r as usize));
            }
        }
    }
    bb
}

/// Sum of black kiki across the board, plus a per-square attacker count.
fn black_kiki_per_square(position: &PositionAux) -> (u32, [u8; 81]) {
    let mut counts = [0u8; 81];
    let mut total = 0u32;
    let black = position.black_bb();
    for &(kind, _) in PER_KIND_NAMES.iter() {
        let mut bb = position.bitboard(Color::BLACK, kind) & black;
        while let Some(sq) = bb.next() {
            let reach = reachable_sub(position, Color::BLACK, sq, kind);
            total += reach.count_ones();
            for r in reach {
                counts[r.index()] = counts[r.index()].saturating_add(1);
            }
        }
    }
    (total, counts)
}

/// A linear model: score = dot(features, weights) + intercept.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinearModel {
    pub feature_names: Vec<String>,
    pub weights: Vec<f32>,
    pub intercept: f32,
}

impl LinearModel {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("read model {}", path.display()))?;
        let model: LinearModel = serde_json::from_str(&data)
            .with_context(|| format!("parse model {}", path.display()))?;
        anyhow::ensure!(
            model.feature_names.len() == model.weights.len(),
            "model has {} feature names but {} weights",
            model.feature_names.len(),
            model.weights.len()
        );
        let expected = feature_names();
        anyhow::ensure!(
            model.feature_names.len() == expected.len()
                && model
                    .feature_names
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, b)| a == b),
            "model feature_names do not match the current schema"
        );
        Ok(model)
    }

    pub fn score(&self, features: &[f32]) -> f32 {
        debug_assert_eq!(features.len(), self.weights.len());
        let mut s = self.intercept;
        for (f, w) in features.iter().zip(self.weights.iter()) {
            s += f * w;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fmrs_core::piece::{Color, Kind};
    use fmrs_core::position::Square;

    #[test]
    fn names_and_features_have_same_length() {
        let mut p = PositionAux::default();
        p.set(Square::S55, Color::WHITE, Kind::King);
        p.set(Square::S54, Color::BLACK, Kind::Gold);
        let f = extract_features(&p);
        assert_eq!(f.len(), feature_names().len());
    }

    #[test]
    fn deterministic_on_simple_position() {
        let mut p = PositionAux::default();
        p.set(Square::S19, Color::WHITE, Kind::King);
        p.set(Square::S18, Color::BLACK, Kind::Gold);
        let a = extract_features(&p);
        let b = extract_features(&p);
        assert_eq!(a, b);
    }

    #[test]
    fn detects_king_neighbors_and_kiki() {
        let mut p = PositionAux::default();
        // White king at 5五, black gold at 5六 (one row below king)
        p.set(Square::S55, Color::WHITE, Kind::King);
        p.set(Square::S56, Color::BLACK, Kind::Gold);
        let names = feature_names();
        let f = extract_features(&p);
        let idx = |n: &str| names.iter().position(|x| *x == n).unwrap();

        assert_eq!(f[idx("king_white_neighbors_black")], 1.0);
        // Gold attacks 6 squares; one of them is the king square (5五).
        assert!(f[idx("total_black_kiki")] >= 1.0);
        assert_eq!(f[idx("king_white_attackers")], 1.0);
    }

    #[test]
    fn linear_model_roundtrips_and_scores() {
        let names = feature_names();
        let weights: Vec<f32> = (0..names.len()).map(|i| (i as f32) * 0.01).collect();
        let model = LinearModel {
            feature_names: names.iter().map(|s| s.to_string()).collect(),
            weights: weights.clone(),
            intercept: 1.5,
        };
        let dir = std::env::temp_dir().join(format!(
            "fmrs-model-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.json");
        std::fs::write(&path, serde_json::to_string(&model).unwrap()).unwrap();
        let loaded = LinearModel::load(&path).unwrap();
        let features: Vec<f32> = (0..names.len()).map(|i| i as f32).collect();
        let expected = 1.5 + (0..names.len()).map(|i| (i as f32) * (i as f32) * 0.01).sum::<f32>();
        let score = loaded.score(&features);
        assert!((score - expected).abs() < 1e-3, "score={} expected={}", score, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn linear_model_load_rejects_mismatched_schema() {
        let model = LinearModel {
            feature_names: vec!["wrong".into(), "names".into()],
            weights: vec![1.0, 2.0],
            intercept: 0.0,
        };
        let dir = std::env::temp_dir().join(format!(
            "fmrs-model-bad-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.json");
        std::fs::write(&path, serde_json::to_string(&model).unwrap()).unwrap();
        assert!(LinearModel::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
