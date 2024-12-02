use serde::Serialize;

use crate::direction::Direction;

use super::{BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct BitBoardPair {
    bitboard0_x: u64, // 0..64
    bitboard0_y: u32, // 64..81
    bitboard1_y: u32, // 64..81
    bitboard1_x: u64, // 0..64
}

impl BitBoardPair {
    pub fn empty() -> Self {
        Self {
            bitboard0_x: 0,
            bitboard0_y: 0,
            bitboard1_x: 0,
            bitboard1_y: 0,
        }
    }
    pub fn get0(&self) -> BitBoard {
        BitBoard::new(self.bitboard0_x, self.bitboard0_y)
    }
    pub fn get1(&self) -> BitBoard {
        BitBoard::new(self.bitboard1_x, self.bitboard1_y)
    }
    pub fn set0(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.bitboard0_x |= 1 << i;
        } else {
            self.bitboard0_y |= 1 << (i - 64);
        }
    }
    pub fn set1(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.bitboard1_x |= 1 << i;
        } else {
            self.bitboard1_y |= 1 << (i - 64);
        }
    }
    pub fn unset0(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.bitboard0_x &= !(1 << i);
        } else {
            self.bitboard0_y &= !(1 << (i - 64));
        }
    }
    pub fn unset1(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.bitboard1_x &= !(1 << i);
        } else {
            self.bitboard1_y &= !(1 << (i - 64));
        }
    }
    pub fn both(&self) -> BitBoard {
        BitBoard::new(
            self.bitboard0_x | self.bitboard1_x,
            self.bitboard0_y | self.bitboard1_y,
        )
    }

    pub(crate) fn shift(&mut self, dir: Direction) {
        let mut b0 = self.get0();
        let mut b1 = self.get1();

        b0.shift(dir);
        b1.shift(dir);

        *self = BitBoardPair {
            bitboard0_x: b0.x,
            bitboard0_y: b0.y,
            bitboard1_x: b1.x,
            bitboard1_y: b1.y,
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard::{bitboard_pair::BitBoardPair, testing::bitboard};

    #[test]
    fn size() {
        assert_eq!(24, std::mem::size_of::<BitBoardPair>());
    }

    #[test]
    fn internal() {
        let bbp = BitBoardPair {
            bitboard0_x: u64::MAX,
            bitboard0_y: 0,
            bitboard1_x: 0,
            bitboard1_y: 0,
        };
        assert_eq!(
            bbp.get0(),
            bitboard!(
                ".********",
                "..*******",
                "..*******",
                "..*******",
                "..*******",
                "..*******",
                "..*******",
                "..*******",
                "..*******",
            )
        );
    }
}
