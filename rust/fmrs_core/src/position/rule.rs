use crate::piece::{Color, Kind};

use super::{BitBoard, Square};

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Color::BLACK => pos.row() < 3,
        Color::WHITE => pos.row() >= 6,
    }
}

pub(super) fn is_allowed_move(
    color: Color,
    source: Square,
    dest: Square,
    kind: Kind,
    promote: bool,
) -> bool {
    if promote {
        debug_assert!(kind.promote().is_some());
        if !promotable(source, color) && !promotable(dest, color) {
            return false;
        }
        return true;
    }
    is_movable(color, dest, kind)
}

pub(super) fn is_movable(color: Color, dest: Square, kind: Kind) -> bool {
    let d = match kind {
        Kind::Pawn | Kind::Lance => 1,
        Kind::Knight => 2,
        _ => return true,
    };
    match color {
        Color::BLACK => dest.row() >= d,
        Color::WHITE => dest.row() < 9 - d,
    }
}

const ILLEGAL_KNIGHT_MASKS: [BitBoard; 2] = [
    BitBoard::from_u128(
        0b000000011000000011000000011000000011000000011000000011000000011000000011000000011,
    ),
    BitBoard::from_u128(
        0b110000000110000000110000000110000000110000000110000000110000000110000000110000000,
    ),
];

const ILLEGAL_PAWN_MASKS: [BitBoard; 2] = [
    BitBoard::from_u128(
        0b000000001000000001000000001000000001000000001000000001000000001000000001000000001,
    ),
    BitBoard::from_u128(
        0b100000000100000000100000000100000000100000000100000000100000000100000000100000000,
    ),
];

pub fn is_legal_drop(color: Color, pos: Square, kind: Kind, pawn_mask: usize) -> bool {
    match kind {
        Kind::Pawn => {
            !ILLEGAL_PAWN_MASKS[color.index()].get(pos) && pawn_mask >> pos.col() & 1 == 0
        }
        Kind::Lance => !ILLEGAL_PAWN_MASKS[color.index()].get(pos),
        Kind::Knight => !ILLEGAL_KNIGHT_MASKS[color.index()].get(pos),
        _ => true,
    }
}
