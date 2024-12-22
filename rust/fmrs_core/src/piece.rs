#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Color(bool);

impl Color {
    pub const BLACK: Color = Color(false);
    pub const WHITE: Color = Color(true);
}

use rand::prelude::Distribution;

impl Color {
    pub fn index(&self) -> usize {
        self.0 as usize
    }
    pub fn iter() -> impl Iterator<Item = Color> {
        [Color::BLACK, Color::WHITE].iter().copied()
    }
    pub fn opposite(self) -> Color {
        Color(!self.0)
    }
    pub fn is_black(self) -> bool {
        !self.0
    }
    pub fn is_white(self) -> bool {
        self.0
    }
    pub fn from_is_black(b: bool) -> Color {
        Color(!b)
    }
    pub fn from_is_white(b: bool) -> Color {
        Color(b)
    }
}

impl Distribution<Color> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Color {
        if rng.gen() {
            Color::BLACK
        } else {
            Color::WHITE
        }
    }
}

#[test]
fn test_color_index() {
    assert_eq!(Color::BLACK.index(), 0);
    assert_eq!(Color::WHITE.index(), 1);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Pawn,
    Lance,
    Knight,
    Silver,
    Gold,
    Bishop,
    Rook,
    King,    // 7
    ProPawn, // 8
    ProLance,
    ProKnight,
    ProSilver,
    ProBishop,
    ProRook, // 13
             // 14
}

const LINE_PIECE_MASK: usize = 1 << Lance.index()
    | 1 << Bishop.index()
    | 1 << Rook.index()
    | 1 << ProBishop.index()
    | 1 << ProRook.index();

impl Distribution<Kind> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Kind {
        Kind::from_index(rng.gen_range(0..NUM_KIND))
    }
}

use serde::Serialize;
pub use Kind::*;

pub const KINDS: [Kind; NUM_KIND] = [
    Pawn, Lance, Knight, Silver, Gold, Bishop, Rook, // kinds that can be in hand
    King, ProPawn, ProLance, ProKnight, ProSilver, ProBishop, ProRook,
];

pub const NUM_HAND_KIND: usize = 7;
pub const NUM_KIND: usize = 14;

impl Kind {
    pub const fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(x: usize) -> Self {
        KINDS[x]
    }
    pub fn iter() -> impl Iterator<Item = Kind> {
        KINDS.iter().copied()
    }

    pub fn promote(self) -> Option<Kind> {
        Some(match self {
            Pawn => ProPawn,
            Lance => ProLance,
            Knight => ProKnight,
            Silver => ProSilver,
            Bishop => ProBishop,
            Rook => ProRook,
            _ => return None,
        })
    }

    pub fn maybe_unpromote(self) -> Kind {
        match self {
            ProPawn => Pawn,
            ProLance => Lance,
            ProKnight => Knight,
            ProSilver => Silver,
            ProBishop => Bishop,
            ProRook => Rook,
            _ => self,
        }
    }

    pub fn unpromote(self) -> Option<Kind> {
        Some(match self {
            ProPawn => Pawn,
            ProLance => Lance,
            ProKnight => Knight,
            ProSilver => Silver,
            ProBishop => Bishop,
            ProRook => Rook,
            _ => return None,
        })
    }
    pub fn is_line_piece(self) -> bool {
        LINE_PIECE_MASK & 1 << self.index() != 0
    }
    pub fn is_hand_piece(self) -> bool {
        self.index() < NUM_HAND_KIND
    }
    pub fn is_promotable(self) -> bool {
        self.is_hand_piece() && self != Gold
    }
}

pub struct Kinds {
    mask: usize,
}

impl Kinds {
    pub fn new(mask: usize) -> Self {
        Self { mask }
    }
}

impl Iterator for Kinds {
    type Item = Kind;

    fn next(&mut self) -> Option<Self::Item> {
        if self.mask == 0 {
            return None;
        }
        let x = self.mask.trailing_zeros() as usize;
        self.mask &= !(1 << x);
        Some(Kind::from_index(x))
    }
}
