use crate::{
    piece::{Color, Kind},
    position::{position::PositionAux, Square},
};

use super::attack_prevent::attacker;

pub fn checked(position: &mut PositionAux, color: Color, king_pos_hint: Option<Square>) -> bool {
    let king_pos = if let Some(king_pos) = king_pos_hint {
        king_pos
    } else if let Some(king_pos) = position.bitboard(color, Kind::King).next() {
        king_pos
    } else {
        return false;
    };
    attacker(position, color, king_pos, true).is_some()
}
