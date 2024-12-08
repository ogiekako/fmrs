use crate::piece::{Kind, NUM_KIND};

use super::{BitBoard, Square};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct KindBitBoard {
    bbs: [BitBoard; NUM_KIND],
    kinds: [Option<Kind>; 81],
}

impl Default for KindBitBoard {
    fn default() -> Self {
        Self {
            bbs: Default::default(),
            kinds: [None; 81],
        }
    }
}

impl KindBitBoard {
    pub fn bitboard(&self, kind: Kind) -> &BitBoard {
        &self.bbs[kind.index()]
    }

    pub fn get(&self, pos: Square) -> Kind {
        self.kinds[pos.index()].unwrap()
    }

    pub fn set(&mut self, pos: Square, kind: Kind) {
        self.kinds[pos.index()] = Some(kind);
        self.bbs[kind.index()].set(pos);
    }
    pub fn unset(&mut self, pos: Square, kind: Kind) {
        self.kinds[pos.index()] = None;
        self.bbs[kind.index()].unset(pos);
    }

    pub(crate) fn shift(&mut self, dir: crate::direction::Direction) {
        self.bbs.iter_mut().for_each(|bb| bb.shift(dir));

        let mut new_kinds = [None; 81];
        for pos in Square::iter() {
            if let Some(kind) = self.kinds[pos.index()] {
                let new_pos = pos.shift(dir);
                new_kinds[new_pos.index()] = Some(kind);
            }
        }
        self.kinds = new_kinds;
    }
}
