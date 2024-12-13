#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
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
    pub fn is_white(self) -> bool {
        self.0
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

#[derive(Debug)]
struct KindImpl {
    index: usize,
    is_line_piece: bool,
    is_hand_piece: bool,
    is_promotable: bool,
}

impl KindImpl {
    const fn new(
        index: usize,
        is_line_piece: bool,
        is_hand_piece: bool,
        is_promotable: bool,
    ) -> Self {
        KindImpl {
            index,
            is_line_piece,
            is_hand_piece,
            is_promotable,
        }
    }
}

const PAWN_IMPL: KindImpl = KindImpl::new(0, false, true, true);
const LANCE_IMPL: KindImpl = KindImpl::new(1, true, true, true);
const KNIGHT_IMPL: KindImpl = KindImpl::new(2, false, true, true);
const SILVER_IMPL: KindImpl = KindImpl::new(3, false, true, true);
const GOLD_IMPL: KindImpl = KindImpl::new(4, false, true, false);
const BISHOP_IMPL: KindImpl = KindImpl::new(5, true, true, true);
const ROOK_IMPL: KindImpl = KindImpl::new(6, true, true, true);
const KING_IMPL: KindImpl = KindImpl::new(7, false, false, false);
const PRO_PAWN_IMPL: KindImpl = KindImpl::new(8, false, false, false);
const PRO_LANCE_IMPL: KindImpl = KindImpl::new(9, false, false, false);
const PRO_KNIGHT_IMPL: KindImpl = KindImpl::new(10, false, false, false);
const PRO_SILVER_IMPL: KindImpl = KindImpl::new(11, false, false, false);
const PRO_BISHOP_IMPL: KindImpl = KindImpl::new(12, true, false, false);
const PRO_ROOK_IMPL: KindImpl = KindImpl::new(13, true, false, false);

pub const KINDS: [Kind; NUM_KIND] = [
    Kind::PAWN,
    Kind::LANCE,
    Kind::KNIGHT,
    Kind::SILVER,
    Kind::GOLD,
    Kind::BISHOP,
    Kind::ROOK,
    Kind::KING,
    Kind::PRO_PAWN,
    Kind::PRO_LANCE,
    Kind::PRO_KNIGHT,
    Kind::PRO_SILVER,
    Kind::PRO_BISHOP,
    Kind::PRO_ROOK,
];

impl PartialEq for KindImpl {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for KindImpl {}

impl PartialOrd for KindImpl {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.index.partial_cmp(&other.index)
    }
}

impl Ord for KindImpl {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Kind(&'static KindImpl);

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self.index() {
            Kind::PAWN_ID => "P",
            Kind::LANCE_ID => "L",
            Kind::KNIGHT_ID => "N",
            Kind::SILVER_ID => "S",
            Kind::GOLD_ID => "G",
            Kind::BISHOP_ID => "B",
            Kind::ROOK_ID => "R",
            Kind::KING_ID => "K",
            Kind::PRO_PAWN_ID => "+P",
            Kind::PRO_LANCE_ID => "+L",
            Kind::PRO_KNIGHT_ID => "+N",
            Kind::PRO_SILVER_ID => "+S",
            Kind::PRO_BISHOP_ID => "+B",
            Kind::PRO_ROOK_ID => "+R",
            _ => unreachable!(),
        };
        write!(f, "{}", name)
    }
}

impl Kind {
    const fn new(k: &'static KindImpl) -> Self {
        Kind(k)
    }

    pub const PAWN: Kind = Self::new(&PAWN_IMPL);
    pub const LANCE: Kind = Self::new(&LANCE_IMPL);
    pub const KNIGHT: Kind = Self::new(&KNIGHT_IMPL);
    pub const SILVER: Kind = Self::new(&SILVER_IMPL);
    pub const GOLD: Kind = Self::new(&GOLD_IMPL);
    pub const BISHOP: Kind = Self::new(&BISHOP_IMPL);
    pub const ROOK: Kind = Self::new(&ROOK_IMPL);
    pub const KING: Kind = Self::new(&KING_IMPL);
    pub const PRO_PAWN: Kind = Self::new(&PRO_PAWN_IMPL);
    pub const PRO_LANCE: Kind = Self::new(&PRO_LANCE_IMPL);
    pub const PRO_KNIGHT: Kind = Self::new(&PRO_KNIGHT_IMPL);
    pub const PRO_SILVER: Kind = Self::new(&PRO_SILVER_IMPL);
    pub const PRO_BISHOP: Kind = Self::new(&PRO_BISHOP_IMPL);
    pub const PRO_ROOK: Kind = Self::new(&PRO_ROOK_IMPL);

    pub const PAWN_ID: usize = Kind::PAWN.index();
    pub const LANCE_ID: usize = Kind::LANCE.index();
    pub const KNIGHT_ID: usize = Kind::KNIGHT.index();
    pub const SILVER_ID: usize = Kind::SILVER.index();
    pub const GOLD_ID: usize = Kind::GOLD.index();
    pub const BISHOP_ID: usize = Kind::BISHOP.index();
    pub const ROOK_ID: usize = Kind::ROOK.index();
    pub const KING_ID: usize = Kind::KING.index();
    pub const PRO_PAWN_ID: usize = Kind::PRO_PAWN.index();
    pub const PRO_LANCE_ID: usize = Kind::PRO_LANCE.index();
    pub const PRO_KNIGHT_ID: usize = Kind::PRO_KNIGHT.index();
    pub const PRO_SILVER_ID: usize = Kind::PRO_SILVER.index();
    pub const PRO_BISHOP_ID: usize = Kind::PRO_BISHOP.index();
    pub const PRO_ROOK_ID: usize = Kind::PRO_ROOK.index();

    pub const fn index(&self) -> usize {
        self.0.index
    }

    pub fn is_line_piece(&self) -> bool {
        self.0.is_line_piece
    }

    pub fn is_hand_piece(&self) -> bool {
        self.0.is_hand_piece
    }

    pub fn is_promotable(&self) -> bool {
        self.0.is_promotable
    }

    pub fn from_index(x: usize) -> Self {
        KINDS[x]
    }
    pub fn iter() -> impl Iterator<Item = Kind> {
        KINDS.iter().copied()
    }

    pub fn promote(&self) -> Option<Kind> {
        Some(match self.index() {
            Kind::PAWN_ID => Kind::PRO_PAWN,
            Kind::LANCE_ID => Kind::PRO_LANCE,
            Kind::KNIGHT_ID => Kind::PRO_KNIGHT,
            Kind::SILVER_ID => Kind::PRO_SILVER,
            Kind::BISHOP_ID => Kind::PRO_BISHOP,
            Kind::ROOK_ID => Kind::PRO_ROOK,
            _ => return None,
        })
    }

    pub fn maybe_unpromote(&self) -> Kind {
        self.unpromote().unwrap_or(*self)
    }

    pub fn unpromote(&self) -> Option<Kind> {
        Some(match self.index() {
            Kind::PRO_PAWN_ID => Kind::PAWN,
            Kind::PRO_LANCE_ID => Kind::LANCE,
            Kind::PRO_KNIGHT_ID => Kind::KNIGHT,
            Kind::PRO_SILVER_ID => Kind::SILVER,
            Kind::PRO_BISHOP_ID => Kind::BISHOP,
            Kind::PRO_ROOK_ID => Kind::ROOK,
            _ => return None,
        })
    }
}

impl Distribution<Kind> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Kind {
        Kind::from_index(rng.gen_range(0..NUM_KIND))
    }
}

pub const NUM_HAND_KIND: usize = 7;
pub const NUM_KIND: usize = 14;

#[cfg(test)]
mod tests {
    use super::Kind;
    use crate::piece::KINDS;

    #[test]
    fn test_eq() {
        let p = KINDS[0];
        assert_eq!(p, Kind::PAWN);
        assert_ne!(p, Kind::LANCE);
    }
}
