use serde::Serialize;

use crate::piece::Kind;

use super::{bitboard_pair::BitBoardPair, BitBoard, Square};

#[derive(Clone, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize)]
pub struct KindBitBoard {
    promote_kind0: BitBoardPair,
    kind1_kind2: BitBoardPair,
}

impl KindBitBoard {
    pub fn empty() -> Self {
        Self {
            promote_kind0: BitBoardPair::empty(),
            kind1_kind2: BitBoardPair::empty(),
        }
    }

    #[inline(always)]
    pub fn bitboard(&self, kind: Kind, occupied: BitBoard) -> BitBoard {
        let mut mask = occupied;
        let i = if let Some(raw) = kind.unpromote() {
            mask &= self.promote_kind0.get0();
            raw.index()
        } else {
            mask = mask.and_not(self.promote_kind0.get0());
            kind.index()
        };

        if (i & 1) > 0 {
            mask &= self.promote_kind0.get1();
        } else {
            mask = mask.and_not(self.promote_kind0.get1());
        }
        if (i >> 1 & 1) > 0 {
            mask &= self.kind1_kind2.get0();
        } else {
            mask = mask.and_not(self.kind1_kind2.get0());
        }
        if (i >> 2 & 1) > 0 {
            mask &= self.kind1_kind2.get1();
        } else {
            mask = mask.and_not(self.kind1_kind2.get1());
        }
        mask
    }
    pub fn get(&self, pos: Square) -> Kind {
        let mut i = 0;
        if self.promote_kind0.get1().get(pos) {
            i |= 1
        };
        if self.kind1_kind2.get0().get(pos) {
            i |= 2
        };
        if self.kind1_kind2.get1().get(pos) {
            i |= 4
        };
        let kind = Kind::from_index(i);

        if self.promote_kind0.get0().get(pos) {
            kind.promote().unwrap()
        } else {
            kind
        }
    }
    pub fn set(&mut self, pos: Square, kind: Kind) {
        let i = if let Some(raw) = kind.unpromote() {
            self.promote_kind0.set0(pos);
            raw.index()
        } else {
            kind.index()
        };
        if (i & 1) > 0 {
            self.promote_kind0.set1(pos);
        }
        if (i >> 1 & 1) > 0 {
            self.kind1_kind2.set0(pos);
        }
        if (i >> 2 & 1) > 0 {
            self.kind1_kind2.set1(pos);
        }
    }
    pub fn unset(&mut self, pos: Square, kind: Kind) {
        let i = if let Some(raw) = kind.unpromote() {
            self.promote_kind0.unset0(pos);
            raw.index()
        } else {
            kind.index()
        };
        if (i & 1) > 0 {
            self.promote_kind0.unset1(pos);
        }
        if (i >> 1 & 1) > 0 {
            self.kind1_kind2.unset0(pos);
        }
        if (i >> 2 & 1) > 0 {
            self.kind1_kind2.unset1(pos);
        }
    }
}
