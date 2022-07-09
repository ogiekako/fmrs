use serde::Serialize;

use crate::piece::{Color, Kind, KINDS, NUM_HAND_KIND};

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

impl Default for Hands {
    fn default() -> Self {
        Self::new()
    }
}

impl Hands {
    pub fn new() -> Hands {
        Hands { x: 0 }
    }

    fn max_count(k: Kind) -> usize {
        debug_assert!(k.is_hand_piece());
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
    pub fn kinds(self, c: Color) -> impl Iterator<Item = Kind> {
        KINDS[0..NUM_HAND_KIND]
            .iter()
            .filter_map(move |&k| if self.count(c, k) > 0 { Some(k) } else { None })
    }
    fn shift_of(c: Color, k: Kind) -> usize {
        let i = if k == Kind::Pawn {
            0
        } else {
            k.index() * 4 + 3
        };
        if c == Color::White {
            i + 32
        } else {
            i
        }
    }
    fn bit_of(c: Color, k: Kind) -> u64 {
        1 << (Hands::shift_of(c, k) as u64)
    }
    pub fn set_turn(&mut self, c: Color) {
        if c == Color::Black {
            self.x &= !(1 << 31);
        } else {
            self.x |= 1 << 31;
        }
    }
    pub fn turn(&self) -> Color {
        if self.x >> 31 & 1 > 0 {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn set_pawn_drop(&mut self, x: bool) {
        if x {
            self.x |= 1 << 63;
        } else {
            self.x &= !(1 << 63);
        }
    }
    pub fn pawn_drop(&self) -> bool {
        self.x >> 63 & 1 > 0
    }
}
