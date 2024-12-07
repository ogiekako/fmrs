use crate::direction::Direction;

use super::square::Square;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct BitBoard(u128);

impl BitBoard {
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
    pub fn set(&mut self, pos: Square) {
        let i = pos.index();
        self.0 |= 1 << i;
    }
    pub fn unset(&mut self, pos: Square) {
        let i = pos.index();
        self.0 &= !(1 << i);
    }
    pub fn get(&self, pos: Square) -> bool {
        let i = pos.index();
        self.0 >> i & 1 != 0
    }
    pub fn and_not(mut self, mask: BitBoard) -> BitBoard {
        self.0 &= !mask.0;
        self
    }

    pub(crate) fn shift(&mut self, dir: Direction) {
        match dir {
            Direction::Up => {
                let mask =
                    0b1000000001000000001000000001000000001000000001000000001000000001000000001u128;

                let upper = self.0 & mask;

                self.0 = (self.0 & !mask) >> 1 | upper << 8;
            }
            Direction::Down => {
                let mask = 0b100000000100000000100000000100000000100000000100000000100000000100000000100000000u128;

                let lower = self.0 & mask;

                self.0 = (self.0 & !mask) << 1 | lower >> 8;
            }
            Direction::Left => {
                let left = self.0 >> 72;
                self.0 = (self.0 << 9 | left) & ((1 << 81) - 1);
            }
            Direction::Right => {
                let right = self.0 & ((1 << 9) - 1);
                self.0 = self.0 >> 9 | right << 72;
            }
        }
    }
}

impl Iterator for BitBoard {
    type Item = Square;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        Some(self.pop())
    }
}

macro_rules! def_op {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            type Output = Self;

            fn $op(self, rhs: Self) -> Self {
                Self(self.0.$op(rhs.0))
            }
        }
    };
}

def_op!(BitAnd, bitand);
def_op!(BitOr, bitor);
def_op!(BitXor, bitxor);

macro_rules! def_op_assign {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            fn $op(&mut self, rhs: Self) {
                self.0.$op(rhs.0);
            }
        }
    };
}

def_op_assign!(BitAndAssign, bitand_assign);
def_op_assign!(BitOrAssign, bitor_assign);

impl std::fmt::Display for BitBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for row in 0..9 {
            for col in (0..9).rev() {
                write!(
                    f,
                    "{}",
                    if self.get(Square::new(col, row)) {
                        "*"
                    } else {
                        "."
                    }
                )?
            }
            writeln!(f)?
        }
        Ok(())
    }
}

impl std::fmt::Debug for BitBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "\n{}", self)
    }
}

impl BitBoard {
    pub(super) fn u128(&self) -> u128 {
        self.0
    }
    // Assumes self is not empty.
    fn pop(&mut self) -> Square {
        debug_assert!(!self.is_empty());
        let res = Square::from_index(self.0.trailing_zeros() as usize);
        self.0 &= self.0 - 1;
        res
    }
    pub(super) fn subsets(&self) -> impl Iterator<Item = BitBoard> {
        let orig = self.u128();
        let mut x = orig;
        (0..(1u128 << x.count_ones())).map(move |_| {
            x = orig & (x.wrapping_sub(1));
            BitBoard::from_u128(x)
        })
    }
    pub(super) fn from_u128(x: u128) -> Self {
        debug_assert!(x < 1 << 81);
        Self(x)
    }
    pub(super) fn digest(&self) -> u64 {
        (self.0 & 0xffff_ffff_ffff_ffff) as u64 + (self.0 >> 64) as u64
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        direction::Direction,
        position::bitboard::{bitboard::BitBoard, square::Square, testing::bitboard},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn test_bitboard_next() {
        let x = Square::new(1, 2);
        let mut tmpl = BitBoard::default();
        tmpl.set(x);
        let tmpl = tmpl;

        let mut b = tmpl;
        assert_eq!(Some(x), b.next());
        assert_eq!(None, b.next());
    }

    #[test]
    fn test_bitboard_subsets() {
        assert_eq!(
            BitBoard(5 | 1 << 64).subsets().collect::<Vec<BitBoard>>(),
            vec![
                BitBoard(4 | 1 << 64),
                BitBoard(1 | 1 << 64),
                BitBoard(0 | 1 << 64),
                BitBoard(5 | 0 << 64),
                BitBoard(4 | 0 << 64),
                BitBoard(1 | 0 << 64),
                BitBoard(0 | 0 << 64),
                BitBoard(5 | 1 << 64),
            ]
        );
    }

    #[test]
    fn shift_lr() {
        let mut bb = BitBoard::default();
        bb.set(Square::new(0, 0));
        bb.set(Square::new(7, 0));
        bb.set(Square::new(7, 1));
        bb.set(Square::new(8, 8));

        let orig = bb.clone();

        bb.shift(Direction::Left);
        assert_eq!(
            bb,
            bitboard!(
                "*......*.",
                "*........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                "........*",
            )
        );

        bb.shift(Direction::Right);
        assert_eq!(bb, orig);

        bb.shift(Direction::Right);

        assert_eq!(
            bb,
            bitboard!(
                "*.*......",
                "..*......",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".*.......",
            )
        );
    }

    #[test]
    fn shift_ud() {
        let mut bb = bitboard!(
            "**.*....*",
            ".*......*",
            ".*......*",
            "........*",
            "........*",
            ".........",
            "........*",
            "........*",
            "***.....*",
        );
        let orig = bb.clone();

        bb.shift(Direction::Up);
        assert_eq!(
            bb,
            bitboard!(
                ".*......*",
                ".*......*",
                "........*",
                "........*",
                ".........",
                "........*",
                "........*",
                "***.....*",
                "**.*....*",
            )
        );

        bb.shift(Direction::Down);
        assert_eq!(bb, orig);

        bb.shift(Direction::Down);

        assert_eq!(
            bb,
            bitboard!(
                "***.....*",
                "**.*....*",
                ".*......*",
                ".*......*",
                "........*",
                "........*",
                ".........",
                "........*",
                "........*",
            )
        );
    }
}
