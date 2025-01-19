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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

use serde::{Deserialize, Serialize};
pub use Kind::*;

use crate::position::BitBoard;

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
        const MASK: usize = 1 << Lance.index()
            | 1 << Bishop.index()
            | 1 << Rook.index()
            | 1 << ProBishop.index()
            | 1 << ProRook.index();

        MASK & 1 << self.index() != 0
    }
    pub fn is_hand_piece(self) -> bool {
        self.index() < NUM_HAND_KIND
    }
    pub fn can_promote(self) -> bool {
        self.is_hand_piece() && self != Gold
    }

    pub fn max_count(&self) -> u32 {
        match self.maybe_unpromote() {
            Kind::Pawn => 18,
            Kind::Lance | Kind::Knight | Kind::Silver | Kind::Gold => 4,
            _ => 2,
        }
    }
    pub fn effect(&self) -> KindEffect {
        match self {
            Pawn => KindEffect::Pawn,
            Lance => KindEffect::Lance,
            Knight => KindEffect::Knight,
            Silver => KindEffect::Silver,
            Bishop => KindEffect::Bishop,
            Rook => KindEffect::Rook,
            King => KindEffect::King,
            ProBishop => KindEffect::ProBishop,
            ProRook => KindEffect::ProRook,
            _ => KindEffect::Gold,
        }
    }

    pub(crate) fn unmovable_bb(&self, color: Color) -> BitBoard {
        match (self, color) {
            (Pawn | Lance, Color::BLACK) => BitBoard::ROW1,
            (Knight, Color::BLACK) => BitBoard::ROW1 | BitBoard::ROW2,
            (Pawn | Lance, Color::WHITE) => BitBoard::ROW9,
            (Knight, Color::WHITE) => BitBoard::ROW8 | BitBoard::ROW9,
            _ => BitBoard::EMPTY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Kinds {
    mask: usize,
}

impl Serialize for Kinds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Kinds {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = Vec::<Kind>::deserialize(deserializer)?;
        Ok(Self::from(v))
    }
}

impl From<Vec<Kind>> for Kinds {
    fn from(v: Vec<Kind>) -> Self {
        let mut kinds = Kinds::default();
        for kind in v {
            kinds.set(kind);
        }
        kinds
    }
}

impl Kinds {
    pub const PAWN: Kinds = Kinds::new(1 << Pawn.index());
    pub const LANCE: Kinds = Kinds::new(1 << Lance.index());
    pub const KNIGHT: Kinds = Kinds::new(1 << Knight.index());
    pub const SILVER: Kinds = Kinds::new(1 << Silver.index());
    pub const BISHOP: Kinds = Kinds::new(1 << Bishop.index());
    pub const ROOK: Kinds = Kinds::new(1 << Rook.index());
    pub const KING: Kinds = Kinds::new(1 << King.index());
    pub const PRO_BISHOP: Kinds = Kinds::new(1 << ProBishop.index());
    pub const PRO_ROOK: Kinds = Kinds::new(1 << ProRook.index());
    pub const GOLDISH: Kinds = Kinds::new(
        1 << Gold.index()
            | 1 << ProPawn.index()
            | 1 << ProLance.index()
            | 1 << ProKnight.index()
            | 1 << ProSilver.index(),
    );

    pub const fn new(mask: usize) -> Self {
        Self { mask }
    }

    pub fn set(&mut self, kind: Kind) {
        self.mask |= 1 << kind.index();
    }

    pub fn count_ones(&self) -> u32 {
        self.mask.count_ones()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KindEffect {
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

pub const KIND_EFFECTS: [KindEffect; 10] = [
    KindEffect::Pawn,
    KindEffect::Lance,
    KindEffect::Knight,
    KindEffect::Silver,
    KindEffect::Gold,
    KindEffect::Bishop,
    KindEffect::Rook,
    KindEffect::King,
    KindEffect::ProBishop,
    KindEffect::ProRook,
];

impl KindEffect {
    pub fn index(&self) -> usize {
        *self as usize
    }
    pub fn from_index(x: usize) -> Self {
        KIND_EFFECTS[x]
    }
    pub fn iter() -> impl Iterator<Item = KindEffect> {
        KIND_EFFECTS.iter().copied()
    }

    pub fn kinds(&self) -> Kinds {
        match self {
            KindEffect::Pawn => Kinds::PAWN,
            KindEffect::Lance => Kinds::LANCE,
            KindEffect::Knight => Kinds::KNIGHT,
            KindEffect::Silver => Kinds::SILVER,
            KindEffect::Gold => Kinds::GOLDISH,
            KindEffect::Bishop => Kinds::BISHOP,
            KindEffect::Rook => Kinds::ROOK,
            KindEffect::King => Kinds::KING,
            KindEffect::ProBishop => Kinds::PRO_BISHOP,
            KindEffect::ProRook => Kinds::PRO_ROOK,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KindEffects {
    mask: u16,
}

impl KindEffects {
    pub fn new(mask: u16) -> Self {
        Self { mask }
    }

    pub fn set(&mut self, kind: KindEffect) {
        self.mask |= 1 << kind as u16;
    }

    pub fn count_ones(&self) -> u32 {
        self.mask.count_ones()
    }

    pub fn contains(&self, effect: KindEffect) -> bool {
        self.mask & (1 << effect as u16) != 0
    }
}

impl Iterator for KindEffects {
    type Item = KindEffect;

    fn next(&mut self) -> Option<Self::Item> {
        if self.mask == 0 {
            return None;
        }
        let x = self.mask.trailing_zeros() as u16;
        self.mask &= !(1 << x);
        Some(KindEffect::from_index(x as usize))
    }
}
