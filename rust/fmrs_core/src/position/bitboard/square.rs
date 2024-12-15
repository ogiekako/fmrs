use rand::{distributions::Standard, prelude::Distribution};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Square {
    x: usize,
}

const ROW: [usize; 81] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 1
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 2
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 3
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 4
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 5
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 6
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 7
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 8
    0, 1, 2, 3, 4, 5, 6, 7, 8, // 9
];

impl Square {
    pub fn new(col: usize, row: usize) -> Self {
        debug_assert!(col < 9 && row < 9);
        Self { x: col * 9 + row }
    }
    pub fn col(self) -> usize {
        self.x / 9
    }
    pub fn row(self) -> usize {
        ROW[self.x]
    }
    pub fn iter() -> impl Iterator<Item = Self> {
        (0..81).map(Self::from_index)
    }
    pub(crate) fn index(self) -> usize {
        self.x
    }
    pub(crate) fn from_index(x: usize) -> Self {
        debug_assert!(x < 81);
        Self { x }
    }

    pub(crate) fn col_row(&self) -> (usize, usize) {
        (self.col(), self.row())
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
