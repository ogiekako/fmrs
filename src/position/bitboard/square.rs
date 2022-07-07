#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        13 + self.col() * 11 + self.row()
    }
    pub(super) fn index81(self) -> usize {
        self.col() * 9 + self.row()
    }
    pub(super) fn from_index(x: usize) -> Self {
        let col = (x - 13) / 11;
        let row = (x - 13) % 11;
        Self::new(col, row)
    }
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}{}", self.col() + 1, self.row() + 1)
    }
}
