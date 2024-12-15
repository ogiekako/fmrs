use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct Position {
    black_bb: BitBoard,    // 16 bytes
    kind_bb: KindBitBoard, // 64 bytes
    hands: Hands,          // 8 bytes
    board_digest: u64,     // 8 bytes
    black_knight_reach: BitBoard,
}

pub type Digest = u64;

use crate::position::bitboard::knight_power;
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

    pub fn black(&self) -> BitBoard {
        self.black_bb
    }

    // pub fn white(&self) -> BitBoard {
    //     self.occupied().and_not(self.black())
    // }

    pub fn occupied(&self) -> BitBoard {
        self.kind_bb.occupied()
    }

    // pub fn color_bb(&self, color: Color) -> BitBoard {
    //     if color.is_black() {
    //         self.black()
    //     } else {
    //         self.white()
    //     }
    // }

    /// Returns a bitboard of pieces of the specified color and kind.
    pub fn bitboard(&self, color: Color, kind: Kind) -> BitBoard {
        if color.is_black() {
            self.kind_bb.bitboard(kind) & self.black_bb
        } else {
            self.kind_bb.bitboard(kind).and_not(self.black_bb)
        }
    }
    pub fn kind_bb(&self) -> &KindBitBoard {
        &self.kind_bb
    }
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        let Some(kind) = self.kind_bb.get(pos) else {
            return None;
        };
        Some(if self.black().get(pos) {
            (Color::BLACK, kind)
        } else {
            (Color::WHITE, kind)
        })
    }
    pub fn get_kind(&self, pos: Square) -> Option<Kind> {
        self.has(pos).then(|| self.must_get_kind(pos))
    }
    pub fn has(&self, pos: Square) -> bool {
        self.occupied().get(pos)
    }
    pub fn must_get_kind(&self, pos: Square) -> Kind {
        self.kind_bb().must_get(pos)
    }
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), None);

        if c.is_black() {
            self.black_bb.set(pos);
        }
        self.kind_bb.set(pos, k);

        if k == Kind::Knight && c == Color::BLACK {
            self.black_knight_reach |= knight_power(c, pos);
        }

        self.board_digest ^= zobrist(c, pos, k);
    }
    pub fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), Some((c, k)));

        if c.is_black() {
            self.black_bb.unset(pos);
        }
        self.kind_bb.unset(pos, k);

        if k == Kind::Knight && c == Color::BLACK {
            let (col, row) = pos.col_row();
            if col >= 1 && row >= 2 {
                if col == 1
                    || self.get(Square::new(col - 2, row)) != Some((Color::BLACK, Kind::Knight))
                {
                    debug_assert!(self.black_knight_reach.get(Square::new(col - 1, row - 2)));
                    self.black_knight_reach.unset(Square::new(col - 1, row - 2));
                }
            }
            if col <= 7 && row >= 2 {
                if col == 7
                    || self.get(Square::new(col + 2, row)) != Some((Color::BLACK, Kind::Knight))
                {
                    debug_assert!(self.black_knight_reach.get(Square::new(col + 1, row - 2)));
                    self.black_knight_reach.unset(Square::new(col + 1, row - 2));
                }
            }
        }

        self.board_digest ^= zobrist(c, pos, k);
    }

    pub fn digest(&self) -> Digest {
        self.board_digest ^ self.hands.x
    }

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn shift(&mut self, dir: Direction) {
        self.black_bb.shift(dir);
        self.kind_bb.shift(dir);

        self.board_digest = 0;
        self.black_knight_reach.clear();

        let color_bb = self.color();
        for c in Color::iter() {
            for pos in color_bb.bitboard(c) {
                let k = self.must_get_kind(pos);
                self.board_digest ^= zobrist(c, pos, k);

                if c == Color::BLACK && k == Kind::Knight {
                    self.black_knight_reach |= knight_power(c, pos);
                }
            }
        }
    }

    pub fn sfen(&self) -> String {
        sfen::encode_position(self)
    }

    pub fn sfen_url(&self) -> String {
        sfen::sfen_to_image_url(&self.sfen())
    }

    pub(crate) fn black_knight_reach(&self) -> BitBoard {
        self.black_knight_reach
    }

    pub fn color(&self) -> ColorBitBoard {
        let occupied = self.occupied();
        let black = self.black();
        let white = occupied.and_not(black);
        ColorBitBoard::new(black, white, occupied)
    }
}

#[cfg(test)]
mod tests {
    use crate::{direction::Direction, position::Square};

    use super::{Color, Kind, Position};

    #[test]
    fn test_shift() {
        let mut position = Position::default();
        position.set(Square::new(0, 0), Color::BLACK, Kind::Pawn);
        position.shift(Direction::Down);

        assert_eq!(position.digest(), {
            let mut position = Position::default();
            position.set(Square::new(0, 1), Color::BLACK, Kind::Pawn);
            position.digest()
        });
    }
}
