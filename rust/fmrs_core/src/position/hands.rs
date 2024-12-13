use serde::Serialize;

use crate::piece::{Color, Kind, Kinds, KINDS, NUM_HAND_KIND};

// Hands represents hands of both side.
// The number of pawns should be less than 256. (8 bits)
// The number of other kinds should be less than 16. (4 bits)
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, Ord, PartialOrd, Serialize)]
pub struct Hands {
    // 0-6  : black pawn, 7-10 : black lance, ..., 27-30: black rook
    // 32-38: white pawn, 39-42: white lance, ..., 59-62: white rook
    // 31: turn (0: black, 1: white)
    // 63: pawn_drop
    pub(super) x: u64,
}

#[test]
fn test_hands_size() {
    assert_eq!(8, std::mem::size_of::<Hands>());
}

impl Default for Hands {
    fn default() -> Self {
        Self::new()
    }
}

const BLACK_MASK: u64 = 0x7FFF_FFFF;
const WHITE_MASK: u64 = 0x7FFF_FFFF << 32;
const TURN_FLAG: u64 = 1 << 31;
const PAWN_DROP_FLAG: u64 = 1 << 63;

const SHIFTS: [u64; 2 * 8] = [
    0, 7, 11, 15, 19, 23, 27, /* dummy */ 31, 32, 39, 43, 47, 51, 55, 59, /* dummy */ 63,
];
const AREA: [u64; 2 * 8] = {
    let mut res = [0; 2 * 8];
    let mut i = 0;
    while i < 7 {
        res[i] = (1 << SHIFTS[i + 1]) - (1 << SHIFTS[i]);
        res[i + 8] = (1 << SHIFTS[i + 9] - 1) - (1 << SHIFTS[i + 8]);
        i += 1;
    }
    res
};

impl Hands {
    pub fn new() -> Hands {
        Hands { x: 0 }
    }

    fn max_count(k: Kind) -> usize {
        debug_assert!(k.is_hand_piece(), "{k:?}");
        if k == Kind::Pawn {
            127
        } else {
            15
        }
    }
    pub fn count(&self, c: Color, k: Kind) -> usize {
        debug_assert!(k.is_hand_piece());
        (self.x >> Hands::shift_of(c, k)) as usize & Hands::max_count(k)
    }
    pub fn add(&mut self, c: Color, k: Kind) {
        debug_assert!(self.count(c, k) <= Hands::max_count(k));
        self.x += Hands::bit_of(c, k);
    }
    pub fn remove(&mut self, c: Color, k: Kind) {
        debug_assert!(self.count(c, k) > 0);
        self.x -= Hands::bit_of(c, k);
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
        self.x & Self::area_of(c, k) != 0
    }

    fn shift_of(c: Color, k: Kind) -> u64 {
        SHIFTS[c.index() << 3 | k.index()]
    }

    fn area_of(c: Color, k: Kind) -> u64 {
        AREA[c.index() << 3 | k.index()]
    }

    fn bit_of(c: Color, k: Kind) -> u64 {
        1 << Hands::shift_of(c, k)
    }

    pub fn set_turn(&mut self, c: Color) {
        if c.is_white() {
            self.x |= TURN_FLAG;
        } else {
            self.x &= !TURN_FLAG;
        }
    }
    pub fn turn(&self) -> Color {
        Color::from_is_white(self.x & TURN_FLAG != 0)
    }

    pub fn set_pawn_drop(&mut self, x: bool) {
        if x {
            self.x |= PAWN_DROP_FLAG;
        } else {
            self.x &= !PAWN_DROP_FLAG;
        }
    }

    pub fn pawn_drop(&self) -> bool {
        self.x & PAWN_DROP_FLAG != 0
    }

    pub fn is_empty(&self, c: Color) -> bool {
        if c.is_white() {
            self.x & WHITE_MASK == 0
        } else {
            self.x & BLACK_MASK == 0
        }
    }
}
