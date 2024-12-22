use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
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
        write!(f, "{}", self.sfen_url())
    }
}

use super::advance::attack_prevent::attacker;
use super::bitboard::BitBoard;
use super::bitboard::ColorBitBoard;
use super::bitboard::KindBitBoard;
use super::hands::Hands;
use super::Movement;
use super::PositionExt as _;
use super::Square;

impl Position {
    pub fn turn(&self) -> Color {
        self.hands.turn()
    }
    pub fn set_turn(&mut self, c: Color) {
        self.hands.set_turn(c);
    }
    pub fn hands(&self) -> Hands {
        self.hands
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

    // #[inline(never)]
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

// TOOD: remove clone
#[derive(Debug, Clone)]
pub struct PositionAux {
    core: Position,
    occupied: Option<BitBoard>,
    white_bb: Option<BitBoard>,
    kind_bb: [Option<BitBoard>; NUM_KIND],
}

impl PositionAux {
    pub fn new(core: Position) -> Self {
        Self {
            core,
            occupied: None,
            white_bb: None,
            kind_bb: [None; 14],
        }
    }

    pub fn moved(&self, movement: &Movement) -> Position {
        let mut position = self.core.clone();
        position.do_move(movement);
        position
    }

    pub fn kind_bb(&mut self, kind: Kind) -> BitBoard {
        *self.kind_bb[kind.index()].get_or_insert_with(|| self.core.kind_bb().bitboard(kind))
    }

    pub fn bitboard(&mut self, color: Color, kind: Kind) -> BitBoard {
        self.kind_bb(kind) & self.color_bb(color)
    }

    pub fn occupied_bb(&mut self) -> BitBoard {
        *self
            .occupied
            .get_or_insert_with(|| self.core.kind_bb().occupied())
    }

    pub fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub fn white_bb(&mut self) -> BitBoard {
        if self.white_bb.is_none() {
            let occupied = self.occupied_bb();
            self.white_bb = Some(occupied.and_not(self.black_bb()));
        }
        self.white_bb.unwrap()
    }

    pub fn color_bb(&mut self, color: Color) -> BitBoard {
        if color.is_black() {
            self.core.black()
        } else {
            self.white_bb()
        }
    }

    pub fn color_bitboard(&mut self) -> ColorBitBoard {
        // Consider avoiding the clone
        ColorBitBoard::new(self.black_bb(), self.white_bb(), self.occupied_bb())
    }

    pub fn hands(&self) -> Hands {
        self.core.hands()
    }

    pub(crate) fn pawn_silver_goldish(&self) -> BitBoard {
        self.core.kind_bb().pawn_silver_goldish()
    }

    pub(crate) fn must_get_kind(&self, pos: Square) -> Kind {
        // TODO: consider having pos -> kind mapping
        self.core.kind_bb().must_get(pos)
    }

    pub(crate) fn get_kind(&self, dest: Square) -> Option<Kind> {
        self.core.kind_bb().get(dest)
    }

    pub fn get(&mut self, pos: Square) -> Option<(Color, Kind)> {
        if !self.occupied_bb().get(pos) {
            return None;
        }
        Some((
            Color::from_is_black(self.black_bb().get(pos)),
            self.must_get_kind(pos),
        ))
    }

    pub fn turn(&self) -> Color {
        self.core.turn()
    }

    pub fn bishopish(&self) -> BitBoard {
        self.core.kind_bb().bishopish()
    }

    pub fn rookish(&self) -> BitBoard {
        self.core.kind_bb().rookish()
    }

    pub fn goldish(&self) -> BitBoard {
        self.core.kind_bb().goldish()
    }

    pub fn pawn_drop(&self) -> bool {
        self.core.pawn_drop()
    }

    pub fn checked_slow(&mut self, king_color: Color) -> bool {
        let king_pos = self.bitboard(king_color, Kind::King).singleton();
        attacker(self, king_color, king_pos, true).is_some()
    }

    pub fn do_move(&mut self, movement: &Movement) {
        // Update
        // occupied, white_bb, kind_bb
        match movement {
            Movement::Move {
                source,
                source_kind_hint,
                dest,
                promote,
                capture_kind_hint,
            } => {
                let source_kind = source_kind_hint.unwrap_or_else(|| self.must_get_kind(*source));
                let capture_kind = capture_kind_hint.unwrap_or_else(|| self.get_kind(*dest));
                let dest_kind = if *promote {
                    source_kind.promote().unwrap()
                } else {
                    source_kind
                };

                // Update occupied
                if let Some(bb) = self.occupied.as_mut() {
                    bb.unset(*source);
                    bb.set(*dest);
                }

                // Update white_bb
                if self.turn().is_white() {
                    if let Some(bb) = self.white_bb.as_mut() {
                        bb.unset(*source);
                        bb.set(*dest);
                    }
                } else if capture_kind.is_some() {
                    if let Some(bb) = self.white_bb.as_mut() {
                        bb.unset(*dest);
                    }
                }

                // Update kind_bb
                if let Some(bb) = self.kind_bb[source_kind.index()].as_mut() {
                    bb.unset(*source);
                }
                if let Some(capture_kind) = capture_kind {
                    if let Some(bb) = self.kind_bb[capture_kind.index()].as_mut() {
                        bb.unset(*dest);
                    }
                }
                if let Some(bb) = self.kind_bb[dest_kind.index()].as_mut() {
                    bb.set(*dest);
                }
            }
            Movement::Drop(pos, kind) => {
                if let Some(bb) = self.occupied.as_mut() {
                    bb.set(*pos);
                }
                if self.turn().is_white() {
                    if let Some(bb) = self.white_bb.as_mut() {
                        bb.set(*pos);
                    }
                }
                if let Some(bb) = self.kind_bb[kind.index()].as_mut() {
                    bb.set(*pos);
                }
            }
        }
        self.core.do_move(movement);
    }

    pub fn digest(&self) -> u64 {
        self.core.digest()
    }

    // TODO: remember attackers
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
