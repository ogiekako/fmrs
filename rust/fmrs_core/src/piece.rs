#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub enum Color {
    Black, // Moves first. e.g. Tsume kata.
    White, // Uke kata.
}
pub use Color::*;

impl Color {
    pub fn index(&self) -> usize {
        *self as usize
    }
    pub fn iter() -> impl Iterator<Item = Color> {
        [Black, White].iter().copied()
    }
    pub fn opposite(&self) -> Color {
        match self {
            Black => White,
            White => Black,
        }
    }
}

#[test]
fn test_color_index() {
    assert_eq!(Black.index(), 0);
    assert_eq!(White.index(), 1);
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
    ProRook,
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
    pub fn index(&self) -> usize {
        *self as usize
    }
    pub fn from_index(x: usize) -> Self {
        KINDS[x]
    }
    pub fn iter() -> impl Iterator<Item = Kind> {
        KINDS.iter().copied()
    }

    pub fn promote(&self) -> Option<Kind> {
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

    pub fn maybe_unpromote(&self) -> Kind {
        self.unpromote().unwrap_or(*self)
    }

    pub fn unpromote(&self) -> Option<Kind> {
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
    pub fn is_line_piece(&self) -> bool {
        matches!(self, Lance | Bishop | Rook | ProBishop | ProRook)
    }
    pub fn is_hand_piece(&self) -> bool {
        matches!(self, Pawn | Lance | Knight | Silver | Gold | Bishop | Rook)
    }
}
