use serde::Serialize;

use crate::piece::*;

#[derive(Clone, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Position {
    color_bb: ColorBitBoard, // 24 bytes
    kind_bb: KindBitBoard,   // 48 bytes
    hands: Hands,            // 8 bytes
    turn: Color,
    pawn_drop: bool,
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

pub type Digest = u64;

#[test]
fn test_position_size() {
    assert_eq!(88, std::mem::size_of::<Position>());
}

use crate::sfen;
use std::fmt;
use std::hash::Hash;
impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", sfen::encode_position(self))
    }
}

use super::bitboard::BitBoard;
use super::bitboard::ColorBitBoard;
use super::bitboard::KindBitBoard;
use super::hands::Hands;
use super::Square;

impl Position {
    pub fn new() -> Self {
        Self {
            color_bb: ColorBitBoard::empty(),
            kind_bb: KindBitBoard::empty(),
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
        let mask = if let Some(c) = color {
            self.color_bb.bitboard(c)
        } else {
            self.color_bb.both()
        };

        let k = if let Some(k) = kind { k } else { return mask };
        self.kind_bb.bitboard(k, mask)
    }
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        let color = if self.bitboard(Some(Color::Black), None).get(pos) {
            Color::Black
        } else if self.bitboard(Some(Color::White), None).get(pos) {
            Color::White
        } else {
            return None;
        };
        Some((color, self.kind_bb.get(pos)))
    }
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(!self.color_bb.bitboard(c).get(pos));

        self.color_bb.set(c, pos);
        self.kind_bb.set(pos, k);
    }
    pub fn digest(&self) -> Digest {
        xxhash_rust::xxh3::xxh3_64(&self.bytes())
    }
    pub(super) fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(self.color_bb.bitboard(c).get(pos));

        self.color_bb.unset(c, pos);
        self.kind_bb.unset(pos, k);
    }

    fn bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}
