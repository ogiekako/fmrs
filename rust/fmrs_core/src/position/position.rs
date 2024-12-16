use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Default, Hash)]
pub struct Position {
    black_bb: BitBoard,    // 16 bytes
    kind_bb: KindBitBoard, // 64 bytes
    hands: Hands,          // 8 bytes
    _padding: u64,         // 8 bytes
}

pub type Digest = u64;

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
    pub fn set_pawn_drop(&mut self, x: bool) {
        self.hands.set_pawn_drop(x)
    }

    pub fn black(&self) -> BitBoard {
        self.black_bb
    }

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
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), None);

        if c.is_black() {
            self.black_bb.set(pos);
        }
        self.kind_bb.set(pos, k);
    }
    pub fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), Some((c, k)));

        if c.is_black() {
            self.black_bb.unset(pos);
        }
        self.kind_bb.unset(pos, k);
    }

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn shift(&mut self, dir: Direction) {
        self.black_bb.shift(dir);
        self.kind_bb.shift(dir);
    }

    pub fn sfen(&self) -> String {
        sfen::encode_position(self)
    }

    pub fn sfen_url(&self) -> String {
        sfen::sfen_to_image_url(&self.sfen())
    }

    pub fn color_bb(&self) -> ColorBitBoard {
        let occupied = self.kind_bb.occupied();
        let black = self.black();
        let white = occupied.and_not(black);
        ColorBitBoard::new(black, white, occupied)
    }

    pub fn digest(&self) -> Digest {
        xxhash_rust::xxh3::xxh3_64(self.as_bytes())
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self as *const Position as *const u8,
                std::mem::size_of::<Position>(),
            )
        }
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

    #[test]
    fn digest_no_collision() {
        for (pos1, pos2) in [
            (
                "9/9/5B3/4kb3/9/5+R3/9/9/2+R6 b 4g4s4n4l18p",
                "9/9/5B3/4kb3/9/5+R3/2R6/9/9 b 4g4s4n4l18p",
            ),
            (
                "9/9/5B3/4kb3/2R6/5+R3/9/9/9 b 4g4s4n4l18p",
                "9/9/5B3/4kb3/9/5+R3/2R6/9/9 b 4g4s4n4l18p",
            ),
        ] {
            let mut pos1 = Position::from_sfen(pos1).unwrap();
            let mut pos2 = Position::from_sfen(pos2).unwrap();

            for pawn_drop1 in [true, false] {
                for pawn_drop2 in [true, false] {
                    pos1.set_pawn_drop(pawn_drop1);
                    pos2.set_pawn_drop(pawn_drop2);

                    assert_ne!(pos1, pos2);
                    assert_ne!(pos1.digest(), pos2.digest(), "{:?} {:?}", pos1, pos2);

                    pretty_assertions::assert_eq!(Position::from_sfen(&pos1.sfen()).unwrap(), pos1);
                    pretty_assertions::assert_eq!(Position::from_sfen(&pos2.sfen()).unwrap(), pos2);
                }
            }
        }
    }

    #[test]
    fn position_size() {
        assert_eq!(std::mem::size_of::<Position>(), 96);
    }
}
