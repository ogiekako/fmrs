use crate::piece::Color;

use super::BitBoard;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ColorBitBoard {
    black: BitBoard,
    white: BitBoard,
    occupied: BitBoard,
}

impl ColorBitBoard {
    pub fn new(black: BitBoard, white: BitBoard, occupied: BitBoard) -> Self {
        Self {
            black,
            white,
            occupied,
        }
    }
    pub fn bitboard(&self, color: Color) -> BitBoard {
        if color.is_black() {
            self.black
        } else {
            self.white
        }
    }

    pub(crate) fn black(&self) -> BitBoard {
        self.black
    }

    pub(crate) fn white(&self) -> BitBoard {
        self.white
    }

    pub fn both(&self) -> BitBoard {
        self.occupied
    }
}
