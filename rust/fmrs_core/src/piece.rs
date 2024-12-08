#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum Color {
    Black, // Moves first. e.g. Tsume kata.
    White, // Uke kata.
}
use rand::prelude::Distribution;
pub use Color::*;

impl Color {
    pub const fn index(&self) -> usize {
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

pub const KIND_TO_ESSENTIAL_KIND: [EssentialKind; NUM_KIND] = [
    EssentialKind::Pawn,
    EssentialKind::Lance,
    EssentialKind::Knight,
    EssentialKind::Silver,
    EssentialKind::Gold,
    EssentialKind::Bishop,
    EssentialKind::Rook,
    EssentialKind::King,
    EssentialKind::Gold,
    EssentialKind::Gold,
    EssentialKind::Gold,
    EssentialKind::Gold,
    EssentialKind::ProBishop,
    EssentialKind::ProRook,
];

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
        let i = self.index();
        if i < Kind::ProPawn.index() {
            return EssentialKind::from_index(i);
        } else if i < Kind::ProBishop.index() {
            return EssentialKind::Gold;
        } else {
            return EssentialKind::from_index(i - 4);
        }
    }

    pub fn is_essentially_gold(&self) -> bool {
        KIND_TO_ESSENTIAL_KIND[self.index()] == EssentialKind::Gold
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

pub const ESSENTIAL_KINDS: [EssentialKind; 10] = [
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

const ESSENTIAL_KIND_TRANSITIONS: [(EssentialKind, EssentialKind); 16] = [
    (EssentialKind::Pawn, EssentialKind::Pawn),
    (EssentialKind::Pawn, EssentialKind::Gold),
    (EssentialKind::Lance, EssentialKind::Lance),
    (EssentialKind::Lance, EssentialKind::Gold),
    (EssentialKind::Knight, EssentialKind::Knight),
    (EssentialKind::Knight, EssentialKind::Gold),
    (EssentialKind::Silver, EssentialKind::Silver),
    (EssentialKind::Silver, EssentialKind::Gold),
    (EssentialKind::Gold, EssentialKind::Gold),
    (EssentialKind::Bishop, EssentialKind::Bishop),
    (EssentialKind::Bishop, EssentialKind::ProBishop),
    (EssentialKind::Rook, EssentialKind::Rook),
    (EssentialKind::Rook, EssentialKind::ProRook),
    (EssentialKind::King, EssentialKind::King),
    (EssentialKind::ProBishop, EssentialKind::ProBishop),
    (EssentialKind::ProRook, EssentialKind::ProRook),
];

impl EssentialKind {
    pub fn iter() -> impl Iterator<Item = EssentialKind> {
        ESSENTIAL_KINDS.iter().copied()
    }

    pub const fn index(&self) -> usize {
        *self as usize
    }

    // #[inline(never)]
    pub fn unique_kind(&self) -> Option<Kind> {
        match self {
            EssentialKind::Gold => return None,
            EssentialKind::ProBishop => return Some(ProBishop),
            EssentialKind::ProRook => return Some(ProRook),
            _ => return Some(Kind::from_index(self.index())),
        }
    }

    pub fn promote(&self) -> Option<EssentialKind> {
        match self {
            EssentialKind::Pawn => Some(EssentialKind::Gold),
            EssentialKind::Lance => Some(EssentialKind::Gold),
            EssentialKind::Knight => Some(EssentialKind::Gold),
            EssentialKind::Silver => Some(EssentialKind::Gold),
            EssentialKind::Bishop => Some(EssentialKind::ProBishop),
            EssentialKind::Rook => Some(EssentialKind::ProRook),
            _ => None,
        }
    }

    // #[inline(never)]
    pub(crate) fn promote_to_kind(&self) -> Option<Kind> {
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

    fn from_index(i: usize) -> EssentialKind {
        ESSENTIAL_KINDS[i]
    }

    pub fn hand_to_kind(&self) -> Kind {
        debug_assert!(self.index() < NUM_HAND_KIND, "{:?}", self);
        Kind::from_index(self.index())
    }

    pub(crate) fn iter_transitions() -> impl Iterator<Item = (EssentialKind, EssentialKind)> {
        ESSENTIAL_KIND_TRANSITIONS.iter().copied()
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

    pub fn is_hand_piece(&self) -> bool {
        self.index() < EssentialKind::King.index()
    }
}
