use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, rule::king_power},
        Position,
    },
};

pub fn checked(position: &Position, color: Color) -> bool {
    let king_pos = {
        if let Some(king_pos) = position.bitboard(color, Kind::King).next() {
            king_pos
        } else {
            return false;
        }
    };
    let opponent_pieces = position.color_bb().bitboard(color.opposite());
    let around_king = king_power(king_pos);
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
        let attackers = position.bitboard(color.opposite(), attacker_kind);
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
