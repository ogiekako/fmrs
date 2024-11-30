use rand::{distributions::Standard, prelude::Distribution};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square {
    x: usize,
}

impl Square {
    pub fn new(col: usize, row: usize) -> Self {
        Self { x: col * 9 + row }
    }
    pub fn col(self) -> usize {
        self.x / 9
    }
    pub fn row(self) -> usize {
        self.x % 9
    }
    pub fn iter() -> impl Iterator<Item = Self> {
        (0..9).flat_map(|col| (0..9).map(move |row| Self::new(col, row)))
    }
    pub(super) fn index(self) -> usize {
        self.x
    }
    pub(super) fn from_index(x: usize) -> Self {
        Self { x }
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
