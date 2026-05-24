use anyhow::Context as _;
use fmrs_core::{
    piece::{Color, Kind, KINDS},
    position::{
        advance::{advance::advance_aux, AdvanceOptions},
        bitboard::rule::power,
        position::PositionAux,
        previous, Movement, Square,
    },
};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use std::hash::{Hash, Hasher};

use super::super::smoke_constraints::{
    canonical_sfen, kind_allowed_by_mask, satisfies_ideal_smoke_undo_candidate,
    satisfies_search_constraints, square_in_bounds, with_white_complement, SearchConstraints,
};

/// Return a path like `<exe_dir>/enum_<hash>.sfens` for caching enumerated SFENs.
/// Returns None if the exe path can't be determined.
fn enum_cache_path(constraints: SearchConstraints) -> Option<std::path::PathBuf> {
    let json = serde_json::to_string(&constraints).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    let hash = hasher.finish();
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(dir.join(format!("enum_{hash:016x}.sfens")))
}

pub(super) fn enumerate_final_2_sfens(
    parallel: usize,
    constraints: SearchConstraints,
) -> anyhow::Result<Vec<String>> {
    // Try reading from disk cache before enumerating.
    if let Some(cache_path) = enum_cache_path(constraints) {
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            let sfens: Vec<String> = content.lines().map(str::to_owned).collect();
            if !sfens.is_empty() {
                return Ok(sfens);
            }
        }
    }

    let kind_pairs = if constraints.double_king {
        vec![]
    } else {
        black_piece_kind_pairs()
    };
    let positions = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel)
        .build()
        .context("failed to build rayon thread pool")?
        .install(|| {
            Square::iter()
                .collect::<Vec<_>>()
                .into_par_iter()
                .map(|white_king| {
                    let mut results = if constraints.double_king {
                        enumerate_for_white_king_double_king(white_king, constraints)
                    } else {
                        enumerate_for_white_king(white_king, &kind_pairs, constraints)
                    };
                    if constraints.miyako && white_king == Square::S55 {
                        if constraints.double_king {
                            results
                                .extend(enumerate_miyako_4piece_double_king(white_king, constraints));
                        } else {
                            results.extend(enumerate_miyako_4piece(white_king, constraints));
                        }
                    }
                    results
                })
                .reduce(FxHashSet::default, |mut acc, set| {
                    acc.extend(set);
                    acc
                })
        });
    let mut sfens = positions.into_iter().collect::<Vec<_>>();
    sfens.sort();
    sfens.dedup();

    // Write to cache for future runs.
    if let Some(cache_path) = enum_cache_path(constraints) {
        let _ = std::fs::write(&cache_path, sfens.join("\n"));
    }

    Ok(sfens)
}

pub(super) fn enumerate_final_2_positions(
    parallel: usize,
    constraints: SearchConstraints,
) -> anyhow::Result<Vec<PositionAux>> {
    let sfens = enumerate_final_2_sfens(parallel, constraints)?;
    if parallel > 1 {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(parallel)
            .build()
            .context("failed to build rayon thread pool")?;
        pool.install(|| {
            sfens
                .into_par_iter()
                .map(|sfen| PositionAux::from_sfen(&sfen))
                .collect()
        })
    } else {
        sfens
            .into_iter()
            .map(|sfen| PositionAux::from_sfen(&sfen))
            .collect()
    }
}

/// Build a candidate position with `white_king` and the listed `placements`,
/// fill in white-hand complement, and register the SFEN if it is a 1-branch
/// mate with at least one valid undo predecessor.
fn try_register_mate(
    white_king: Square,
    placements: &[(Color, Kind, Square)],
    constraints: SearchConstraints,
    movements: &mut Vec<Movement>,
    results: &mut FxHashSet<String>,
) {
    let mut position = PositionAux::default();
    position.set_turn(Color::WHITE);
    position.set(white_king, Color::WHITE, Kind::King);
    for &(color, kind, sq) in placements {
        position.set(sq, color, kind);
    }
    let mut position = with_white_complement(&position);
    if !position.checked_slow(Color::WHITE) {
        return;
    }
    if !satisfies_search_constraints(&position, constraints) {
        return;
    }
    movements.clear();
    let mate_options = AdvanceOptions {
        max_allowed_branches: Some(0),
        ..Default::default()
    };
    if matches!(
        advance_aux(&mut position, &mate_options, movements),
        Ok(true)
    ) && has_valid_predecessor(&mut position, constraints)
    {
        results.insert(canonical_sfen(&position, constraints));
    }
}

fn enumerate_for_white_king(
    white_king: Square,
    kind_pairs: &[(Kind, Kind)],
    constraints: SearchConstraints,
) -> FxHashSet<String> {
    let mut results = FxHashSet::default();
    let mut movements = Vec::new();

    if !square_in_bounds(white_king, constraints) {
        return results;
    }
    if constraints.mate_squares != 0 && (constraints.mate_squares >> white_king.index()) & 1 == 0 {
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
                try_register_mate(
                    white_king,
                    &[(Color::BLACK, kind1, sq1), (Color::BLACK, kind2, sq2)],
                    constraints,
                    &mut movements,
                    &mut results,
                );
            }
        }
    }
    results
}

/// Enumerate 3-piece mate positions for 双玉 mode: white king + black king +
/// one black non-king piece.
fn enumerate_for_white_king_double_king(
    white_king: Square,
    constraints: SearchConstraints,
) -> FxHashSet<String> {
    let mut results = FxHashSet::default();
    let mut movements = Vec::new();

    if !square_in_bounds(white_king, constraints) {
        return results;
    }
    if constraints.mate_squares != 0 && (constraints.mate_squares >> white_king.index()) & 1 == 0 {
        return results;
    }
    for black_king in Square::iter() {
        if black_king == white_king || !square_in_bounds(black_king, constraints) {
            continue;
        }
        let black_piece_kinds = KINDS
            .iter()
            .copied()
            .filter(|&k| k != Kind::King)
            .collect::<Vec<_>>();
        for kind in black_piece_kinds {
            if !kind_allowed_by_mask(kind, constraints.allowed_kinds_mask) {
                continue;
            }
            for sq in legal_black_piece_squares(kind) {
                if sq == white_king || sq == black_king || !square_in_bounds(sq, constraints) {
                    continue;
                }
                try_register_mate(
                    white_king,
                    &[
                        (Color::BLACK, Kind::King, black_king),
                        (Color::BLACK, kind, sq),
                    ],
                    constraints,
                    &mut movements,
                    &mut results,
                );
            }
        }
    }
    results
}

/// Enumerate 4-piece mate positions for miyako 双玉: white king on center +
/// black king + 2 additional pieces (any mix of black/white, excluding kings).
fn enumerate_miyako_4piece_double_king(
    white_king: Square,
    constraints: SearchConstraints,
) -> FxHashSet<String> {
    let mut results = FxHashSet::default();
    let mut movements = Vec::new();

    if !square_in_bounds(white_king, constraints) {
        return results;
    }

    // Pruning: at least one piece (black king or either non-king piece) must
    // give check to the white king.
    let pieces = miyako_piece_list(constraints);
    let n = pieces.len();
    let attacker_sqs: Vec<Vec<Square>> = pieces
        .iter()
        .map(|&(c, k)| {
            if c != Color::BLACK {
                return vec![];
            }
            piece_squares(c, k)
                .into_iter()
                .filter(|&sq| black_attacks_sq(k, sq, white_king))
                .collect()
        })
        .collect();

    for black_king in Square::iter() {
        if black_king == white_king || !square_in_bounds(black_king, constraints) {
            continue;
        }
        let king_atk = black_attacks_sq(Kind::King, black_king, white_king);
        for i in 0..n {
            let (c1, k1) = pieces[i];
            let sqs1 = piece_squares(c1, k1);
            for &sq1 in &sqs1 {
                if sq1 == white_king || sq1 == black_king || !square_in_bounds(sq1, constraints) {
                    continue;
                }
                let sq1_atk = c1 == Color::BLACK && black_attacks_sq(k1, sq1, white_king);
                let king_or_sq1_atk = king_atk || sq1_atk;
                for j in (i + 1)..n {
                    let (c2, k2) = pieces[j];
                    // If neither black king nor piece 1 gives check, piece 2 must be black.
                    if !king_or_sq1_atk && c2 != Color::BLACK {
                        continue;
                    }
                    let ps2_buf;
                    let sqs2: &[Square] = if king_or_sq1_atk {
                        ps2_buf = piece_squares(c2, k2);
                        &ps2_buf
                    } else {
                        &attacker_sqs[j]
                    };
                    for &sq2 in sqs2 {
                        if sq2 == white_king
                            || sq2 == black_king
                            || sq2 == sq1
                            || !square_in_bounds(sq2, constraints)
                        {
                            continue;
                        }
                        if pieces[i] == pieces[j] && sq2 <= sq1 {
                            continue;
                        }
                        if has_nifu(c1, k1, sq1, c2, k2, sq2) {
                            continue;
                        }
                        try_register_mate(
                            white_king,
                            &[
                                (Color::BLACK, Kind::King, black_king),
                                (c1, k1, sq1),
                                (c2, k2, sq2),
                            ],
                            constraints,
                            &mut movements,
                            &mut results,
                        );
                    }
                }
            }
        }
    }
    results
}

/// Enumerate 4-piece mate positions for miyako (都詰): white king on center
/// + 3 additional pieces (any mix of black/white, excluding king).
fn enumerate_miyako_4piece(
    white_king: Square,
    constraints: SearchConstraints,
) -> FxHashSet<String> {
    let mut results = FxHashSet::default();
    let mut movements = Vec::new();

    if !square_in_bounds(white_king, constraints) {
        return results;
    }

    // Pruning: at least one piece must give check to the white king (i.e. be a
    // black piece that attacks white_king on an empty board). White pieces can
    // never check their own king, so they are skipped when needed.
    let pieces = miyako_piece_list(constraints);
    let n = pieces.len();
    // Precompute per (kind, square): attacking squares for each black kind
    // (used to restrict sq3 iteration when neither piece 1 nor 2 gives check).
    let attacker_sqs: Vec<Vec<Square>> = pieces
        .iter()
        .map(|&(c, k)| {
            if c != Color::BLACK {
                return vec![];
            }
            piece_squares(c, k)
                .into_iter()
                .filter(|&sq| black_attacks_sq(k, sq, white_king))
                .collect()
        })
        .collect();

    for i in 0..n {
        let (c1, k1) = pieces[i];
        let sqs1 = piece_squares(c1, k1);
        for &sq1 in &sqs1 {
            if sq1 == white_king || !square_in_bounds(sq1, constraints) {
                continue;
            }
            let sq1_atk = c1 == Color::BLACK && black_attacks_sq(k1, sq1, white_king);
            for j in (i + 1)..n {
                let (c2, k2) = pieces[j];
                let sqs2 = piece_squares(c2, k2);
                for &sq2 in &sqs2 {
                    if sq2 == white_king || sq2 == sq1 || !square_in_bounds(sq2, constraints) {
                        continue;
                    }
                    if pieces[i] == pieces[j] && sq2 <= sq1 {
                        continue;
                    }
                    if c1 == c2
                        && (k1 == Kind::Pawn || k1 == Kind::ProPawn)
                        && (k2 == Kind::Pawn || k2 == Kind::ProPawn)
                        && sq1.col() == sq2.col()
                    {
                        continue;
                    }
                    let sq2_atk = c2 == Color::BLACK && black_attacks_sq(k2, sq2, white_king);
                    let first_two_atk = sq1_atk || sq2_atk;
                    for k in (j + 1)..n {
                        let (c3, k3) = pieces[k];
                        // If neither piece 1 nor 2 gives check, piece 3 must.
                        // White pieces can never give check → skip them.
                        if !first_two_atk && c3 != Color::BLACK {
                            continue;
                        }
                        let ps3_buf;
                        let sqs3: &[Square] = if first_two_atk {
                            ps3_buf = piece_squares(c3, k3);
                            &ps3_buf
                        } else {
                            &attacker_sqs[k]
                        };
                        for &sq3 in sqs3 {
                            if sq3 == white_king
                                || sq3 == sq1
                                || sq3 == sq2
                                || !square_in_bounds(sq3, constraints)
                            {
                                continue;
                            }
                            if pieces[j] == pieces[k] && sq3 <= sq2 {
                                continue;
                            }
                            if pieces[i] == pieces[k] && sq3 <= sq1 {
                                continue;
                            }
                            if has_nifu(c1, k1, sq1, c3, k3, sq3)
                                || has_nifu(c2, k2, sq2, c3, k3, sq3)
                            {
                                continue;
                            }
                            try_register_mate(
                                white_king,
                                &[(c1, k1, sq1), (c2, k2, sq2), (c3, k3, sq3)],
                                constraints,
                                &mut movements,
                                &mut results,
                            );
                        }
                    }
                }
            }
        }
    }
    results
}

/// Returns true if a black piece of the given kind at `from` can attack `target`
/// on an empty board. White pieces can never attack the white king, so they
/// always return false. Used as a cheap pre-filter before `try_register_mate`.
#[inline]
fn black_attacks_sq(kind: Kind, from: Square, target: Square) -> bool {
    power(Color::BLACK, from, kind).contains(target)
}

fn miyako_piece_list(constraints: SearchConstraints) -> Vec<(Color, Kind)> {
    let mut pieces = Vec::new();
    for &kind in &KINDS {
        if kind == Kind::King {
            continue;
        }
        if !kind_allowed_by_mask(kind, constraints.allowed_kinds_mask) {
            continue;
        }
        pieces.push((Color::BLACK, kind));
        if constraints.allow_white_pieces {
            pieces.push((Color::WHITE, kind));
        }
    }
    pieces
}

fn piece_squares(color: Color, kind: Kind) -> Vec<Square> {
    Square::iter()
        .filter(|&sq| match color {
            Color::BLACK => black_piece_can_stand_on(kind, sq),
            Color::WHITE => white_piece_can_stand_on(kind, sq),
        })
        .collect()
}

fn white_piece_can_stand_on(kind: Kind, sq: Square) -> bool {
    match kind {
        Kind::Pawn | Kind::Lance => sq.row() != 8,
        Kind::Knight => sq.row() <= 6,
        _ => true,
    }
}

fn has_nifu(c1: Color, k1: Kind, sq1: Square, c2: Color, k2: Kind, sq2: Square) -> bool {
    c1 == c2 && k1 == Kind::Pawn && k2 == Kind::Pawn && sq1.col() == sq2.col()
}

fn has_valid_predecessor(position: &mut PositionAux, constraints: SearchConstraints) -> bool {
    let mut undo_moves = Vec::new();
    previous(position, false, &mut undo_moves);
    undo_moves
        .iter()
        .any(|m| satisfies_ideal_smoke_undo_candidate(position, m, 1, constraints))
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

#[cfg(test)]
mod tests {
    use super::super::super::smoke_constraints::{
        parse_allowed_kinds, with_white_complement, SearchConstraints,
    };
    use super::{
        black_attacks_sq, enumerate_final_2_sfens, enumerate_miyako_4piece,
        enumerate_miyako_4piece_double_king, has_nifu, miyako_piece_list, piece_squares,
        square_in_bounds, try_register_mate,
    };
    use fmrs_core::{
        piece::{Color, Kind},
        position::{position::PositionAux, Square},
    };
    use rustc_hash::FxHashSet;

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

    // --- black_attacks_sq tests ---

    #[test]
    fn black_attacks_sq_rook() {
        // Rook on same column attacks 5五
        assert!(black_attacks_sq(Kind::Rook, Square::S51, Square::S55));
        // Rook on same row attacks 5五
        assert!(black_attacks_sq(Kind::Rook, Square::S15, Square::S55));
        // Rook on different row AND column does NOT attack 5五
        assert!(!black_attacks_sq(Kind::Rook, Square::S11, Square::S55));
    }

    #[test]
    fn black_attacks_sq_bishop() {
        // Bishop on diagonal (4四) attacks 5五
        assert!(black_attacks_sq(Kind::Bishop, Square::S44, Square::S55));
        // Bishop on diagonal (6六) attacks 5五
        assert!(black_attacks_sq(Kind::Bishop, Square::S66, Square::S55));
        // Bishop NOT on diagonal does NOT attack 5五
        assert!(!black_attacks_sq(Kind::Bishop, Square::S45, Square::S55));
    }

    #[test]
    fn black_attacks_sq_pawn() {
        // Black pawn at 5六 attacks 5五 (one step forward for black)
        assert!(black_attacks_sq(Kind::Pawn, Square::S56, Square::S55));
        // Black pawn at 5四 moves away from 5五, does NOT attack it
        assert!(!black_attacks_sq(Kind::Pawn, Square::S54, Square::S55));
        // Black pawn at 6六 is on a different column
        assert!(!black_attacks_sq(Kind::Pawn, Square::S66, Square::S55));
    }

    #[test]
    fn black_attacks_sq_knight() {
        // Black knight at 4七 attacks 5五 (±1 col, -2 rows: (3,6)→(4,4)=5五)
        assert!(black_attacks_sq(Kind::Knight, Square::S47, Square::S55));
        // Black knight at 6七 attacks 5五 (col 5+1=6, row 4+2=6: (5,6)→(4,4))
        assert!(black_attacks_sq(Kind::Knight, Square::S67, Square::S55));
        // Black knight at 5七 does NOT attack 5五
        assert!(!black_attacks_sq(Kind::Knight, Square::S57, Square::S55));
    }

    // --- Pruning correctness: compare optimized vs naive for miyako ---

    /// Naive (no pruning) reference implementation of enumerate_miyako_4piece.
    fn enumerate_miyako_4piece_naive(
        white_king: Square,
        constraints: SearchConstraints,
    ) -> FxHashSet<String> {
        let mut results = FxHashSet::default();
        let mut movements = Vec::new();
        if !square_in_bounds(white_king, constraints) {
            return results;
        }
        let pieces = miyako_piece_list(constraints);
        let n = pieces.len();
        for i in 0..n {
            let (c1, k1) = pieces[i];
            for &sq1 in &piece_squares(c1, k1) {
                if sq1 == white_king || !square_in_bounds(sq1, constraints) {
                    continue;
                }
                for j in (i + 1)..n {
                    let (c2, k2) = pieces[j];
                    for &sq2 in &piece_squares(c2, k2) {
                        if sq2 == white_king || sq2 == sq1 || !square_in_bounds(sq2, constraints) {
                            continue;
                        }
                        if pieces[i] == pieces[j] && sq2 <= sq1 {
                            continue;
                        }
                        if c1 == c2
                            && (k1 == Kind::Pawn || k1 == Kind::ProPawn)
                            && (k2 == Kind::Pawn || k2 == Kind::ProPawn)
                            && sq1.col() == sq2.col()
                        {
                            continue;
                        }
                        for k in (j + 1)..n {
                            let (c3, k3) = pieces[k];
                            for &sq3 in &piece_squares(c3, k3) {
                                if sq3 == white_king
                                    || sq3 == sq1
                                    || sq3 == sq2
                                    || !square_in_bounds(sq3, constraints)
                                {
                                    continue;
                                }
                                if pieces[j] == pieces[k] && sq3 <= sq2 {
                                    continue;
                                }
                                if pieces[i] == pieces[k] && sq3 <= sq1 {
                                    continue;
                                }
                                if has_nifu(c1, k1, sq1, c3, k3, sq3)
                                    || has_nifu(c2, k2, sq2, c3, k3, sq3)
                                {
                                    continue;
                                }
                                try_register_mate(
                                    white_king,
                                    &[(c1, k1, sq1), (c2, k2, sq2), (c3, k3, sq3)],
                                    constraints,
                                    &mut movements,
                                    &mut results,
                                );
                            }
                        }
                    }
                }
            }
        }
        results
    }

    /// Naive reference for enumerate_miyako_4piece_double_king.
    fn enumerate_miyako_4piece_double_king_naive(
        white_king: Square,
        constraints: SearchConstraints,
    ) -> FxHashSet<String> {
        let mut results = FxHashSet::default();
        let mut movements = Vec::new();
        if !square_in_bounds(white_king, constraints) {
            return results;
        }
        let pieces = miyako_piece_list(constraints);
        let n = pieces.len();
        for black_king in Square::iter() {
            if black_king == white_king || !square_in_bounds(black_king, constraints) {
                continue;
            }
            for i in 0..n {
                let (c1, k1) = pieces[i];
                for &sq1 in &piece_squares(c1, k1) {
                    if sq1 == white_king || sq1 == black_king || !square_in_bounds(sq1, constraints)
                    {
                        continue;
                    }
                    for j in (i + 1)..n {
                        let (c2, k2) = pieces[j];
                        for &sq2 in &piece_squares(c2, k2) {
                            if sq2 == white_king
                                || sq2 == black_king
                                || sq2 == sq1
                                || !square_in_bounds(sq2, constraints)
                            {
                                continue;
                            }
                            if pieces[i] == pieces[j] && sq2 <= sq1 {
                                continue;
                            }
                            if has_nifu(c1, k1, sq1, c2, k2, sq2) {
                                continue;
                            }
                            try_register_mate(
                                white_king,
                                &[
                                    (Color::BLACK, Kind::King, black_king),
                                    (c1, k1, sq1),
                                    (c2, k2, sq2),
                                ],
                                constraints,
                                &mut movements,
                                &mut results,
                            );
                        }
                    }
                }
            }
        }
        results
    }

    fn small_constraints() -> SearchConstraints {
        // Restrict piece kinds and board to keep iteration count small.
        // max_file=5 (cols 0-4) + max_rank=5 (rows 4-8) confines pieces to the
        // 5×5 bottom-left quadrant that contains S55, dramatically reducing
        // the search space while still covering both attacker and non-attacker
        // square combinations that exercise the pruning logic.
        let mask =
            parse_allowed_kinds(&["silver".to_string(), "rook".to_string()]).unwrap();
        SearchConstraints {
            allowed_kinds_mask: Some(mask),
            miyako: true,
            max_file: Some(5),
            max_rank: Some(5),
            ..Default::default()
        }
    }

    #[test]
    fn miyako_piece_list_white_pieces_gated_by_allow_white_pieces() {
        let without = miyako_piece_list(SearchConstraints {
            miyako: true,
            ..Default::default()
        });
        let with_white = miyako_piece_list(SearchConstraints {
            miyako: true,
            allow_white_pieces: true,
            ..Default::default()
        });
        assert!(without.iter().all(|&(c, _)| c == Color::BLACK));
        assert!(with_white.iter().any(|&(c, _)| c == Color::WHITE));
        assert!(with_white.len() > without.len());
    }

    #[test]
    fn miyako_pruning_matches_naive() {
        let constraints = small_constraints();
        let optimized = enumerate_miyako_4piece(Square::S55, constraints);
        let naive = enumerate_miyako_4piece_naive(Square::S55, constraints);
        assert_eq!(
            optimized, naive,
            "pruned enumerate_miyako_4piece must produce identical results to naive"
        );
    }

    #[test]
    fn miyako_double_king_pruning_matches_naive() {
        let constraints = SearchConstraints {
            miyako: true,
            double_king: true,
            ..small_constraints()
        };
        let optimized = enumerate_miyako_4piece_double_king(Square::S55, constraints);
        let naive = enumerate_miyako_4piece_double_king_naive(Square::S55, constraints);
        assert_eq!(
            optimized, naive,
            "pruned enumerate_miyako_4piece_double_king must produce identical results to naive"
        );
    }
}
