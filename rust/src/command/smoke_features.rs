//! Feature extraction and a tiny linear scoring model for beam search and
//! offline analysis of single-king-smoke positions.
//!
//! `extract_features` returns a Vec<f32> of fixed length matching
//! `feature_names()`. The order is stable; a unit test enforces the lengths
//! agree.

use std::path::Path;

use anyhow::Context as _;
use fmrs_core::piece::{Color, Kind, KINDS, NUM_HAND_KIND};
use fmrs_core::position::advance::advance::advance_aux;
use fmrs_core::position::advance::AdvanceOptions;
use fmrs_core::position::bitboard::rule::{king_power, reachable_sub};
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{checked, BitBoard, Movement, Square};
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
    // Phase: the right scorer differs early vs late, so step is a feature.
    v.push("step");
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
    // --- composer-intuition features ---
    // King freedom / promise (room before forced collapse).
    v.push("king_liberties");
    v.push("king_safe_flights");
    v.push("king_flight_cov_avg");
    v.push("king_overcovered_flights");
    v.push("king_escape_depth");
    v.push("king_net_frac");
    v.push("king_ray_freedom");
    v.push("white_mobility");
    // Dispersion / placement quality.
    v.push("king_centroid_cheby");
    v.push("black_far_from_king");
    v.push("white_nonking_far");
    v.push("bbox_area");
    v.push("board_density");
    v.push("occupied_files");
    v.push("occupied_ranks");
    v.push("row_std");
    v.push("col_std");
    v.push("promoted_total");
    // Heavy (cook/余詰ぽさ): # of black moves giving check. Computed only when
    // env FMRS_FEAT_HEAVY != "0" (else 0.0); the column stays in the schema so
    // models are comparable and the runtime cost is opt-in for beam.
    v.push("black_check_moves");
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
pub fn extract_features(position: &PositionAux, step: u16) -> Vec<f32> {
    let mut f: Vec<f32> = Vec::with_capacity(96);
    let occupied = position.occupied_bb();
    let black = position.black_bb();
    let white = position.white_bb();
    let hands = position.hands();

    f.push(step as f32);
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

    // Black kiki (per-square attacker counts) — computed once and reused by both
    // the king block and the composer-intuition block below (it iterates every
    // black piece's reach, so it is the costliest part of extract_features).
    let (total_kiki, black_cnt) = black_kiki_per_square(position);

    if let Some(kp) = king_pos_opt {
        f.push(kp.row() as f32);
        f.push(kp.col() as f32);
        let r = kp.row();
        let c = kp.col();
        let min_edge = r.min(8 - r).min(c.min(8 - c)) as f32;
        f.push(min_edge);

        let ring1 = king_power(kp);
        let ring1_black = (ring1 & black).count_ones();
        let ring1_white = (ring1 & white).count_ones();
        f.push(ring1_black as f32);
        f.push(ring1_white as f32);

        let ring2 = ring2_around(kp);
        f.push((ring2 & black).count_ones() as f32);

        // Attacker count and kiki on king ring.
        let attackers_on_king = black_cnt[kp.index()];
        f.push(attackers_on_king as f32);

        let mut neighbors_attacked: u32 = 0;
        for s in ring1 {
            if black_cnt[s.index()] > 0 {
                neighbors_attacked += 1;
            }
        }
        f.push(neighbors_attacked as f32);

        f.push(total_kiki as f32);

        let overcovered = black_cnt.iter().filter(|&&n| n >= 2).count() as f32;
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

    // --- composer-intuition features ---
    if let Some(kp) = king_pos_opt {
        let ring1 = king_power(kp);
        let ring1_n = ring1.count_ones().max(1);
        let mut liberties = 0u32;
        let mut safe_flights = 0u32;
        let mut flight_cov_sum = 0u32;
        let mut flight_cnt = 0u32;
        let mut overcovered_flights = 0u32;
        let mut escape_depth = 0u32;
        let mut netted = 0u32;
        for s in ring1 {
            let empty = !occupied.contains(s);
            let cov = black_cnt[s.index()] as u32;
            if empty {
                liberties += 1;
                flight_cov_sum += cov;
                flight_cnt += 1;
                if cov == 0 {
                    safe_flights += 1;
                    for t in king_power(s) {
                        if !occupied.contains(t) && black_cnt[t.index()] == 0 {
                            escape_depth += 1;
                        }
                    }
                }
                if cov >= 2 {
                    overcovered_flights += 1;
                }
            }
            if !empty || cov > 0 {
                netted += 1; // neighbor blocked or covered
            }
        }
        f.push(liberties as f32);
        f.push(safe_flights as f32);
        f.push(if flight_cnt > 0 {
            flight_cov_sum as f32 / flight_cnt as f32
        } else {
            0.0
        });
        f.push(overcovered_flights as f32);
        f.push(escape_depth as f32);
        f.push(netted as f32 / ring1_n as f32);
        f.push(ray_freedom(&occupied, kp) as f32);
    } else {
        for _ in 0..7 {
            f.push(0.0);
        }
    }
    f.push(white_mobility(position) as f32);

    // Dispersion / placement quality.
    let (mut sum_r, mut sum_c, mut n_occ) = (0i32, 0i32, 0i32);
    let mut file_mask: u32 = 0;
    let mut rank_mask: u32 = 0;
    for s in occupied {
        sum_r += s.row() as i32;
        sum_c += s.col() as i32;
        n_occ += 1;
        file_mask |= 1u32 << s.col();
        rank_mask |= 1u32 << s.row();
    }
    let (cen_r, cen_c) = if n_occ > 0 {
        (sum_r as f32 / n_occ as f32, sum_c as f32 / n_occ as f32)
    } else {
        (4.0, 4.0)
    };
    if let Some(kp) = king_pos_opt {
        let dr = (kp.row() as f32 - cen_r).abs();
        let dc = (kp.col() as f32 - cen_c).abs();
        f.push(dr.max(dc));
        let mut black_far = 0u32;
        let mut white_far = 0u32;
        for s in occupied {
            let d = (s.row() as i32 - kp.row() as i32)
                .abs()
                .max((s.col() as i32 - kp.col() as i32).abs());
            if d > 2 {
                if black.contains(s) {
                    black_far += 1;
                } else if white.contains(s) && s != kp {
                    white_far += 1;
                }
            }
        }
        f.push(black_far as f32);
        f.push(white_far as f32);
    } else {
        for _ in 0..3 {
            f.push(0.0);
        }
    }
    let bbox_area = if max_r >= 0 {
        ((max_r - min_r + 1) * (max_c - min_c + 1)) as f32
    } else {
        0.0
    };
    f.push(bbox_area);
    f.push(if bbox_area > 0.0 {
        occupied.count_ones() as f32 / bbox_area
    } else {
        0.0
    });
    f.push(file_mask.count_ones() as f32);
    f.push(rank_mask.count_ones() as f32);
    // Std of occupied rows/cols.
    let (mut var_r, mut var_c) = (0f32, 0f32);
    for s in occupied {
        var_r += (s.row() as f32 - cen_r).powi(2);
        var_c += (s.col() as f32 - cen_c).powi(2);
    }
    if n_occ > 0 {
        var_r /= n_occ as f32;
        var_c /= n_occ as f32;
    }
    f.push(var_r.sqrt());
    f.push(var_c.sqrt());
    let mut promoted_total = 0u32;
    for &(kind, _) in PER_KIND_NAMES[NUM_HAND_KIND..].iter() {
        promoted_total += position.bitboard(Color::BLACK, kind).count_ones();
        promoted_total += position.bitboard(Color::WHITE, kind).count_ones();
    }
    f.push(promoted_total as f32);

    // Heavy: # of black moves giving check (cook/余詰 proximity). Opt-in.
    // Cache the env read once — extract_features runs per position in the beam.
    static HEAVY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let heavy = *HEAVY.get_or_init(|| {
        std::env::var("FMRS_FEAT_HEAVY").map(|v| v != "0").unwrap_or(false)
    });
    f.push(if heavy {
        count_black_check_moves(position) as f32
    } else {
        0.0
    });

    debug_assert_eq!(f.len(), feature_names().len());
    f
}

/// Empty squares along the 8 king rays until the first occupied square or edge
/// (a measure of open space around the king).
fn ray_freedom(occupied: &BitBoard, king: Square) -> u32 {
    let (kr, kc) = (king.row() as i32, king.col() as i32);
    let mut total = 0u32;
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let (mut r, mut c) = (kr + dr, kc + dc);
            while (0..9).contains(&r) && (0..9).contains(&c) {
                let s = Square::new(c as usize, r as usize);
                if occupied.contains(s) {
                    break;
                }
                total += 1;
                r += dr;
                c += dc;
            }
        }
    }
    total
}

/// Total white kiki (white mobility proxy = sum of reach over white pieces).
fn white_mobility(position: &PositionAux) -> u32 {
    let white = position.white_bb();
    let mut total = 0u32;
    for &(kind, _) in PER_KIND_NAMES.iter() {
        let mut bb = position.bitboard(Color::WHITE, kind) & white;
        while let Some(sq) = bb.next() {
            total += reachable_sub(position, Color::WHITE, sq, kind).count_ones();
        }
    }
    total
}

/// Number of black moves (black to move) that put the white king in check.
fn count_black_check_moves(position: &PositionAux) -> u32 {
    if !position.turn().is_black() {
        return 0;
    }
    let mut moves: Vec<Movement> = Vec::new();
    let mut p = position.clone();
    if advance_aux(&mut p, &AdvanceOptions::default(), &mut moves).is_err() {
        return 0;
    }
    let mut checks = 0u32;
    for m in &moves {
        let mut q = position.clone();
        q.do_move(m);
        if checked(&mut q, Color::WHITE) {
            checks += 1;
        }
    }
    checks
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

/// A gradient-boosted tree ensemble (exported from sklearn
/// HistGradientBoostingRegressor by analysis/smoke_cone/export_gbdt.py).
/// Captures the nonlinear within-cell signal the LinearModel can't (per-cell
/// Spearman ~0.32 vs ~0.20). score = baseline + Σ over trees of the reached leaf
/// value. Each node is (feature_idx, threshold, left, right, value, is_leaf);
/// a leaf returns `value`, else go `left` if x[feature] <= threshold else right.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GbdtModel {
    pub feature_names: Vec<String>,
    pub baseline: f32,
    pub trees: Vec<Vec<(u32, f32, u32, u32, f32, u8)>>,
}

impl GbdtModel {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("read gbdt model {}", path.display()))?;
        Self::from_json_str(&data)
    }

    pub fn from_json_str(data: &str) -> anyhow::Result<Self> {
        let model: GbdtModel =
            serde_json::from_str(data).context("parse gbdt model")?;
        let expected = feature_names();
        anyhow::ensure!(
            model.feature_names.len() == expected.len()
                && model
                    .feature_names
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, b)| a == b),
            "gbdt model feature_names do not match the current schema"
        );
        Ok(model)
    }

    #[inline]
    pub fn score(&self, features: &[f32]) -> f32 {
        let mut s = self.baseline;
        for tree in &self.trees {
            let mut i = 0usize;
            loop {
                let (feat, thr, left, right, value, is_leaf) = tree[i];
                if is_leaf != 0 {
                    s += value;
                    break;
                }
                i = if features[feat as usize] <= thr {
                    left as usize
                } else {
                    right as usize
                };
            }
        }
        s
    }
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
        let f = extract_features(&p, 5);
        assert_eq!(f.len(), feature_names().len());
    }

    #[test]
    fn deterministic_on_simple_position() {
        let mut p = PositionAux::default();
        p.set(Square::S19, Color::WHITE, Kind::King);
        p.set(Square::S18, Color::BLACK, Kind::Gold);
        let a = extract_features(&p, 5);
        let b = extract_features(&p, 5);
        assert_eq!(a, b);
    }

    #[test]
    fn detects_king_neighbors_and_kiki() {
        let mut p = PositionAux::default();
        // White king at 5五, black gold at 5六 (one row below king)
        p.set(Square::S55, Color::WHITE, Kind::King);
        p.set(Square::S56, Color::BLACK, Kind::Gold);
        let names = feature_names();
        let f = extract_features(&p, 5);
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
        let dir = std::env::temp_dir().join(format!("fmrs-model-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.json");
        std::fs::write(&path, serde_json::to_string(&model).unwrap()).unwrap();
        let loaded = LinearModel::load(&path).unwrap();
        let features: Vec<f32> = (0..names.len()).map(|i| i as f32).collect();
        let expected = 1.5
            + (0..names.len())
                .map(|i| (i as f32) * (i as f32) * 0.01)
                .sum::<f32>();
        let score = loaded.score(&features);
        assert!(
            (score - expected).abs() < 1e-3,
            "score={} expected={}",
            score,
            expected
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn linear_model_load_rejects_mismatched_schema() {
        let model = LinearModel {
            feature_names: vec!["wrong".into(), "names".into()],
            weights: vec![1.0, 2.0],
            intercept: 0.0,
        };
        let dir = std::env::temp_dir().join(format!("fmrs-model-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.json");
        std::fs::write(&path, serde_json::to_string(&model).unwrap()).unwrap();
        assert!(LinearModel::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
