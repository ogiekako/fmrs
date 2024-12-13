use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, rule::king_power},
        rule, Movement, Position,
    },
};

pub fn checked(position: &Position, color: Color) -> bool {
    let king_pos = {
        if let Some(king_pos) = position.bitboard(color.into(), Kind::KING.into()).next() {
            king_pos
        } else {
            return false;
        }
    };
    let opponent_pieces = position.bitboard(color.opposite().into(), None);
    let around_king = king_power(king_pos);
    // Non line or leap moves
    for attacker_pos in around_king & opponent_pieces {
        let arracker_kind = position.get(attacker_pos).unwrap().1;
        if arracker_kind == Kind::KNIGHT || arracker_kind.is_line_piece() {
            continue;
        }
        let attacker_power = bitboard::power(color.opposite(), attacker_pos, arracker_kind);
        if attacker_power.get(king_pos) {
            return true;
        }
    }
    for attacker_kind in [
        Kind::LANCE,
        Kind::KNIGHT,
        Kind::BISHOP,
        Kind::ROOK,
        Kind::PRO_BISHOP,
        Kind::PRO_ROOK,
    ] {
        let attackers = position.bitboard(color.opposite().into(), attacker_kind.into());
        if attackers.is_empty() {
            continue;
        }
        let attack_squares =
            bitboard::reachable(position.color_bb(), color, king_pos, attacker_kind, false);
        if !(attackers & attack_squares).is_empty() {
            return true;
        }
    }
    false
}

// Checks double pawn, unmovable pieces.
pub(super) fn maybe_legal_movement(
    turn: Color,
    movement: &Movement,
    kind: Kind,
    pawn_mask: usize,
) -> bool {
    match movement {
        Movement::Drop(pos, kind) => {
            if kind == &Kind::PAWN && pawn_mask >> pos.col() & 1 > 0 {
                return false;
            }
            rule::is_movable(turn, *pos, *kind)
        }
        Movement::Move {
            source,
            dest,
            promote,
        } => rule::is_allowed_move(turn, *source, *dest, kind, *promote),
    }
}
