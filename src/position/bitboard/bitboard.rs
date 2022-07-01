use serde::{Deserialize, Serialize};

/*
9 8 7 6 5 4 3 2 1
              9 0 一
                1 二
    ...       ...
                7 八
80              8 九
*/
#[derive(Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
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

macro_rules! def_op {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            type Output = Self;

            fn $op(self, rhs: Self) -> Self {
                BitBoard {
                    x: self.x.$op(rhs.x),
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
        BitBoard { x: self.x.not() }
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

// Private methods
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
}

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

use std::fmt;

use super::Square;
impl fmt::Display for BitBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl fmt::Debug for BitBoard {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "\n{}", self)
    }
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
