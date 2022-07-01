#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square {
    x: usize,
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Square {}{}", self.col() + 1, self.row() + 1)
    }
}

impl Square {
    pub fn new(col: usize, row: usize) -> Square {
        debug_assert!(col < 9);
        debug_assert!(row < 9);
        Square::from_index(col * 9 + row)
    }

    pub fn from_index(x: usize) -> Square {
        debug_assert!(x < 81);
        Square { x }
    }

    pub fn index(&self) -> usize {
        self.x as usize
    }

    pub fn col(&self) -> usize {
        self.x / 9
    }

    pub fn row(&self) -> usize {
        self.x % 9
    }

    pub fn iter() -> impl Iterator<Item = Square> {
        (0..81).map(|i| Square::from_index(i))
    }

    pub fn add(&self, col: isize, row: isize) -> Option<Square> {
        let (c, r) = (self.col() as isize + col, self.row() as isize + row);
        if 0 <= c && c < 9 && 0 <= r && r < 9 {
            Some(Square::new(c as usize, r as usize))
        } else {
            None
        }
    }
}
