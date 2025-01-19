use crate::direction::Direction;

use super::square::Square;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct BitBoard(u128);

impl BitBoard {
    pub const EMPTY: BitBoard = BitBoard(0);

    pub const UPPER: BitBoard = BitBoard::ROW1;
    pub const LOWER: BitBoard = BitBoard::ROW9;

    pub const BLACK_PROMOTABLE: BitBoard =
        Self::from_u128(Self::ROW1.u128() | Self::ROW2.u128() | Self::ROW3.u128());
    pub const WHITE_PROMOTABLE: BitBoard =
        Self::from_u128(Self::ROW7.u128() | Self::ROW8.u128() | Self::ROW9.u128());

    pub const ROW1: BitBoard = BitBoard::from_u128(
        0b000000001000000001000000001000000001000000001000000001000000001000000001000000001u128,
    );
    pub const ROW2: BitBoard = BitBoard::from_u128(
        0b000000010000000010000000010000000010000000010000000010000000010000000010000000010u128,
    );
    pub const ROW3: BitBoard = BitBoard::from_u128(
        0b000000100000000100000000100000000100000000100000000100000000100000000100000000100u128,
    );
    pub const ROW4: BitBoard = BitBoard::from_u128(
        0b000001000000001000000001000000001000000001000000001000000001000000001000000001000u128,
    );
    pub const ROW5: BitBoard = BitBoard::from_u128(
        0b000010000000010000000010000000010000000010000000010000000010000000010000000010000u128,
    );
    pub const ROW6: BitBoard = BitBoard::from_u128(
        0b000100000000100000000100000000100000000100000000100000000100000000100000000100000u128,
    );
    pub const ROW7: BitBoard = BitBoard::from_u128(
        0b001000000001000000001000000001000000001000000001000000001000000001000000001000000u128,
    );
    pub const ROW8: BitBoard = BitBoard::from_u128(
        0b010000000010000000010000000010000000010000000010000000010000000010000000010000000u128,
    );
    pub const ROW9: BitBoard = BitBoard::from_u128(
        0b100000000100000000100000000100000000100000000100000000100000000100000000100000000u128,
    );
    pub const FULL: BitBoard = BitBoard::from_u128(
        0b111111111111111111111111111111111111111111111111111111111111111111111111111111111u128,
    );

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
    pub const fn set(&mut self, pos: Square) {
        let i = pos.index();
        self.0 |= 1 << i;
    }
    pub fn unset(&mut self, pos: Square) {
        let i = pos.index();
        self.0 &= !(1 << i);
    }
    pub fn contains(&self, pos: Square) -> bool {
        let i = pos.index();
        self.0 >> i & 1 != 0
    }
    pub fn and_not(&self, mask: BitBoard) -> BitBoard {
        BitBoard::from_u128(self.0 & !mask.0)
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

    pub(crate) fn digest(&self) -> u64 {
        (self.0 >> 64) as u64 ^ self.0 as u64
    }

    pub(crate) fn from_square(pos: Square) -> BitBoard {
        BitBoard::from_u128(1 << pos.index())
    }

    pub(crate) const fn const_default() -> BitBoard {
        BitBoard(0)
    }

    pub fn col_mask(&self) -> usize {
        let mut res = 0;
        for i in 0..9 {
            if !self.col_is_empty(i) {
                res |= 1 << i;
            }
        }
        res
    }

    fn col_is_empty(&self, col: usize) -> bool {
        (self.u128() >> (col * 9)) & 0b111111111 == 0
    }

    pub fn count_ones(&self) -> u32 {
        self.0.count_ones()
    }

    pub(crate) fn col_mask_bb(self) -> BitBoard {
        let mut res = 0;
        for pos in self {
            let i = pos.col();
            res |= 0x1FF << (i * 9);
        }
        BitBoard::from_u128(res)
    }

    pub fn and_not_assign(&mut self, other: BitBoard) {
        self.0 &= !other.0;
    }

    pub(crate) fn s99_to_highest(&self) -> BitBoard {
        if self.is_empty() {
            return BitBoard::FULL;
        }
        ((1 << 81) - (1 << self.0.ilog2())).into()
    }

    pub(crate) fn s11_to_lowest(&self) -> BitBoard {
        if self.is_empty() {
            return BitBoard::FULL;
        }
        ((2 << self.0.trailing_zeros()) - 1).into()
    }

    pub(crate) fn surrounding(&self, pos: Square) -> u128 {
        debug_assert!(!self.contains(pos));
        let p = 1u128 << pos.index();
        let high = self.0 ^ self.0.wrapping_sub(p);
        let mut lower = self.0 & (p - 1);
        if lower == 0 {
            lower = 1;
        };
        high - (1 << lower.ilog2())
    }
}

impl From<u128> for BitBoard {
    fn from(x: u128) -> Self {
        Self(x)
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
                    if self.contains(Square::new(col, row)) {
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
    pub const fn u128(&self) -> u128 {
        self.0
    }
    pub fn singleton(&self) -> Square {
        debug_assert!(self.0.count_ones() == 1);
        Square::from_index(self.0.trailing_zeros() as usize)
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
    pub const fn from_u128(x: u128) -> Self {
        debug_assert!(x < 1 << 81);
        Self(x)
    }
}

#[macro_export]
macro_rules! bitboard {
    ($($x:expr,)*) => {
        {
            let v = vec![$($x),*];
            if v.len() != 9 {
                panic!("Exactly 9 elements should be given.");
            }
            let mut res = $crate::position::BitBoard::default();
            for i in 0..9 {
                if v[i].len() != 9 {
                    panic!("v[{}] = {:?} should contain exactly 9 characters.", i, v[i]);
                }
                for (j, c) in v[i].chars().rev().enumerate() {
                    if c == '*' {
                        res.set($crate::position::Square::new(j, i));
                    }
                }
            }
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        direction::Direction,
        position::bitboard::{bitboard::BitBoard, square::Square},
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
                BitBoard(1 << 64),
                BitBoard(5),
                BitBoard(4),
                BitBoard(1),
                BitBoard(0 << 64),
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

        let orig = bb;

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
        let orig = bb;

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
