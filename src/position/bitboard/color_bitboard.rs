use crate::piece::Color;

use super::{BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColorBitBoard {
    black_bitboard_x: u64,
    black_bitboard_y: u32,
    white_bitboard_y: u32,
    white_bitboard_x: u64,
}

impl ColorBitBoard {
    pub fn empty() -> Self {
        Self {
            black_bitboard_x: 0,
            black_bitboard_y: 0,
            white_bitboard_x: 0,
            white_bitboard_y: 0,
        }
    }
    pub fn get(&self, color: Color) -> BitBoard {
        match color {
            Color::Black => BitBoard::new(self.black_bitboard_x, self.black_bitboard_y),
            Color::White => BitBoard::new(self.white_bitboard_x, self.white_bitboard_y),
        }
    }
    pub fn set(&mut self, color: Color, pos: Square) {
        let i = pos.index();
        match color {
            Color::Black => {
                if i < 64 {
                    self.black_bitboard_x |= 1 << i;
                } else {
                    self.black_bitboard_y |= 1 << (i - 64);
                }
            }
            Color::White => {
                if i < 64 {
                    self.white_bitboard_x |= 1 << i;
                } else {
                    self.white_bitboard_y |= 1 << (i - 64);
                }
            }
        };
    }
    pub fn unset(&mut self, color: Color, pos: Square) {
        let i = pos.index();
        match color {
            Color::Black => {
                if i < 64 {
                    self.black_bitboard_x &= !(1 << i);
                } else {
                    self.black_bitboard_y &= !(1 << (i - 64));
                }
            }
            Color::White => {
                if i < 64 {
                    self.white_bitboard_x &= !(1 << i);
                } else {
                    self.white_bitboard_y &= !(1 << (i - 64));
                }
            }
        };
    }
    pub fn both(&self) -> BitBoard {
        BitBoard::new(
            self.black_bitboard_x | self.white_bitboard_x,
            self.black_bitboard_y | self.white_bitboard_y,
        )
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
