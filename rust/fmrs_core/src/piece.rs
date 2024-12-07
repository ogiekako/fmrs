#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum Color {
    Black, // Moves first. e.g. Tsume kata.
    White, // Uke kata.
}
use rand::prelude::Distribution;
pub use Color::*;

impl Color {
    #[inline(always)]
    pub const fn index(&self) -> usize {
        *self as usize
    }
    pub fn iter() -> impl Iterator<Item = Color> {
        [Black, White].iter().copied()
    }
    #[inline(always)]
    pub fn opposite(&self) -> Color {
        match self {
            Black => White,
            White => Black,
        }
    }
}

impl Distribution<Color> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Color {
        if rng.gen() {
            Black
        } else {
            White
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
    ProRook, // 13
             // 14
}

impl Distribution<Kind> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Kind {
        Kind::from_index(rng.gen_range(0..NUM_KIND))
    }
}

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
    pub fn to_essential_kind(&self) -> EssentialKind {
        match self {
            Pawn => EssentialKind::Pawn,
            Lance => EssentialKind::Lance,
            Knight => EssentialKind::Knight,
            Silver => EssentialKind::Silver,
            Gold | ProPawn | ProLance | ProKnight | ProSilver => EssentialKind::Gold,
            Bishop => EssentialKind::Bishop,
            Rook => EssentialKind::Rook,
            King => EssentialKind::King,
            ProBishop => EssentialKind::ProBishop,
            ProRook => EssentialKind::ProRook,
        }
    }

    pub fn is_essentially_gold(&self) -> bool {
        matches!(self, Gold | ProPawn | ProLance | ProKnight | ProSilver)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EssentialKind {
    Pawn,
    Lance,
    Knight,
    Silver,
    Gold,
    Bishop,
    Rook,
    King,
    ProBishop,
    ProRook,
}

const ESSENTIAL_KINDS: [EssentialKind; 10] = [
    EssentialKind::Pawn,
    EssentialKind::Lance,
    EssentialKind::Knight,
    EssentialKind::Silver,
    EssentialKind::Gold,
    EssentialKind::Bishop,
    EssentialKind::Rook,
    EssentialKind::King,
    EssentialKind::ProBishop,
    EssentialKind::ProRook,
];

impl EssentialKind {
    pub fn iter() -> impl Iterator<Item = EssentialKind> {
        ESSENTIAL_KINDS.iter().copied()
    }

    #[inline(always)]
    pub const fn index(&self) -> usize {
        *self as usize
    }

    pub fn unique_kind(&self) -> Option<Kind> {
        match self {
            EssentialKind::Gold => return None,
            EssentialKind::ProBishop => return Some(ProBishop),
            EssentialKind::ProRook => return Some(ProRook),
            _ => return Some(Kind::from_index(self.index())),
        }
    }

    pub(crate) fn is_line_piece(&self) -> bool {
        matches!(
            self,
            EssentialKind::Lance
                | EssentialKind::Bishop
                | EssentialKind::Rook
                | EssentialKind::ProBishop
                | EssentialKind::ProRook
        )
    }

    pub(crate) fn promote(&self) -> Option<Kind> {
        match self {
            EssentialKind::Pawn => Some(ProPawn),
            EssentialKind::Lance => Some(ProLance),
            EssentialKind::Knight => Some(ProKnight),
            EssentialKind::Silver => Some(ProSilver),
            EssentialKind::Bishop => Some(ProBishop),
            EssentialKind::Rook => Some(ProRook),
            _ => None,
        }
    }
}
