use crate::piece::{Color, Kind};

use super::{BitBoard, Square};

const PROMOTABLE_MASKS: [BitBoard; 2] = [
    BitBoard::from_u128(
        0b000000111000000111000000111000000111000000111000000111000000111000000111000000111,
    ),
    BitBoard::from_u128(
        0b111000000111000000111000000111000000111000000111000000111000000111000000111000000,
    ),
];

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    if c.is_black() {
        PROMOTABLE_MASKS[0].get(pos)
    } else {
        PROMOTABLE_MASKS[1].get(pos)
    }
}

pub(super) fn is_legal_move(
    color: Color,
    source: Square,
    dest: Square,
    kind: Kind,
    promote: bool,
) -> bool {
    if promote {
        debug_assert!(kind.promote().is_some());
        return promotable(source, color) || promotable(dest, color);
    }
    is_movable(color, dest, kind)
}

pub(super) fn is_movable(color: Color, dest: Square, kind: Kind) -> bool {
    match kind {
        Kind::Pawn | Kind::Lance => !ILLEGAL_PAWN_MASKS[color.index()].get(dest),
        Kind::Knight => !ILLEGAL_KNIGHT_MASKS[color.index()].get(dest),
        _ => true,
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
    if kind == Kind::Pawn && (pawn_mask & 1 << pos.col()) != 0 {
        return false;
    }
    is_movable(color, pos, kind)
}
