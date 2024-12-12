use crate::{direction::Direction, piece::Color};

use super::{BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ColorBitBoard(BitBoard, BitBoard);

#[test]
fn test_color_bitboard_size() {
    assert_eq!(32, std::mem::size_of::<ColorBitBoard>());
}

impl ColorBitBoard {
    pub fn empty() -> Self {
        Self(BitBoard::empty(), BitBoard::empty())
    }
    pub fn bitboard(&self, color: Color) -> BitBoard {
        match color {
            Color::BLACK => self.0,
            Color::WHITE => self.1,
        }
    }
    pub fn set(&mut self, color: Color, pos: Square) {
        match color {
            Color::BLACK => {
                self.0.set(pos);
            }
            Color::WHITE => {
                self.1.set(pos);
            }
        };
    }
    pub fn unset(&mut self, color: Color, pos: Square) {
        match color {
            Color::BLACK => {
                self.0.unset(pos);
            }
            Color::WHITE => {
                self.1.unset(pos);
            }
        };
    }
    pub(crate) fn black(&self) -> BitBoard {
        self.0
    }

    pub(crate) fn white(&self) -> BitBoard {
        self.1
    }

    pub fn both(&self) -> BitBoard {
        self.0 | self.1
    }

    pub(crate) fn shift(&mut self, dir: Direction) {
        self.0.shift(dir);
        self.1.shift(dir);
    }
}
