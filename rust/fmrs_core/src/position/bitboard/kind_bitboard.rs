use crate::piece::Kind;

use super::{BitBoard, Square};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Default, Debug)]
pub struct KindBitBoard {
    promote: BitBoard,
    kind0: BitBoard,
    kind1: BitBoard,
    kind2: BitBoard,
}

// promote = 0:
// 1: Pawn
// 2: Lance
// 3: Knight
// 4: Silver
// 5: Gold
// 6: Bishop
// 7: Rook

// promote = 1:
// 1: ProPawn
// 2: ProLance
// 3: ProKnight
// 4: ProSilver
// 5: King
// 6: ProBishop
// 7: ProRook

const KINDS: [Kind; 16] = [
    Kind::King, // dummy
    Kind::Pawn,
    Kind::Lance,
    Kind::Knight,
    Kind::Silver,
    Kind::Gold,
    Kind::Bishop,
    Kind::Rook,
    Kind::King, // dummy
    Kind::ProPawn,
    Kind::ProLance,
    Kind::ProKnight,
    Kind::ProSilver,
    Kind::King,
    Kind::ProBishop,
    Kind::ProRook,
];

#[test]
fn test_kind_bitboard_size() {
    assert_eq!(64, std::mem::size_of::<KindBitBoard>());
}

impl KindBitBoard {
    // #[inline(never)]
    pub fn goldish(&self) -> BitBoard {
        // p a b c
        // (false, 5), (true, 1), (true, 2), (true, 3), (true, 4)
        // p & ~c | (p ^ a) & ~b & c
        self.promote.and_not(self.kind2)
            | (self.promote ^ self.kind0).and_not(self.kind1) & (self.kind2)
    }

    pub(crate) fn pawn_silver_goldish(&self) -> BitBoard {
        // p a b c
        // (false, 1), (false, 4), (false, 5), (true, 1), (true, 2), (true, 3), (true, 4)
        // ~p & a & ~b & ~c | ~p & ~a & ~b & c | p & ~c | (p & ~a | ~p & a) & ~b & c
        // = ~b & (~p & (a ^ c) | c & (p ^ a)) | p & ~c

        ((self.kind0 ^ self.kind2).and_not(self.promote) | (self.promote ^ self.kind0) & self.kind2)
            .and_not(self.kind1)
            | self.promote.and_not(self.kind2)
    }

    // rook and prorook
    // #[inline(never)]
    pub fn rookish(&self) -> BitBoard {
        self.kind0 & self.kind1 & self.kind2
    }

    // bishop and probishop
    // #[inline(never)]
    pub fn bishopish(&self) -> BitBoard {
        (self.kind1 & self.kind2).and_not(self.kind0)
    }

    fn ids(kind: Kind) -> (bool, usize) {
        if kind.index() < 7 {
            return (false, kind.index() + 1);
        }
        (
            true,
            match kind {
                Kind::ProPawn => 1,
                Kind::ProLance => 2,
                Kind::ProKnight => 3,
                Kind::ProSilver => 4,
                Kind::King => 5,
                Kind::ProBishop => 6,
                Kind::ProRook => 7,
                _ => unreachable!("{:?}", kind),
            },
        )
    }

    pub fn bitboard(&self, kind: Kind) -> BitBoard {
        let (promote, i) = Self::ids(kind);

        let b = match i {
            1 => self.kind0.and_not(self.kind1 | self.kind2),
            2 => self.kind1.and_not(self.kind0 | self.kind2),
            3 => (self.kind0 & self.kind1).and_not(self.kind2),
            4 => self.kind2.and_not(self.kind0 | self.kind1),
            5 => (self.kind0 & self.kind2).and_not(self.kind1),
            6 => (self.kind1 & self.kind2).and_not(self.kind0),
            7 => self.kind0 & self.kind1 & self.kind2,
            _ => unreachable!(),
        };
        if promote {
            b & self.promote
        } else {
            b.and_not(self.promote)
        }
    }
    // #[inline(never)]
    pub fn must_get(&self, pos: Square) -> Kind {
        let mut i = 0;
        if self.kind0.get(pos) {
            i |= 1;
        }
        if self.kind1.get(pos) {
            i |= 2;
        }
        if self.kind2.get(pos) {
            i |= 4;
        }
        debug_assert_ne!(i, 0);
        if self.promote.get(pos) {
            i |= 8;
        }
        KINDS[i]
    }
    // #[inline(never)]
    pub fn set(&mut self, pos: Square, kind: Kind) {
        let (promote, i) = Self::ids(kind);

        if promote {
            self.promote.set(pos);
        }
        if (i & 1) != 0 {
            self.kind0.set(pos);
        }
        if (i & 2) != 0 {
            self.kind1.set(pos);
        }
        if (i & 4) != 0 {
            self.kind2.set(pos);
        }
    }
    // #[inline(never)]
    pub fn unset(&mut self, pos: Square, kind: Kind) {
        let (promote, i) = Self::ids(kind);

        if promote {
            self.promote.unset(pos);
        }
        if (i & 1) != 0 {
            self.kind0.unset(pos);
        }
        if (i & 2) != 0 {
            self.kind1.unset(pos);
        }
        if (i & 4) != 0 {
            self.kind2.unset(pos);
        }
    }

    pub(crate) fn shift(&mut self, dir: crate::direction::Direction) {
        self.promote.shift(dir);
        self.kind0.shift(dir);
        self.kind1.shift(dir);
        self.kind2.shift(dir);
    }

    // #[inline(never)]
    pub fn get(&self, pos: Square) -> Option<Kind> {
        let mut i = 0;
        if self.kind0.get(pos) {
            i |= 1;
        }
        if self.kind1.get(pos) {
            i |= 2;
        }
        if self.kind2.get(pos) {
            i |= 4;
        }
        if i == 0 {
            return None;
        }
        if self.promote.get(pos) {
            i |= 8;
        }
        Some(KINDS[i])
    }

    pub fn occupied(&self) -> BitBoard {
        self.kind0 | self.kind1 | self.kind2
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::Kind,
        position::{bitboard::KindBitBoard, Square},
    };

    #[test]
    fn get_set() {
        let mut b = KindBitBoard::default();
        let pos = Square::from_index(0);
        assert_eq!(None, b.get(pos));
        b.set(pos, Kind::Pawn);
        assert_eq!(Some(Kind::Pawn), b.get(pos));
        b.unset(pos, Kind::Pawn);
        assert_eq!(None, b.get(pos));
        b.set(pos, Kind::Knight);
        assert_eq!(Some(Kind::Knight), b.get(pos));
    }
}
