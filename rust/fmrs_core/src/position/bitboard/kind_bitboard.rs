use crate::piece::Kind;

use super::{BitBoard, Square};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct KindBitBoard {
    promote: BitBoard,
    kind0: BitBoard,
    kind1: BitBoard,
    kind2: BitBoard,
    /// Per-square 4-bit kind cache packed 2 squares per byte.
    /// 81 squares × 4 bits = 324 bits → 41 bytes (rounded to 48 with alignment).
    /// Encoding: 0 = empty, 1..=15 = same as KINDS array index.
    /// Replaces 4 sequential BitBoard::contains() calls in must_get/get with
    /// one byte load + shift + mask. Maintained on set/unset.
    square_kinds_packed: [u8; 41],
}

impl Default for KindBitBoard {
    fn default() -> Self {
        Self {
            promote: BitBoard::default(),
            kind0: BitBoard::default(),
            kind1: BitBoard::default(),
            kind2: BitBoard::default(),
            square_kinds_packed: [0u8; 41],
        }
    }
}

impl KindBitBoard {
    pub(crate) fn raw_parts(&self) -> (BitBoard, BitBoard, BitBoard, BitBoard) {
        (self.promote, self.kind0, self.kind1, self.kind2)
    }

    pub(crate) fn from_raw_parts(
        promote: BitBoard,
        kind0: BitBoard,
        kind1: BitBoard,
        kind2: BitBoard,
    ) -> Self {
        let mut result = Self {
            promote,
            kind0,
            kind1,
            kind2,
            square_kinds_packed: [0u8; 41],
        };
        for (kind_idx, &kind) in KINDS.iter().enumerate() {
            if kind_idx == 0 || kind_idx == 8 {
                continue;
            }
            for pos in result.bitboard(kind) {
                result.write_kind_idx(pos.index(), kind_idx as u8);
            }
        }
        result
    }

    #[inline(always)]
    fn read_kind_idx(&self, pos_idx: usize) -> usize {
        // Layout: byte[i] holds squares (2i, 2i+1) in (low, high) nibble.
        let byte = unsafe { *self.square_kinds_packed.get_unchecked(pos_idx >> 1) };
        ((byte >> ((pos_idx & 1) * 4)) & 0xF) as usize
    }

    #[inline(always)]
    fn write_kind_idx(&mut self, pos_idx: usize, idx: u8) {
        debug_assert!(idx <= 0xF);
        let byte_ref = unsafe { self.square_kinds_packed.get_unchecked_mut(pos_idx >> 1) };
        let shift = (pos_idx & 1) * 4;
        *byte_ref = (*byte_ref & !(0xF << shift)) | (idx << shift);
    }
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
    // 4× BitBoard (4×16=64) + [u8; 41] packed cache padded to 16-byte alignment.
    // 64+41=105 → padded to 112.
    assert_eq!(112, std::mem::size_of::<KindBitBoard>());
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

    #[inline(always)]
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
    #[inline(always)]
    pub fn must_get(&self, pos: Square) -> Kind {
        let i = self.read_kind_idx(pos.index());
        debug_assert_ne!(i, 0);
        KINDS[i]
    }
    // #[inline(never)]
    #[inline(always)]
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
        let encoded = (i | if promote { 8 } else { 0 }) as u8;
        self.write_kind_idx(pos.index(), encoded);
    }
    // #[inline(never)]
    #[inline(always)]
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
        self.write_kind_idx(pos.index(), 0);
    }

    /// Replace `old` kind at `pos` with `new` kind. Optimized to touch only the
    /// layer bitboards whose bit actually flips between the two encodings — for
    /// e.g. ProSilver→ProPawn this is a single bitboard update plus the packed
    /// write, vs. the 6 layer ops of unset + set.
    #[inline(always)]
    pub fn change_kind(&mut self, pos: Square, old: Kind, new: Kind) {
        let (old_p, old_i) = Self::ids(old);
        let (new_p, new_i) = Self::ids(new);
        let i_diff = old_i ^ new_i;
        if (i_diff & 1) != 0 {
            if (new_i & 1) != 0 {
                self.kind0.set(pos);
            } else {
                self.kind0.unset(pos);
            }
        }
        if (i_diff & 2) != 0 {
            if (new_i & 2) != 0 {
                self.kind1.set(pos);
            } else {
                self.kind1.unset(pos);
            }
        }
        if (i_diff & 4) != 0 {
            if (new_i & 4) != 0 {
                self.kind2.set(pos);
            } else {
                self.kind2.unset(pos);
            }
        }
        if old_p != new_p {
            if new_p {
                self.promote.set(pos);
            } else {
                self.promote.unset(pos);
            }
        }
        let encoded = (new_i | if new_p { 8 } else { 0 }) as u8;
        self.write_kind_idx(pos.index(), encoded);
    }

    /// Move a piece from `src` to `dst` without changing kind. Faster than
    /// `unset(src) + set(dst)` because each layer bitboard sees a single XOR
    /// that flips both bits simultaneously, and only one combined zobrist XOR
    /// is needed at the call site.
    #[inline(always)]
    pub fn move_piece(&mut self, src: Square, dst: Square, kind: Kind) {
        let (promote, i) = Self::ids(kind);
        let mask = (1u128 << src.index()) | (1u128 << dst.index());
        if promote {
            self.promote.toggle_mask(mask);
        }
        if (i & 1) != 0 {
            self.kind0.toggle_mask(mask);
        }
        if (i & 2) != 0 {
            self.kind1.toggle_mask(mask);
        }
        if (i & 4) != 0 {
            self.kind2.toggle_mask(mask);
        }
        let encoded = (i | if promote { 8 } else { 0 }) as u8;
        self.write_kind_idx(src.index(), 0);
        self.write_kind_idx(dst.index(), encoded);
    }

    pub(crate) fn shift(&mut self, dir: crate::direction::Direction) {
        self.promote.shift(dir);
        self.kind0.shift(dir);
        self.kind1.shift(dir);
        self.kind2.shift(dir);
        // Rebuild square_kinds_packed after shifting bitboards; per-square direct
        // shift would require translating each square's index, which is more
        // expensive than the rare shift call.
        self.square_kinds_packed = [0u8; 41];
        for (kind_idx, &kind) in KINDS.iter().enumerate() {
            if kind_idx == 0 || kind_idx == 8 {
                continue; // dummies
            }
            for pos in self.bitboard(kind) {
                self.write_kind_idx(pos.index(), kind_idx as u8);
            }
        }
    }

    // #[inline(never)]
    #[inline(always)]
    pub fn get(&self, pos: Square) -> Option<Kind> {
        let i = self.read_kind_idx(pos.index());
        if i == 0 {
            None
        } else {
            Some(KINDS[i])
        }
    }

    pub fn occupied(&self) -> BitBoard {
        self.kind0 | self.kind1 | self.kind2
    }

    /// Promote-layer bitboard: set bit for any "promoted" kind (or King).
    /// Useful for splitting `bishopish() / rookish()` into raw vs. promoted
    /// without two separate `bitboard(kind)` lookups.
    #[inline(always)]
    pub fn promote_layer(&self) -> BitBoard {
        self.promote
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
