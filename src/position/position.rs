use crate::piece::*;

#[derive(Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct Position {
    color_bb: [BitBoard; 2],
    promote_bb: BitBoard,
    kind_bb: [BitBoard; 3],
    hands: Hands,
    turn: Color,
    pawn_drop: bool,
}

pub type Digest = u64;

#[test]
fn test_position_size() {
    assert_eq!(112, std::mem::size_of::<Position>());
}

use crate::sfen;
use std::fmt;
impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", sfen::encode_position(self))
    }
}

use super::bitboard::BitBoard;
use super::hands::Hands;
use super::Square;

impl Position {
    pub fn new() -> Self {
        Self {
            kind_bb: [BitBoard::new(); 3],
            promote_bb: BitBoard::new(),
            color_bb: [BitBoard::new(); 2],
            hands: Hands::new(),
            turn: Black,
            pawn_drop: false,
        }
    }
    pub fn turn(&self) -> Color {
        self.turn
    }
    pub fn set_turn(&mut self, c: Color) {
        self.turn = c;
    }
    pub fn hands(&self) -> &Hands {
        &self.hands
    }
    pub fn hands_mut(&mut self) -> &mut Hands {
        &mut self.hands
    }
    pub fn pawn_drop(&self) -> bool {
        self.pawn_drop
    }
    pub(super) fn set_pawn_drop(&mut self, x: bool) {
        self.pawn_drop = x;
    }
    pub(super) fn bitboard(&self, color: Option<Color>, kind: Option<Kind>) -> BitBoard {
        let mut mask = if let Some(c) = color {
            self.color_bb[c.index()]
        } else {
            self.color_bb[0] | self.color_bb[1]
        };

        let k = if let Some(k) = kind { k } else { return mask };
        let i = if let Some(raw) = k.unpromote() {
            mask &= self.promote_bb;
            raw.index()
        } else {
            mask = mask.and_not(self.promote_bb);
            k.index()
        };
        for j in 0..3 {
            if (i >> j & 1) > 0 {
                mask &= self.kind_bb[j];
            } else {
                mask = mask.and_not(self.kind_bb[j]);
            }
        }
        mask
    }
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        let color = if self.bitboard(Some(Color::Black), None).get(pos) {
            Color::Black
        } else if self.bitboard(Some(Color::White), None).get(pos) {
            Color::White
        } else {
            return None;
        };
        let mut k = 0;
        for i in 0..3 {
            if self.kind_bb[i].get(pos) {
                k |= 1 << i;
            }
        }
        let kind = Kind::from_index(k);
        if self.promote_bb.get(pos) {
            Some((color, kind.promote().unwrap()))
        } else {
            Some((color, kind))
        }
    }
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(!self.color_bb[c.index()].get(pos));

        self.color_bb[c.index()].set(pos);
        let i = if let Some(raw) = k.unpromote() {
            self.promote_bb.set(pos);
            raw.index()
        } else {
            k.index()
        };
        for j in 0..3 {
            if (i >> j & 1) > 0 {
                self.kind_bb[j].set(pos);
            }
        }
    }
    pub fn digest(&self) -> Digest {
        let mut res = 0u128;
        res = res.wrapping_mul(127) + self.color_bb[0].x;
        res = res.wrapping_mul(127) + self.color_bb[1].x;
        res = res.wrapping_mul(127) + self.promote_bb.x;
        res = res.wrapping_mul(127) + self.kind_bb[0].x;
        res = res.wrapping_mul(127) + self.kind_bb[1].x;
        res = res.wrapping_mul(127) + self.kind_bb[2].x;
        (res >> 64) as Digest
            ^ res as Digest
            ^ self.hands.x << 1
            ^ if self.pawn_drop { 1 } else { 0 }
    }
    pub(super) fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(self.color_bb[c.index()].get(pos));

        self.color_bb[c.index()].unset(pos);
        let i = if let Some(raw) = k.unpromote() {
            self.promote_bb.unset(pos);
            raw.index()
        } else {
            k.index()
        };
        for j in 0..3 {
            if (i >> j & 1) > 0 {
                self.kind_bb[j].unset(pos);
            }
        }
    }
}
