use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Position {
    color_bb: ColorBitBoard, // 32 bytes
    kind_bb: KindBitBoard,   // 64 bytes
    hands: Hands,            // 8 bytes
    board_digest: u64,       // 8 bytes
    __heavy: [u128; 0],
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

pub type Digest = u64;

use crate::position::zobrist::zobrist;
use crate::sfen;
use std::fmt;

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
            color_bb: ColorBitBoard::default(),
            kind_bb: KindBitBoard::default(),
            hands: Hands::new(),
            board_digest: 0,
            __heavy: Default::default(),
        }
    }
    pub fn turn(&self) -> Color {
        self.hands.turn()
    }
    pub fn set_turn(&mut self, c: Color) {
        self.hands.set_turn(c);
    }
    pub fn hands(&self) -> &Hands {
        &self.hands
    }
    pub fn hands_mut(&mut self) -> &mut Hands {
        &mut self.hands
    }
    pub fn pawn_drop(&self) -> bool {
        self.hands.pawn_drop()
    }
    pub(super) fn set_pawn_drop(&mut self, x: bool) {
        self.hands.set_pawn_drop(x)
    }

    pub fn color_bb(&self) -> &ColorBitBoard {
        &self.color_bb
    }

    /// Returns a bitboard of pieces of the specified color and kind.
    pub fn bitboard(&self, color: Option<Color>, kind: Option<Kind>) -> BitBoard {
        let mask = if let Some(c) = color {
            self.color_bb.bitboard(c)
        } else {
            self.color_bb.both()
        };

        let k = if let Some(k) = kind { k } else { return *mask };
        self.kind_bb.bitboard(k) & mask
    }

    pub fn bitboard_essential_kind(&self, color: Option<Color>, ek: EssentialKind) -> BitBoard {
        if let Some(k) = ek.unique_kind() {
            self.bitboard(color, Some(k))
        } else {
            let mask = self.kind_bb.golds();
            match color {
                Some(c) => mask & self.color_bb.bitboard(c),
                None => *mask,
            }
        }
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

        let i = c.index() | k.index() << 1;
        self.board_digest ^= zobrist(pos).wrapping_shl(i as u32);
    }
    pub fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(self.color_bb.bitboard(c).get(pos));

        self.color_bb.unset(c, pos);
        self.kind_bb.unset(pos, k);

        let i = c.index() | k.index() << 1;
        self.board_digest ^= zobrist(pos).wrapping_shl(i as u32);
    }

    pub fn digest(&self) -> Digest {
        self.board_digest ^ self.hands.x
    }

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn shift(&mut self, dir: Direction) {
        self.color_bb.shift(dir);
        self.kind_bb.shift(dir);

        self.board_digest = 0;
        for pos in Square::iter() {
            if let Some((c, k)) = self.get(pos) {
                let i = c.index() | k.index() << 1;
                self.board_digest ^= zobrist(pos).wrapping_shl(i as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{direction::Direction, position::Square};

    use super::{Color, Kind, Position};

    #[test]
    fn test_shift() {
        let mut position = Position::new();
        position.set(Square::new(0, 0), Color::Black, Kind::Pawn);
        position.shift(Direction::Down);

        assert_eq!(position.digest(), {
            let mut position = Position::new();
            position.set(Square::new(0, 1), Color::Black, Kind::Pawn);
            position.digest()
        });
    }
}
