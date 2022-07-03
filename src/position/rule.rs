use crate::piece::{Color, Kind};

use super::{Square};

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Color::Black => pos.row() < 3,
        Color::White => pos.row() >= 6,
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
        Color::Black => dest.row() >= d,
        Color::White => dest.row() < 9 - d,
    }
}
