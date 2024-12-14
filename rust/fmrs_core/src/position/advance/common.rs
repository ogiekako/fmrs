use crate::{
    piece::{Color, Kind},
    position::{Position, Square},
};

use super::attack_prevent::attacker;

pub fn checked(position: &Position, color: Color, king_pos_hint: Option<Square>) -> bool {
    let king_pos = if let Some(king_pos) = king_pos_hint {
        king_pos
    } else if let Some(king_pos) = position.bitboard(color, Kind::King).next() {
        king_pos
    } else {
        return false;
    };
    attacker(position, color, king_pos).is_some()
}
