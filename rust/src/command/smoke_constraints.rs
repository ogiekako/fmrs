use fmrs_core::{
    piece::{Color, Kind, KINDS, NUM_HAND_KIND},
    position::{position::PositionAux, Square, UndoMove},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy)]
pub(super) struct KillerSeedLimits {
    pub(super) max_memo_entries: Option<usize>,
    pub(super) max_frontier: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SearchConstraints {
    pub(super) no_gold: bool,
    #[serde(default)]
    pub(super) no_pawn: bool,
    #[serde(default)]
    pub(super) only_pawn: bool,
    /// Bitmask of allowed piece kinds (bit i = Kind index i). None = all allowed.
    /// King is always implicitly allowed regardless of this mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) allowed_kinds_mask: Option<u16>,
    #[serde(default)]
    pub(super) natural_piece_limit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_file: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_rank: Option<u8>,
    #[serde(default)]
    pub(super) allow_white_pieces: bool,
    #[serde(default)]
    pub(super) slack: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_promoted_pct: Option<u16>,
    #[serde(default)]
    pub(super) max_promoted_pct_after_step: u16,
}

impl SearchConstraints {
    pub(super) fn breaks_lr_symmetry(self) -> bool {
        self.max_file.is_some()
    }
}

pub(super) fn expected_pieces_range(step: u16, slack: u16) -> (u32, u32) {
    let expected = step as u32 / 2 + 3;
    (expected.saturating_sub(slack as u32), expected)
}

pub(super) fn satisfies_ideal_smoke_constraints(
    position: &PositionAux,
    step: u16,
    constraints: SearchConstraints,
) -> bool {
    if step == 0 || step % 2 == 0 {
        return false;
    }
    if position.turn() != Color::BLACK {
        return false;
    }
    // Output must always have no black hand pieces.
    if !position.hands().is_empty(Color::BLACK) {
        return false;
    }
    let board = board_piece_count(position);
    let (min, max) = expected_pieces_range(step, constraints.slack);
    if board < min || board > max {
        return false;
    }
    if constraints.natural_piece_limit && !satisfies_natural_piece_limit(position) {
        return false;
    }
    satisfies_search_constraints(position, constraints)
}

pub(super) fn satisfies_ideal_smoke_generation_constraints(
    position: &PositionAux,
    step: u16,
    constraints: SearchConstraints,
) -> bool {
    if step == 0 {
        return satisfies_search_constraints(position, constraints);
    }
    if !constraints.allow_white_pieces && !position.hands().is_empty(Color::BLACK) {
        return false;
    }
    let pip = pieces_in_play(position);
    let (min, max) = expected_pieces_range(step, constraints.slack);
    if pip < min || pip > max {
        return false;
    }
    if !satisfies_promoted_pct(position, step, constraints) {
        return false;
    }
    if constraints.natural_piece_limit && !satisfies_natural_piece_limit(position) {
        return false;
    }
    satisfies_search_constraints(position, constraints)
}

pub(super) fn satisfies_ideal_smoke_undo_candidate(
    position: &PositionAux,
    undo_move: &UndoMove,
    next_step: u16,
    constraints: SearchConstraints,
) -> bool {
    if next_step == 0 {
        return true;
    }
    if !constraints.allow_white_pieces && undo_spawns_white_piece(position, undo_move) {
        return false;
    }
    if constraints.no_gold && undo_creates_gold(position, undo_move) {
        return false;
    }
    if constraints.no_pawn && undo_creates_pawn(position, undo_move) {
        return false;
    }
    if constraints.only_pawn && undo_creates_non_pawn(position, undo_move) {
        return false;
    }
    if constraints.allowed_kinds_mask.is_some()
        && undo_creates_forbidden_kind(position, undo_move, constraints.allowed_kinds_mask)
    {
        return false;
    }
    if undo_creates_out_of_bounds_piece(undo_move, constraints) {
        return false;
    }
    let pip = pieces_in_play_after_undo(position, undo_move);
    let (min, max) = expected_pieces_range(next_step, constraints.slack);
    if pip < min || pip > max {
        return false;
    }
    if !satisfies_promoted_pct(position, next_step, constraints) {
        return false;
    }
    constraints.allow_white_pieces || black_hand_empty_after_undo(position, undo_move)
}

pub(super) fn validate_search_constraints(constraints: SearchConstraints) -> anyhow::Result<()> {
    use anyhow::bail;
    if let Some(max_file) = constraints.max_file {
        if !(1..=9).contains(&max_file) {
            bail!("max-file must be between 1 and 9");
        }
    }
    if let Some(max_rank) = constraints.max_rank {
        if !(1..=9).contains(&max_rank) {
            bail!("max-rank must be between 1 and 9");
        }
    }
    if let Some(p) = constraints.max_promoted_pct {
        if p > 100 {
            bail!("max-promoted-pct must be between 0 and 100");
        }
    }
    Ok(())
}

pub(super) fn parse_allowed_kinds(names: &[String]) -> anyhow::Result<u16> {
    use anyhow::bail;
    let mut mask = 0u16;
    for name in names {
        let kind = match name.to_lowercase().as_str() {
            "pawn" | "p" => Kind::Pawn,
            "lance" | "l" => Kind::Lance,
            "knight" | "n" => Kind::Knight,
            "silver" | "s" => Kind::Silver,
            "gold" | "g" => Kind::Gold,
            "bishop" | "b" => Kind::Bishop,
            "rook" | "r" => Kind::Rook,
            other => bail!("unknown kind: {other}"),
        };
        mask |= 1u16 << kind.index();
        if let Some(promoted) = kind.promote() {
            mask |= 1u16 << promoted.index();
        }
    }
    Ok(mask)
}

pub(super) fn kind_allowed_by_mask(kind: Kind, mask: Option<u16>) -> bool {
    let Some(mask) = mask else { return true };
    kind == Kind::King || (mask >> kind.index()) & 1 == 1
}

pub(super) fn satisfies_search_constraints(
    position: &PositionAux,
    constraints: SearchConstraints,
) -> bool {
    if constraints.no_gold && board_gold_count(position) != 0 {
        return false;
    }
    if constraints.no_pawn && board_pawn_count(position) != 0 {
        return false;
    }
    if constraints.only_pawn && !board_only_pawn(position) {
        return false;
    }
    if let Some(mask) = constraints.allowed_kinds_mask {
        for square in Square::iter() {
            if let Some((_, kind)) = position.get(square) {
                if !kind_allowed_by_mask(kind, Some(mask)) {
                    return false;
                }
            }
        }
    }
    for square in Square::iter() {
        if position.get(square).is_some() && !square_in_bounds(square, constraints) {
            return false;
        }
    }
    true
}

pub(super) fn square_in_bounds(square: Square, constraints: SearchConstraints) -> bool {
    square_satisfies_file_constraint(square, constraints.max_file)
        && square_satisfies_rank_constraint(square, constraints.max_rank)
}

pub(super) fn square_satisfies_file_constraint(square: Square, max_file: Option<u8>) -> bool {
    max_file.is_none_or(|max_file| square.col() < max_file as usize)
}

pub(super) fn square_satisfies_rank_constraint(square: Square, max_rank: Option<u8>) -> bool {
    max_rank.is_none_or(|max_rank| square.row() >= 9 - max_rank as usize)
}

pub(super) fn board_gold_count(position: &PositionAux) -> u32 {
    position.bitboard(Color::BLACK, Kind::Gold).count_ones()
        + position.bitboard(Color::WHITE, Kind::Gold).count_ones()
}

pub(super) fn satisfies_promoted_pct(
    position: &PositionAux,
    step: u16,
    constraints: SearchConstraints,
) -> bool {
    let Some(max_pct) = constraints.max_promoted_pct else {
        return true;
    };
    if step < constraints.max_promoted_pct_after_step {
        return true;
    }
    let total = position.occupied_bb().count_ones();
    if total == 0 {
        return true;
    }
    let promoted = board_promoted_count(position);
    promoted * 100 <= max_pct as u32 * total
}

pub(super) fn satisfies_natural_piece_limit(position: &PositionAux) -> bool {
    let hands = position.hands();
    let count = |kind: Kind| -> u32 {
        position.bitboard(Color::BLACK, kind).count_ones()
            + position.bitboard(Color::WHITE, kind).count_ones()
            + if kind.is_hand_piece() {
                hands.count(Color::BLACK, kind) as u32
            } else {
                0
            }
    };
    let count_with_promoted = |base: Kind, promoted: Kind| -> u32 {
        count(base) + count(promoted)
    };
    count_with_promoted(Kind::Pawn, Kind::ProPawn) <= 9
        && count_with_promoted(Kind::Lance, Kind::ProLance) <= 2
        && count_with_promoted(Kind::Knight, Kind::ProKnight) <= 2
        && count_with_promoted(Kind::Silver, Kind::ProSilver) <= 2
        && count(Kind::Gold) <= 2
        && count_with_promoted(Kind::Bishop, Kind::ProBishop) <= 1
        && count_with_promoted(Kind::Rook, Kind::ProRook) <= 1
}

pub(super) fn board_only_pawn(position: &PositionAux) -> bool {
    const FORBIDDEN: [Kind; 10] = [
        Kind::Lance,
        Kind::Knight,
        Kind::Silver,
        Kind::Gold,
        Kind::Bishop,
        Kind::Rook,
        Kind::ProLance,
        Kind::ProKnight,
        Kind::ProSilver,
        Kind::ProBishop,
    ];
    for &kind in &FORBIDDEN {
        if position.bitboard(Color::BLACK, kind).count_ones() > 0
            || position.bitboard(Color::WHITE, kind).count_ones() > 0
        {
            return false;
        }
    }
    // ProRook also forbidden
    if position.bitboard(Color::BLACK, Kind::ProRook).count_ones() > 0
        || position.bitboard(Color::WHITE, Kind::ProRook).count_ones() > 0
    {
        return false;
    }
    true
}

pub(super) fn board_promoted_count(position: &PositionAux) -> u32 {
    const PROMOTED: [Kind; 6] = [
        Kind::ProPawn,
        Kind::ProLance,
        Kind::ProKnight,
        Kind::ProSilver,
        Kind::ProBishop,
        Kind::ProRook,
    ];
    PROMOTED
        .iter()
        .map(|&k| {
            position.bitboard(Color::BLACK, k).count_ones()
                + position.bitboard(Color::WHITE, k).count_ones()
        })
        .sum()
}

pub(super) fn board_pawn_count(position: &PositionAux) -> u32 {
    position.bitboard(Color::BLACK, Kind::Pawn).count_ones()
        + position.bitboard(Color::WHITE, Kind::Pawn).count_ones()
        + position.bitboard(Color::BLACK, Kind::ProPawn).count_ones()
        + position.bitboard(Color::WHITE, Kind::ProPawn).count_ones()
}

pub(super) fn undo_creates_gold(position: &PositionAux, undo_move: &UndoMove) -> bool {
    match undo_move {
        UndoMove::UnDrop(square, _) => position
            .get(*square)
            .is_some_and(|(_, kind)| kind == Kind::Gold),
        UndoMove::UnMove {
            dest,
            promote,
            capture,
            ..
        } => {
            capture.is_some_and(|kind| kind == Kind::Gold)
                || position.get(*dest).is_some_and(|(_, kind)| {
                    let previous_kind = if *promote {
                        kind.unpromote().unwrap()
                    } else {
                        kind
                    };
                    previous_kind == Kind::Gold
                })
        }
    }
}

pub(super) fn undo_creates_forbidden_kind(
    position: &PositionAux,
    undo_move: &UndoMove,
    mask: Option<u16>,
) -> bool {
    match undo_move {
        UndoMove::UnDrop(square, _) => position
            .get(*square)
            .is_some_and(|(_, kind)| !kind_allowed_by_mask(kind, mask)),
        UndoMove::UnMove {
            dest,
            promote,
            capture,
            ..
        } => {
            if capture.is_some_and(|kind| !kind_allowed_by_mask(kind, mask)) {
                return true;
            }
            position.get(*dest).is_some_and(|(_, kind)| {
                let previous_kind = if *promote {
                    kind.unpromote().unwrap()
                } else {
                    kind
                };
                !kind_allowed_by_mask(previous_kind, mask)
            })
        }
    }
}

pub(super) fn undo_creates_non_pawn(position: &PositionAux, undo_move: &UndoMove) -> bool {
    let is_pawn_kind = |k: Kind| k == Kind::Pawn || k == Kind::ProPawn;
    match undo_move {
        UndoMove::UnDrop(square, _) => position
            .get(*square)
            .is_some_and(|(_, kind)| !is_pawn_kind(kind) && kind != Kind::King),
        UndoMove::UnMove {
            dest,
            promote,
            capture,
            ..
        } => {
            capture.is_some_and(|kind| !is_pawn_kind(kind) && kind != Kind::King)
                || position.get(*dest).is_some_and(|(_, kind)| {
                    let previous_kind = if *promote {
                        kind.unpromote().unwrap()
                    } else {
                        kind
                    };
                    !is_pawn_kind(previous_kind) && previous_kind != Kind::King
                })
        }
    }
}

pub(super) fn undo_creates_pawn(position: &PositionAux, undo_move: &UndoMove) -> bool {
    match undo_move {
        UndoMove::UnDrop(square, _) => position
            .get(*square)
            .is_some_and(|(_, kind)| kind == Kind::Pawn || kind == Kind::ProPawn),
        UndoMove::UnMove {
            dest,
            promote,
            capture,
            ..
        } => {
            capture.is_some_and(|kind| kind == Kind::Pawn || kind == Kind::ProPawn)
                || position.get(*dest).is_some_and(|(_, kind)| {
                    let previous_kind = if *promote {
                        kind.unpromote().unwrap()
                    } else {
                        kind
                    };
                    previous_kind == Kind::Pawn || previous_kind == Kind::ProPawn
                })
        }
    }
}

pub(super) fn undo_creates_out_of_bounds_piece(
    undo_move: &UndoMove,
    constraints: SearchConstraints,
) -> bool {
    match undo_move {
        UndoMove::UnDrop(_, _) => false,
        UndoMove::UnMove { source, .. } => !square_in_bounds(*source, constraints),
    }
}

pub(super) fn undo_spawns_white_piece(position: &PositionAux, undo_move: &UndoMove) -> bool {
    matches!(
        undo_move,
        UndoMove::UnMove {
            capture: Some(_),
            ..
        } if position.turn() == Color::WHITE
    )
}

pub(super) fn board_piece_count(position: &PositionAux) -> u32 {
    position.occupied_bb().count_ones()
}

pub(super) fn black_hand_count(position: &PositionAux) -> u32 {
    KINDS[..NUM_HAND_KIND]
        .iter()
        .map(|&kind| position.hands().count(Color::BLACK, kind) as u32)
        .sum()
}

pub(super) fn pieces_in_play(position: &PositionAux) -> u32 {
    board_piece_count(position) + black_hand_count(position)
}

pub(super) fn pieces_in_play_after_undo(position: &PositionAux, undo_move: &UndoMove) -> u32 {
    let board = board_piece_count_after_undo(position, undo_move);
    let prev_turn = position.turn().opposite();
    let hand = if prev_turn == Color::BLACK {
        let current = black_hand_count(position);
        match undo_move {
            UndoMove::UnDrop(_, _) => current + 1,
            UndoMove::UnMove {
                capture: Some(_), ..
            } => current - 1,
            UndoMove::UnMove { capture: None, .. } => current,
        }
    } else {
        black_hand_count(position)
    };
    board + hand
}

pub(super) fn board_piece_count_after_undo(position: &PositionAux, undo_move: &UndoMove) -> u32 {
    let count = board_piece_count(position);
    match undo_move {
        UndoMove::UnDrop(_, _) => count - 1,
        UndoMove::UnMove {
            capture: Some(_), ..
        } => count + 1,
        UndoMove::UnMove { capture: None, .. } => count,
    }
}

pub(super) fn black_hand_empty_after_undo(position: &PositionAux, undo_move: &UndoMove) -> bool {
    let prev_turn = position.turn().opposite();
    match undo_move {
        UndoMove::UnDrop(_, _) => {
            prev_turn != Color::BLACK && position.hands().is_empty(Color::BLACK)
        }
        UndoMove::UnMove {
            capture: Some(capture),
            ..
        } if prev_turn == Color::BLACK => {
            black_hand_is_exactly(position, capture.maybe_unpromote())
        }
        UndoMove::UnMove { .. } => position.hands().is_empty(Color::BLACK),
    }
}

pub(super) fn black_hand_is_exactly(position: &PositionAux, expected: Kind) -> bool {
    for &kind in &KINDS[..NUM_HAND_KIND] {
        let count = position.hands().count(Color::BLACK, kind);
        if kind == expected {
            if count != 1 {
                return false;
            }
        } else if count != 0 {
            return false;
        }
    }
    true
}

pub(super) fn canonical_lr_sfen(position: &PositionAux) -> String {
    let sfen = position.sfen();
    let reflected = reflect_left_right(position).sfen();
    if sfen <= reflected {
        sfen
    } else {
        reflected
    }
}

pub(super) fn canonical_sfen(position: &PositionAux, constraints: SearchConstraints) -> String {
    if constraints.breaks_lr_symmetry() {
        position.sfen()
    } else {
        canonical_lr_sfen(position)
    }
}

pub(super) fn reflect_left_right(position: &PositionAux) -> PositionAux {
    use fmrs_core::piece::KINDS;
    let mut reflected = PositionAux::default();
    reflected.set_turn(position.turn());
    reflected.set_pawn_drop(position.pawn_drop());
    for color in Color::iter() {
        for kind in KINDS[..NUM_HAND_KIND].iter().copied() {
            reflected
                .hands_mut()
                .add_n(color, kind, position.hands().count(color, kind));
        }
    }
    for sq in Square::iter() {
        if let Some((color, kind)) = position.get(sq) {
            reflected.set(Square::new(8 - sq.col(), sq.row()), color, kind);
        }
    }
    reflected
}

pub(super) fn canonical_lr_position(position: &PositionAux) -> PositionAux {
    let reflected = reflect_left_right(position);
    if position.sfen() <= reflected.sfen() {
        position.clone()
    } else {
        reflected
    }
}

pub(super) fn canonical_position(
    position: &PositionAux,
    constraints: SearchConstraints,
) -> PositionAux {
    if constraints.breaks_lr_symmetry() {
        position.clone()
    } else {
        canonical_lr_position(position)
    }
}

pub(super) fn count_kind_on_board(position: &PositionAux, kind: Kind) -> u32 {
    let mut count = position.bitboard(Color::BLACK, kind).count_ones()
        + position.bitboard(Color::WHITE, kind).count_ones();
    if let Some(promoted) = kind.promote() {
        count += position.bitboard(Color::BLACK, promoted).count_ones()
            + position.bitboard(Color::WHITE, promoted).count_ones();
    }
    count
}

pub(super) fn with_white_complement(position: &PositionAux) -> PositionAux {
    let mut position = position.clone();
    for kind in KINDS[..NUM_HAND_KIND].iter().copied() {
        let board_used = count_kind_on_board(&position, kind);
        let black_hands = position.hands().count(Color::BLACK, kind) as u32;
        let white_hands = position.hands().count(Color::WHITE, kind) as u32;
        let total_used = board_used + black_hands + white_hands;
        let missing = kind
            .max_count()
            .checked_sub(total_used)
            .expect("piece count should not exceed max");
        position
            .hands_mut()
            .add_n(Color::WHITE, kind, missing as usize);
    }
    position
}

#[cfg(test)]
pub(super) fn white_hands_are_complement(position: &PositionAux) -> bool {
    KINDS[..NUM_HAND_KIND].iter().copied().all(|kind| {
        let board_used = count_kind_on_board(position, kind);
        let black_hands = position.hands().count(Color::BLACK, kind) as u32;
        let white_hands = position.hands().count(Color::WHITE, kind) as u32;
        board_used + black_hands + white_hands == kind.max_count()
            && white_hands == kind.max_count() - board_used - black_hands
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fmrs_core::{
        piece::{Color, Kind},
        position::{position::PositionAux, previous, Square, UndoMove},
    };

    #[test]
    fn reflect_left_right_is_involution() {
        let mut position = PositionAux::default();
        position.set_turn(Color::WHITE);
        position.set(Square::S19, Color::WHITE, Kind::King);
        position.set(Square::S38, Color::BLACK, Kind::ProRook);
        position.set(Square::S72, Color::BLACK, Kind::Silver);

        assert_eq!(
            reflect_left_right(&reflect_left_right(&position)).sfen(),
            position.sfen()
        );
    }

    #[test]
    fn with_white_complement_fills_remaining_pieces_to_white_hand() {
        let position = PositionAux::from_sfen("+R1k6/4R4/9/9/9/9/9/9/9 w - 1").unwrap();
        let position = with_white_complement(&position);
        assert!(position.hands().is_empty(Color::BLACK));
        assert!(white_hands_are_complement(&position));
        assert_eq!(count_kind_on_board(&position, Kind::Rook), 2);
        assert_eq!(position.hands().count(Color::WHITE, Kind::Rook), 0);
        assert_eq!(position.hands().count(Color::WHITE, Kind::Pawn), 18);
    }

    #[test]
    fn smoke_constraint_rejects_even_step() {
        let position = PositionAux::from_sfen("+R1k6/4R4/9/9/9/9/9/9/9 b - 1").unwrap();
        assert_eq!(board_piece_count(&position), 3);
        assert!(!satisfies_ideal_smoke_constraints(
            &position,
            2,
            SearchConstraints::default()
        ));
    }

    #[test]
    fn smoke_undo_prefilter_matches_full_generation_constraint() {
        let mut position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let mut undo_moves = vec![];
        previous(&mut position, false, &mut undo_moves);

        for undo_move in undo_moves {
            let mut previous_position = position.clone();
            previous_position.undo_move(&undo_move);
            assert_eq!(
                satisfies_ideal_smoke_undo_candidate(
                    &position,
                    &undo_move,
                    1,
                    SearchConstraints::default()
                ),
                satisfies_ideal_smoke_generation_constraints(
                    &previous_position,
                    1,
                    SearchConstraints::default()
                ),
                "{undo_move:?}"
            );
        }
    }

    #[test]
    fn smoke_undo_prefilter_rejects_white_piece_spawn() {
        let position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let undo_move = UndoMove::UnMove {
            source: Square::S11,
            dest: Square::S19,
            promote: false,
            capture: Some(Kind::Pawn),
            pawn_drop: false,
        };
        assert!(undo_spawns_white_piece(&position, &undo_move));
        assert!(!satisfies_ideal_smoke_undo_candidate(
            &position,
            &undo_move,
            3,
            SearchConstraints::default()
        ));
    }

    #[test]
    fn no_gold_rejects_gold_but_allows_promoted_goldish() {
        let constraints = SearchConstraints {
            no_gold: true,
            ..Default::default()
        };
        let gold = PositionAux::from_sfen("9/9/9/9/9/9/9/9/G6k1 b - 1").unwrap();
        let pro_pawn = PositionAux::from_sfen("9/9/9/9/9/9/9/9/+P6k1 b - 1").unwrap();

        assert!(!satisfies_search_constraints(&gold, constraints));
        assert!(satisfies_search_constraints(&pro_pawn, constraints));
    }

    #[test]
    fn no_gold_undo_prefilter_rejects_gold_creation() {
        let constraints = SearchConstraints {
            no_gold: true,
            ..Default::default()
        };
        let position =
            PositionAux::from_sfen("+B8/9/9/9/9/9/9/7+B1/7k1 w 2r4g4s4n4l18p 1").unwrap();
        let undo_move = UndoMove::UnMove {
            source: Square::S11,
            dest: Square::S19,
            promote: false,
            capture: Some(Kind::Gold),
            pawn_drop: false,
        };

        assert!(undo_creates_gold(&position, &undo_move));
        assert!(!satisfies_ideal_smoke_undo_candidate(
            &position,
            &undo_move,
            3,
            constraints
        ));
    }

    #[test]
    fn max_file_constraint_restricts_board_squares() {
        let constraints = SearchConstraints {
            max_file: Some(4),
            ..Default::default()
        };
        let mut inside = PositionAux::default();
        inside.set(Square::S11, Color::BLACK, Kind::Bishop);
        inside.set(Square::S41, Color::BLACK, Kind::Bishop);
        inside.set(Square::S19, Color::WHITE, Kind::King);
        let mut outside = inside.clone();
        outside.set(Square::S51, Color::BLACK, Kind::Bishop);

        assert!(satisfies_search_constraints(&inside, constraints));
        assert!(!satisfies_search_constraints(&outside, constraints));
    }

    #[test]
    fn max_rank_constraint_restricts_board_squares() {
        // max_rank=7 keeps ranks 3-9 (rows 2-8). S11 is rank 1 (row 0) -> outside.
        let constraints = SearchConstraints {
            max_rank: Some(7),
            ..Default::default()
        };
        let mut inside = PositionAux::default();
        inside.set(Square::S13, Color::BLACK, Kind::Bishop);
        inside.set(Square::S19, Color::WHITE, Kind::King);
        let mut outside = inside.clone();
        outside.set(Square::S11, Color::BLACK, Kind::Bishop);

        assert!(satisfies_search_constraints(&inside, constraints));
        assert!(!satisfies_search_constraints(&outside, constraints));
    }

    #[test]
    fn seed_log_constraints_treat_missing_and_null_max_file_as_none() {
        let missing = serde_json::from_str::<SearchConstraints>(r#"{"no_gold":true}"#).unwrap();
        let null = serde_json::from_str::<SearchConstraints>(
            r#"{"no_gold":true,"max_file":null}"#,
        )
        .unwrap();
        let explicit = SearchConstraints {
            no_gold: true,
            ..Default::default()
        };

        assert_eq!(missing, explicit);
        assert_eq!(null, explicit);
        let value = serde_json::to_value(explicit).unwrap();
        assert_eq!(value["no_gold"], true);
        assert_eq!(value["no_pawn"], false);
        assert_eq!(value["allow_white_pieces"], false);
        assert!(value.get("max_file").is_none());
    }
}
