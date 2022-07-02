use super::square::Square;

const MASK: u128 = 0b11111111100_11111111100_11111111100_11111111100_11111111100_11111111100_11111111100_11111111100_11111111100_00000000000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BitBoard {
    pub(super) x: u128,
}

impl BitBoard {
    pub fn new() -> BitBoard {
        BitBoard { x: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.x == 0
    }
    pub fn set(&mut self, pos: Square) {
        self.x |= 1 << pos.index();
    }
    pub fn unset(&mut self, pos: Square) {
        self.x &= !(1 << pos.index());
    }
    pub fn get(&self, pos: Square) -> bool {
        (self.x >> pos.index() & 1) == 1
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
                BitBoard {
                    x: (self.x.$op(rhs.x)) & MASK,
                }
            }
        }
    };
}

def_op!(Mul, mul);
def_op!(Shr, shr);
def_op!(BitAnd, bitand);
def_op!(BitOr, bitor);

macro_rules! def_op_assign {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            fn $op(&mut self, rhs: Self) {
                self.x.$op(rhs.x);
                self.x &= MASK;
            }
        }
    };
}

def_op_assign!(MulAssign, mul_assign);
def_op_assign!(ShrAssign, shr_assign);
def_op_assign!(BitAndAssign, bitand_assign);
def_op_assign!(BitOrAssign, bitor_assign);

impl std::ops::Not for BitBoard {
    type Output = Self;

    fn not(self) -> BitBoard {
        BitBoard {
            x: self.x.not() & MASK,
        }
    }
}

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
    // Assumes self is not empty.
    fn pop(&mut self) -> Square {
        let res = Square::from_index(self.x.trailing_zeros() as usize);
        self.x = self.x & (self.x - 1);
        res
    }
    pub(super) fn subsets(&self) -> impl Iterator<Item = BitBoard> {
        let orig = self.x;
        let mut x = self.x;
        (0..(1u128 << self.x.count_ones())).map(move |_| {
            x = orig & (x.wrapping_sub(1));
            BitBoard { x }
        })
    }
    pub(super) fn from_u128(x: u128) -> Self {
        Self { x: x & MASK }
    }
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard11::{bitboard::BitBoard, square::Square};

    #[test]
    fn test_bitboard_next() {
        let x = Square::new(1, 2);
        let mut tmpl = BitBoard::new();
        tmpl.set(x);
        let tmpl = tmpl;

        let mut b = tmpl;
        assert_eq!(Some(x), b.next());
        assert_eq!(None, b.next());
    }

    #[test]
    fn test_bitboard_subsets() {
        assert_eq!(
            BitBoard { x: 5 }.subsets().collect::<Vec<BitBoard>>(),
            vec![
                BitBoard { x: 4 },
                BitBoard { x: 1 },
                BitBoard { x: 0 },
                BitBoard { x: 5 },
            ]
        );
    }
}
