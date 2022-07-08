use super::square::Square;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BitBoard {
    x: u64,
    y: u32,
}

impl BitBoard {
    pub fn empty() -> Self {
        Self { x: 0, y: 0 }
    }
    pub(super) fn new(x: u64, y: u32) -> Self {
        Self {x, y}
    }
    pub fn is_empty(&self) -> bool {
        self.x == 0 && self.y == 0
    }
    pub fn set(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.x |= 1 << i;
        } else {
            self.y |= 1 << (i - 64);
        }
    }
    pub fn unset(&mut self, pos: Square) {
        let i = pos.index();
        if i < 64 {
            self.x &= !(1 << i);
        } else {
            self.y &= !(1 << (i - 64));
        }
    }
    pub fn get(&self, pos: Square) -> bool {
        let i = pos.index();
        if i < 64 {
            self.x >> i & 1 == 1
        } else {
            self.y >> (i - 64) & 1 == 1
        }
    }
    pub fn and_not(mut self, mask: BitBoard) -> BitBoard {
        self.x &= !mask.x;
        self.y &= !mask.y;
        self
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
                Self {
                    x: self.x.$op(rhs.x),
                    y: self.y.$op(rhs.y),
                }
            }
        }
    };
}

def_op!(BitAnd, bitand);
def_op!(BitOr, bitor);

macro_rules! def_op_assign {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            fn $op(&mut self, rhs: Self) {
                self.x.$op(rhs.x);
                self.y.$op(rhs.y);
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
        self.x as u128 | (self.y as u128) << 64
    }
    // Assumes self is not empty.
    fn pop(&mut self) -> Square {
        if self.x == 0 {
            let res = Square::from_index(self.y.trailing_zeros() as usize + 64);
            self.y = self.y & (self.y - 1);
            res
        } else {
            let res = Square::from_index(self.x.trailing_zeros() as usize);
            self.x = self.x & (self.x - 1);
            res
        }
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
        Self {
            x: x as u64,
            y: (x >> 64) as u32,
        }
    }
    pub(super) fn digest(&self) -> u64 {
        self.x.wrapping_mul(127).wrapping_add(self.y as u64)
    }
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard::{bitboard::BitBoard, square::Square};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_bitboard_next() {
        let x = Square::new(1, 2);
        let mut tmpl = BitBoard::empty();
        tmpl.set(x);
        let tmpl = tmpl;

        let mut b = tmpl;
        assert_eq!(Some(x), b.next());
        assert_eq!(None, b.next());
    }

    #[test]
    fn test_bitboard_subsets() {
        assert_eq!(
            BitBoard { x: 5, y: 1 }.subsets().collect::<Vec<BitBoard>>(),
            vec![
                BitBoard { x: 4, y: 1 },
                BitBoard { x: 1, y: 1 },
                BitBoard { x: 0, y: 1 },
                BitBoard { x: 5, y: 0 },
                BitBoard { x: 4, y: 0 },
                BitBoard { x: 1, y: 0 },
                BitBoard { x: 0, y: 0 },
                BitBoard { x: 5, y: 1 },
            ]
        );
    }
}
