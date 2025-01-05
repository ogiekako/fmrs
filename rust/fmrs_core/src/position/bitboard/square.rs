use rand::{distributions::Standard, prelude::Distribution};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Square {
    x: usize,
}

impl Square {
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
    pub(crate) const fn index(self) -> usize {
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
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}{}", self.col() + 1, self.row() + 1)
    }
}

impl Distribution<Square> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Square {
        Square::from_index(rng.gen_range(0..81))
    }
}
