use crate::{
    piece::{Color, Kind},
    position::{
        bitboard,
        rule, Movement, Position,
    },
};

pub fn checked(position: &Position, color: Color) -> bool {
    let king_pos = {
        if let Some(king_pos) = position.bitboard(color.into(), Kind::King.into()).next() {
            king_pos
        } else {
            return false;
        }
    };
    let opponent_pieces = position.bitboard(color.opposite().into(), None);
    let turn_pieces = position.bitboard(color.into(), None);
    let around_king = bitboard::power(color, king_pos, Kind::King);
    // Non line or leap moves
    for attacker_pos in around_king & opponent_pieces {
        let arracker_kind = position.get(attacker_pos).unwrap().1;
        if arracker_kind == Kind::Knight || arracker_kind.is_line_piece() {
            continue;
        }
        let attacker_power = bitboard::power(color.opposite(), attacker_pos, arracker_kind);
        if attacker_power.get(king_pos) {
            return true;
        }
    }
    for attacker_kind in [
        Kind::Lance,
        Kind::Knight,
        Kind::Bishop,
        Kind::Rook,
        Kind::ProBishop,
        Kind::ProRook,
    ] {
        let attackers = position.bitboard(color.opposite().into(), attacker_kind.into());
        if attackers.is_empty() {
            continue;
        }
        let attack_squares = if color == Color::Black {
            bitboard::reachable(
                turn_pieces,
                opponent_pieces,
                Color::Black,
                king_pos,
                attacker_kind,
            )
        } else {
            bitboard::reachable(
                opponent_pieces,
                turn_pieces,
                Color::White,
                king_pos,
                attacker_kind,
            )
        };
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
            if kind == &Kind::Pawn && pawn_mask >> pos.col() & 1 > 0 {
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
