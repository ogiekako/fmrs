use anyhow::Context as _;
use fmrs_core::{
    piece::{Color, Kind, KINDS},
    position::{
        advance::{advance::advance_aux, AdvanceOptions},
        position::PositionAux,
        previous, Movement, Square,
    },
};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use super::super::smoke_constraints::{
    canonical_sfen, kind_allowed_by_mask, satisfies_ideal_smoke_undo_candidate,
    satisfies_search_constraints, square_in_bounds, with_white_complement, SearchConstraints,
};

pub(super) fn enumerate_final_2_sfens(
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
                .map(|white_king| {
                    let mut results =
                        enumerate_for_white_king(white_king, &kind_pairs, constraints);
                    if constraints.miyako && white_king == Square::S55 {
                        results.extend(enumerate_miyako_4piece(white_king, constraints));
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

    let pieces = miyako_piece_list(constraints);
    let n = pieces.len();
    for i in 0..n {
        let (c1, k1) = pieces[i];
        let sqs1 = piece_squares(c1, k1);
        for &sq1 in &sqs1 {
            if sq1 == white_king || !square_in_bounds(sq1, constraints) {
                continue;
            }
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
                    for k in (j + 1)..n {
                        let (c3, k3) = pieces[k];
                        let sqs3 = piece_squares(c3, k3);
                        for &sq3 in &sqs3 {
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
    use super::super::super::smoke_constraints::{with_white_complement, SearchConstraints};
    use super::enumerate_final_2_sfens;
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
