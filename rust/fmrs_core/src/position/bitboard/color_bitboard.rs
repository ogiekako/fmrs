use crate::{direction::Direction, piece::Color};

use super::{BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ColorBitBoard {
    black: BitBoard,
    white: BitBoard,
}

#[test]
fn test_color_bitboard_size() {
    assert_eq!(32, std::mem::size_of::<ColorBitBoard>());
}

impl ColorBitBoard {
    pub fn new(black: BitBoard, white: BitBoard) -> Self {
        Self { black, white }
    }
    pub fn bitboard(&self, color: Color) -> BitBoard {
        if color.is_black() {
            self.black
        } else {
            self.white
        }
    }
    pub fn set(&mut self, color: Color, pos: Square) {
        if color.is_black() {
            self.black.set(pos);
        } else {
            self.white.set(pos);
        }
    }
    pub fn unset(&mut self, color: Color, pos: Square) {
        if color.is_black() {
            self.black.unset(pos);
        } else {
            self.white.unset(pos);
        }
    }
    pub(crate) fn black(&self) -> BitBoard {
        self.black
    }

    pub(crate) fn white(&self) -> BitBoard {
        self.white
    }

    pub fn both(&self) -> BitBoard {
        self.black | self.white
    }

    pub(crate) fn shift(&mut self, dir: Direction) {
        self.black.shift(dir);
        self.white.shift(dir);
    }
}
