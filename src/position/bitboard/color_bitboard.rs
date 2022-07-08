use crate::piece::Color;

use super::{bitboard_pair::BitBoardPair, BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColorBitBoard(BitBoardPair);

impl ColorBitBoard {
    pub fn empty() -> Self {
        Self(BitBoardPair::empty())
    }
    pub fn bitboard(&self, color: Color) -> BitBoard {
        match color {
            Color::Black => self.0.get0(),
            Color::White => self.0.get1(),
        }
    }
    pub fn set(&mut self, color: Color, pos: Square) {
        match color {
            Color::Black => {
                self.0.set0(pos);
            }
            Color::White => {
                self.0.set1(pos);
            }
        };
    }
    pub fn unset(&mut self, color: Color, pos: Square) {
        match color {
            Color::Black => {
                self.0.unset0(pos);
            }
            Color::White => {
                self.0.unset1(pos);
            }
        };
    }
    pub fn both(&self) -> BitBoard {
        self.0.both()
    }
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard::ColorBitBoard;

    #[test]
    fn size() {
        assert_eq!(24, std::mem::size_of::<ColorBitBoard>());
    }
}
