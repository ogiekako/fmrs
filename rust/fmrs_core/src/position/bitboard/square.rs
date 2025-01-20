use rand::{distributions::Standard, prelude::Distribution};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square {
    x: usize,
}

impl Square {
    pub const S11: Square = Square::new(0, 0);
    pub const S12: Square = Square::new(0, 1);
    pub const S13: Square = Square::new(0, 2);
    pub const S14: Square = Square::new(0, 3);
    pub const S15: Square = Square::new(0, 4);
    pub const S16: Square = Square::new(0, 5);
    pub const S17: Square = Square::new(0, 6);
    pub const S18: Square = Square::new(0, 7);
    pub const S19: Square = Square::new(0, 8);
    pub const S21: Square = Square::new(1, 0);
    pub const S22: Square = Square::new(1, 1);
    pub const S23: Square = Square::new(1, 2);
    pub const S24: Square = Square::new(1, 3);
    pub const S25: Square = Square::new(1, 4);
    pub const S26: Square = Square::new(1, 5);
    pub const S27: Square = Square::new(1, 6);
    pub const S28: Square = Square::new(1, 7);
    pub const S29: Square = Square::new(1, 8);
    pub const S31: Square = Square::new(2, 0);
    pub const S32: Square = Square::new(2, 1);
    pub const S33: Square = Square::new(2, 2);
    pub const S34: Square = Square::new(2, 3);
    pub const S35: Square = Square::new(2, 4);
    pub const S36: Square = Square::new(2, 5);
    pub const S37: Square = Square::new(2, 6);
    pub const S38: Square = Square::new(2, 7);
    pub const S39: Square = Square::new(2, 8);
    pub const S41: Square = Square::new(3, 0);
    pub const S42: Square = Square::new(3, 1);
    pub const S43: Square = Square::new(3, 2);
    pub const S44: Square = Square::new(3, 3);
    pub const S45: Square = Square::new(3, 4);
    pub const S46: Square = Square::new(3, 5);
    pub const S47: Square = Square::new(3, 6);
    pub const S48: Square = Square::new(3, 7);
    pub const S49: Square = Square::new(3, 8);
    pub const S51: Square = Square::new(4, 0);
    pub const S52: Square = Square::new(4, 1);
    pub const S53: Square = Square::new(4, 2);
    pub const S54: Square = Square::new(4, 3);
    pub const S55: Square = Square::new(4, 4);
    pub const S56: Square = Square::new(4, 5);
    pub const S57: Square = Square::new(4, 6);
    pub const S58: Square = Square::new(4, 7);
    pub const S59: Square = Square::new(4, 8);
    pub const S61: Square = Square::new(5, 0);
    pub const S62: Square = Square::new(5, 1);
    pub const S63: Square = Square::new(5, 2);
    pub const S64: Square = Square::new(5, 3);
    pub const S65: Square = Square::new(5, 4);
    pub const S66: Square = Square::new(5, 5);
    pub const S67: Square = Square::new(5, 6);
    pub const S68: Square = Square::new(5, 7);
    pub const S69: Square = Square::new(5, 8);
    pub const S71: Square = Square::new(6, 0);
    pub const S72: Square = Square::new(6, 1);
    pub const S73: Square = Square::new(6, 2);
    pub const S74: Square = Square::new(6, 3);
    pub const S75: Square = Square::new(6, 4);
    pub const S76: Square = Square::new(6, 5);
    pub const S77: Square = Square::new(6, 6);
    pub const S78: Square = Square::new(6, 7);
    pub const S79: Square = Square::new(6, 8);
    pub const S81: Square = Square::new(7, 0);
    pub const S82: Square = Square::new(7, 1);
    pub const S83: Square = Square::new(7, 2);
    pub const S84: Square = Square::new(7, 3);
    pub const S85: Square = Square::new(7, 4);
    pub const S86: Square = Square::new(7, 5);
    pub const S87: Square = Square::new(7, 6);
    pub const S88: Square = Square::new(7, 7);
    pub const S89: Square = Square::new(7, 8);
    pub const S91: Square = Square::new(8, 0);
    pub const S92: Square = Square::new(8, 1);
    pub const S93: Square = Square::new(8, 2);
    pub const S94: Square = Square::new(8, 3);
    pub const S95: Square = Square::new(8, 4);
    pub const S96: Square = Square::new(8, 5);
    pub const S97: Square = Square::new(8, 6);
    pub const S98: Square = Square::new(8, 7);
    pub const S99: Square = Square::new(8, 8);

    pub const fn new(col: usize, row: usize) -> Self {
        debug_assert!(col < 9 && row < 9);
        Self { x: col * 9 + row }
    }
    pub const fn col(self) -> usize {
        self.x / 9
    }
    pub const fn row(self) -> usize {
        self.x % 9
    }
    pub fn iter() -> impl Iterator<Item = Self> {
        (0..81).map(Self::from_index)
    }
    pub const fn index(self) -> usize {
        self.x
    }
    pub const fn from_index(x: usize) -> Self {
        debug_assert!(x < 81);
        Self { x }
    }

    pub(crate) fn shift(&mut self, dir: crate::direction::Direction) {
        let col = (self.col() as isize + dir.col() + 9) % 9;
        let row = (self.row() as isize + dir.row() + 9) % 9;
        *self = Square::new(col as usize, row as usize)
    }

    pub(crate) fn flipped(&self) -> Square {
        Square::new(8 - self.col(), 8 - self.row())
    }

    pub fn parity(&self) -> bool {
        (self.col() + self.row()) % 2 == 1
    }
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "S{}{}", self.col() + 1, self.row() + 1)
    }
}

impl Distribution<Square> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Square {
        Square::from_index(rng.gen_range(0..81))
    }
}
