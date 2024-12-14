use crate::piece::Kind;

use super::Square;

#[derive(Debug, Clone, Default, Eq, PartialEq, PartialOrd, Ord)]
pub(super) struct PlacedKinds {
    mask32: u128,
    mask64: u128,
    mask96: u128,
}

impl PlacedKinds {
    pub fn set(&mut self, pos: Square, kind: Kind) {
        let i = (pos.index() & 31) * 4;
        if pos.index() < 32 {
            self.mask32 |= (kind.index() as u128) << i;
        } else if pos.index() < 64 {
            self.mask64 |= (kind.index() as u128) << i;
        } else {
            self.mask96 |= (kind.index() as u128) << i;
        }
    }

    pub fn unset(&mut self, pos: Square) {
        let i = (pos.index() & 31) * 4;
        if pos.index() < 32 {
            self.mask32 &= !(15 << i);
        } else if pos.index() < 64 {
            self.mask64 &= !(15 << i);
        } else {
            self.mask96 &= !(15 << i);
        }
    }

    pub fn get(&self, pos: Square) -> Kind {
        let i = (pos.index() & 31) * 4;
        if pos.index() < 32 {
            Kind::from_index(((self.mask32 >> i) & 15) as usize)
        } else if pos.index() < 64 {
            Kind::from_index(((self.mask64 >> i) & 15) as usize)
        } else {
            Kind::from_index(((self.mask96 >> i) & 15) as usize)
        }
    }

    pub(crate) fn clear(&mut self) {
        self.mask32 = 0;
        self.mask64 = 0;
        self.mask96 = 0;
    }
}
