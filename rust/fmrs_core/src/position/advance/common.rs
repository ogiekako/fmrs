use crate::{
    piece::{Color, Kind},
    position::{bitboard::ColorBitBoard, Position, Square},
};

use super::attack_prevent::attacker;

pub fn checked(
    position: &Position,
    color: Color,
    king_pos_hint: Option<Square>,
    color_bb_hint: Option<&ColorBitBoard>,
) -> bool {
    let king_pos = if let Some(king_pos) = king_pos_hint {
        king_pos
    } else if let Some(king_pos) = position.bitboard(color, Kind::King).next() {
        king_pos
    } else {
        return false;
    };
    let color_bb = if let Some(color_bb) = color_bb_hint {
        color_bb
    } else {
        &position.color_bb()
    };
    attacker(position, color_bb, color, king_pos, true).is_some()
}
