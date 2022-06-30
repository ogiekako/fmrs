use crate::piece::Color;

use super::Square;

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Color::Black => pos.row() < 3,
        Color::White => pos.row() >= 6,
    }
}
