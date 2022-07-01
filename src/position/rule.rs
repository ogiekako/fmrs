use crate::piece::{Color, Kind};

use super::{
    bitboard::{self, BitBoard},
    Movement, Square,
};

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Color::Black => pos.row() < 3,
        Color::White => pos.row() >= 6,
    }
}

pub(super) fn movable_positions(
    turn_pieces: BitBoard,
    opponent_pieces: BitBoard,
    turn: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    let mask = bitboard::movable_positions(turn_pieces | opponent_pieces, pos, turn, kind);
    mask & !turn_pieces
}

pub(super) fn is_allowed_move(
    color: Color,
    source: Square,
    dest: Square,
    kind: Kind,
    promote: bool,
) -> bool {
    if promote {
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
        Color::Black => dest.row() >= d,
        Color::White => dest.row() < 9 - d,
    }
}
