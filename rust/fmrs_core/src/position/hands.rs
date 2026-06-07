use std::fmt::Debug;

use crate::piece::{Color, Kind, Kinds, KINDS, NUM_HAND_KIND};

// Hands holds both sides' hands as two independent 64-bit words, `h[0]` for black
// and `h[1]` for white, indexed by `Color::index()`. Keeping the colors in
// separate words means every per-color read/write is a plain u64 shift/add (no
// 128-bit shift to extract a half). Per-color field widths (bits):
//   pawn 7 (<128), lance/knight/silver/gold 5 (<32), bishop/rook 4 (<16)
// laid out at the same offsets in both words (35 bits used). turn / pawn_drop
// live in the spare high bits of the black word.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Ord, PartialOrd, Default, Hash)]
pub struct Hands {
    // h[0]: black (pawn @0, lance @7, knight @12, silver @17, gold @22,
    //              bishop @27, rook @31; turn @63, pawn_drop @62)
    // h[1]: white (same field offsets)
    pub(crate) h: [u64; 2],
}

#[test]
fn test_hands_size() {
    assert_eq!(16, std::mem::size_of::<Hands>());
}

// Field width in bits per hand kind (index = Kind hand index: P,L,N,S,G,B,R).
const WIDTHS: [u32; 7] = [7, 5, 5, 5, 5, 4, 4];

// turn / pawn_drop occupy the black word's spare high bits (fields use 0..=34).
pub(crate) const TURN_FLAG: u64 = 1 << 63;
pub(crate) const PAWN_DROP_FLAG: u64 = 1 << 62;

// Per-kind shift within a color word (slot 7 unused).
const SHIFTS: [u32; 8] = {
    let mut res = [0u32; 8];
    let mut acc = 0;
    let mut k = 0;
    while k < 7 {
        res[k] = acc;
        acc += WIDTHS[k];
        k += 1;
    }
    res
};
// Per-kind value mask (slot 7 unused).
const MASKS: [u64; 8] = {
    let mut res = [0u64; 8];
    let mut k = 0;
    while k < 7 {
        res[k] = (1u64 << WIDTHS[k]) - 1;
        k += 1;
    }
    res
};
// All count fields of one word (flags excluded), bits 0..=34.
const FIELDS_MASK: u64 = {
    let mut m = 0u64;
    let mut k = 0;
    while k < 7 {
        m |= MASKS[k] << SHIFTS[k];
        k += 1;
    }
    m
};

impl Hands {
    fn max_count(k: Kind) -> usize {
        debug_assert!(k.is_hand_piece(), "{k:?}");
        MASKS[k.index()] as usize
    }
    pub fn count(&self, c: Color, k: Kind) -> usize {
        if !k.is_hand_piece() {
            return 0;
        }
        ((self.h[c.index()] >> SHIFTS[k.index()]) & MASKS[k.index()]) as usize
    }
    #[inline]
    pub fn add(&mut self, c: Color, k: Kind) {
        debug_assert!(self.count(c, k) < Hands::max_count(k));
        self.h[c.index()] += Hands::bit_of(k);
    }
    #[inline]
    pub fn add_n(&mut self, c: Color, k: Kind, n: usize) {
        debug_assert!(self.count(c, k) + n <= Hands::max_count(k));
        self.h[c.index()] += Hands::bit_of(k) * n as u64;
    }
    #[inline]
    pub fn remove(&mut self, c: Color, k: Kind) {
        debug_assert!(self.count(c, k) > 0);
        self.h[c.index()] -= Hands::bit_of(k);
    }
    #[inline]
    pub fn remove_n(&mut self, c: Color, k: Kind, n: usize) {
        debug_assert!(self.count(c, k) >= n);
        self.h[c.index()] -= Hands::bit_of(k) * n as u64;
    }
    pub fn kinds(self, c: Color) -> Kinds {
        let mut mask = 0;
        for i in 0..NUM_HAND_KIND {
            if self.has(c, KINDS[i]) {
                mask |= 1 << i;
            }
        }
        Kinds::new(mask)
    }
    pub fn has(&self, c: Color, k: Kind) -> bool {
        self.h[c.index()] & Self::area_of(k) != 0
    }

    fn area_of(k: Kind) -> u64 {
        MASKS[k.index()] << SHIFTS[k.index()]
    }
    /// The unit increment for one piece of kind `k` within a color word. Color is
    /// chosen by which word (`h[c.index()]`) the caller adds it to.
    #[inline]
    pub(crate) fn bit_of(k: Kind) -> u64 {
        1 << SHIFTS[k.index()]
    }

    /// Direct mutable access to the white count word, for batched count updates
    /// (e.g. canonicalize folds many `add_n`s into a single add).
    #[inline]
    pub(crate) fn white_word_mut(&mut self) -> &mut u64 {
        &mut self.h[1]
    }

    /// Fold both color words into a 64-bit value for mixing into the Zobrist
    /// digest (which is u64). Black and white use the *same* field offsets, so a
    /// plain `h[0] ^ h[1]` would alias a black piece onto the white piece of the
    /// same kind/count (structured collisions that break digest-keyed dedup).
    /// Multiply the white word by an odd constant to scatter it across all 64
    /// bits; collisions then become birthday-random rather than systematic.
    #[inline(always)]
    pub(crate) fn fold(&self) -> u64 {
        self.h[0] ^ self.h[1].wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    #[inline]
    pub fn set_turn(&mut self, c: Color) {
        if c.is_white() {
            self.h[0] |= TURN_FLAG;
        } else {
            self.h[0] &= !TURN_FLAG;
        }
    }
    /// Flip the turn bit; cheaper than `set_turn(c.opposite())` when the
    /// current turn is already known: a single XOR instead of branch+OR/AND.
    #[inline(always)]
    pub fn toggle_turn(&mut self) {
        self.h[0] ^= TURN_FLAG;
    }
    pub fn turn(&self) -> Color {
        Color::from_is_white(self.h[0] & TURN_FLAG != 0)
    }

    #[inline]
    pub fn set_pawn_drop(&mut self, x: bool) {
        if x {
            self.h[0] |= PAWN_DROP_FLAG;
        } else {
            self.h[0] &= !PAWN_DROP_FLAG;
        }
    }

    pub fn pawn_drop(&self) -> bool {
        self.h[0] & PAWN_DROP_FLAG != 0
    }

    pub fn is_empty(&self, c: Color) -> bool {
        self.h[c.index()] & FIELDS_MASK == 0
    }
}
