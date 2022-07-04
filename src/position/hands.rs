use crate::piece::{Color, Kind, KINDS, NUM_HAND_KIND};

// Hands represents hands of both side.
// The number of pawns should be less than 256. (8 bits)
// The number of other kinds should be less than 16. (4 bits)
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct Hands {
    // 0-7  : black pawn, 8-11 : black lance, ..., 28-31: black rook
    // 32-39: white pawn, 40-43: white lance, ..., 60-63: white rook
    x: u64,
}

impl Hands {
    pub fn new() -> Hands {
        Hands { x: 0 }
    }

    fn max_count(k: Kind) -> usize {
        debug_assert!(k.is_hand_piece());
        if k == Kind::Pawn {
            255
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
            k.index() * 4 + 4
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
}
