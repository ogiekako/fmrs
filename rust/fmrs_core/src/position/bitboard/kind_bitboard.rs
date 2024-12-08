use crate::piece::Kind;

use super::{BitBoard, Square};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct KindBitBoard {
    promote: BitBoard,
    kind0: BitBoard,
    kind1: BitBoard,
    kind2: BitBoard,
}

#[test]
fn test_kind_bitboard_size() {
    assert_eq!(64, std::mem::size_of::<KindBitBoard>());
}

impl KindBitBoard {
    #[inline(always)]
    pub fn bitboard(&self, kind: Kind, occupied: BitBoard) -> BitBoard {
        let mut mask = occupied;
        let i = if let Some(raw) = kind.unpromote() {
            mask &= self.promote;
            raw.index()
        } else {
            mask = mask.and_not(self.promote);
            kind.index()
        };

        if (i & 1) > 0 {
            mask &= self.kind0;
        } else {
            mask = mask.and_not(self.kind0);
        }
        if (i >> 1 & 1) > 0 {
            mask &= self.kind1;
        } else {
            mask = mask.and_not(self.kind1);
        }
        if (i >> 2 & 1) > 0 {
            mask &= self.kind2;
        } else {
            mask = mask.and_not(self.kind2);
        }
        mask
    }
    pub fn get(&self, pos: Square) -> Kind {
        let mut i = 0;
        if self.kind0.get(pos) {
            i |= 1
        };
        if self.kind1.get(pos) {
            i |= 2
        };
        if self.kind2.get(pos) {
            i |= 4
        };
        let kind = Kind::from_index(i);

        if self.promote.get(pos) {
            kind.promote().unwrap()
        } else {
            kind
        }
    }
    pub fn set(&mut self, pos: Square, kind: Kind) {
        let i = if let Some(raw) = kind.unpromote() {
            self.promote.set(pos);
            raw.index()
        } else {
            kind.index()
        };
        if (i & 1) > 0 {
            self.kind0.set(pos);
        }
        if (i >> 1 & 1) > 0 {
            self.kind1.set(pos);
        }
        if (i >> 2 & 1) > 0 {
            self.kind2.set(pos);
        }
    }
    pub fn unset(&mut self, pos: Square, kind: Kind) {
        let i = if let Some(raw) = kind.unpromote() {
            self.promote.unset(pos);
            raw.index()
        } else {
            kind.index()
        };
        if (i & 1) > 0 {
            self.kind0.unset(pos);
        }
        if (i >> 1 & 1) > 0 {
            self.kind1.unset(pos);
        }
        if (i >> 2 & 1) > 0 {
            self.kind2.unset(pos);
        }
    }

    pub(crate) fn shift(&mut self, dir: crate::direction::Direction) {
        self.promote.shift(dir);
        self.kind0.shift(dir);
        self.kind1.shift(dir);
        self.kind2.shift(dir);
    }
}
