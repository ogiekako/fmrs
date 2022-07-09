use super::{BitBoard, Square};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BitBoardPair {
    bitboard0_x: u64,
    bitboard0_y: u32,
    bitboard1_y: u32,
    bitboard1_x: u64,
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
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard::bitboard_pair::BitBoardPair;

    #[test]
    fn size() {
        assert_eq!(24, std::mem::size_of::<BitBoardPair>());
    }
}
