#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Square {
    x: usize,
}

impl Square {
    pub fn new(col: usize, row: usize) -> Self {
        Self { x: col << 32 | row }
    }
    pub fn col(self) -> usize {
        self.x >> 32
    }
    pub fn row(self) -> usize {
        self.x & 0xFFFFFFFF
    }
    pub fn iter() -> impl Iterator<Item = Self> {
        (0..9).flat_map(|col| (0..9).map(move |row| Self::new(col, row)))
    }
    pub(super) fn index(self) -> usize {
        15 + self.col() * 13 + self.row()
    }
    pub(super) fn from_index(x: usize) -> Self {
        let col = (x - 15) / 13;
        let row = (x - 15) % 13;
        Self::new(col, row)
    }
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}{}", self.col() + 1, self.row() + 1)
    }
}
