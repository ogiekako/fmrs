use crate::{direction::Direction, piece::Color};

use super::{BitBoard, Square};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ColorBitBoard {
    black: BitBoard,
    white: BitBoard,
    both: BitBoard,
}

impl ColorBitBoard {
    pub fn bitboard(&self, color: Color) -> &BitBoard {
        match color {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
    pub fn set(&mut self, color: Color, pos: Square) {
        self.both.set(pos);
        match color {
            Color::Black => {
                self.black.set(pos);
            }
            Color::White => {
                self.white.set(pos);
            }
        };
    }
    pub fn unset(&mut self, color: Color, pos: Square) {
        self.both.unset(pos);
        match color {
            Color::Black => {
                self.black.unset(pos);
            }
            Color::White => {
                self.white.unset(pos);
            }
        };
    }
    pub fn both(&self) -> &BitBoard {
        &self.both
    }

    pub(crate) fn shift(&mut self, dir: Direction) {
        self.black.shift(dir);
        self.white.shift(dir);
        self.both.shift(dir);
    }
}
