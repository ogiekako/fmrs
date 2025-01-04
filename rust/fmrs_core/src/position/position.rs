use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
pub struct Position {
    black_bb: BitBoard,     // 16 bytes
    kind_bb: KindBitBoard,  // 64 bytes
    hands: Hands,           // 8 bytes
    pub(super) digest: u64, // 8 bytes
}

use crate::sfen;
use std::fmt;
use std::fmt::Debug;

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", PositionAux::new(self.clone()).sfen_url())
    }
}

use super::advance::attack_prevent::attacker;
use super::bitboard::reachable_sub;
use super::bitboard::BitBoard;
use super::bitboard::KindBitBoard;
use super::hands::Hands;
use super::zobrist::zobrist;
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
        let kind = self.kind_bb.get(pos)?;
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

        self.digest ^= self.hash_at(pos);
    }
    pub fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), Some((c, k)));

        self.digest ^= self.hash_at(pos);

        if c.is_black() {
            self.black_bb.unset(pos);
        }
        self.kind_bb.unset(pos, k);
    }

    pub fn shift(&mut self, dir: Direction) {
        self.black_bb.shift(dir);
        self.kind_bb.shift(dir);
        self.digest = 0;
        for pos in self.kind_bb.occupied() {
            self.digest ^= self.hash_at(pos);
        }
    }

    pub(super) fn hash_at(&self, pos: Square) -> u64 {
        let color = if self.black_bb.get(pos) {
            Color::BLACK
        } else {
            Color::WHITE
        };
        let kind = self.kind_bb.must_get(pos);
        zobrist(color, pos, kind)
    }

    // #[inline(never)]
    pub fn digest(&self) -> u64 {
        self.digest ^ self.hands.x
    }

    pub fn sfen(&self) -> String {
        PositionAux::new(self.clone()).sfen()
    }
}

// TOOD: remove clone
#[derive(Clone, Default)]
pub struct PositionAux {
    core: Position,
    occupied: Option<BitBoard>,
    white_bb: Option<BitBoard>,
    white_king_pos: Option<Square>,
    black_king_pos: Option<Option<Square>>,
    stone: Option<BitBoard>,
}

impl PartialEq for PositionAux {
    fn eq(&self, other: &Self) -> bool {
        self.core == other.core && self.stone == other.stone
    }
}

impl Eq for PositionAux {}

impl Debug for PositionAux {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.clone().sfen_url())
    }
}

impl PositionAux {
    pub fn new(core: Position) -> Self {
        Self {
            core,
            ..Default::default()
        }
    }

    pub(crate) fn moved_digest(&self, movement: &Movement) -> u64 {
        self.core.moved_digest(movement)
    }

    pub(crate) fn kind_bb(&mut self, kind: Kind) -> BitBoard {
        self.core.kind_bb().bitboard(kind)
    }

    pub fn bitboard(&mut self, color: Color, kind: Kind) -> BitBoard {
        self.kind_bb(kind) & self.color_bb(color)
    }

    pub(crate) fn occupied_bb(&mut self) -> BitBoard {
        if let Some(occupied) = &self.occupied {
            return *occupied;
        }
        let mut res = self.core.kind_bb().occupied();
        if let Some(stone) = self.stone() {
            res |= *stone;
        }
        self.occupied = Some(res);
        res
    }

    pub(crate) fn capturable_by(&mut self, color: Color) -> BitBoard {
        if color.is_black() {
            self.white_bb()
        } else {
            self.core.black()
        }
    }

    pub(crate) fn color_bb_and_stone(&mut self, color: Color) -> BitBoard {
        let mut res = if color.is_black() {
            self.core.black()
        } else {
            self.white_bb()
        };
        if let Some(stone) = self.stone() {
            res |= *stone;
        }
        res
    }

    pub(crate) fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub(crate) fn white_bb(&mut self) -> BitBoard {
        if self.white_bb.is_none() {
            let occupied = self.occupied_bb();
            let white_bb = occupied.and_not(self.color_bb_and_stone(Color::BLACK));
            self.white_bb = Some(white_bb);
        }
        self.white_bb.unwrap()
    }

    pub(crate) fn color_bb(&mut self, color: Color) -> BitBoard {
        if color.is_black() {
            self.core.black()
        } else {
            self.white_bb()
        }
    }

    pub fn hands(&self) -> Hands {
        self.core.hands()
    }

    pub(crate) fn must_get_kind(&self, pos: Square) -> Kind {
        // TODO: consider having pos -> kind mapping
        self.core.kind_bb().must_get(pos)
    }

    pub(crate) fn get_kind(&self, dest: Square) -> Option<Kind> {
        self.core.kind_bb().get(dest)
    }

    pub fn get(&mut self, pos: Square) -> Option<(Color, Kind)> {
        if self.has_stone(pos) {
            return None;
        }
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

    pub(crate) fn pawn_silver_goldish(&self) -> BitBoard {
        self.core.kind_bb().pawn_silver_goldish()
    }

    pub(crate) fn bishopish(&mut self) -> BitBoard {
        self.core.kind_bb.bishopish()
    }

    pub(crate) fn rookish(&mut self) -> BitBoard {
        self.core.kind_bb.rookish()
    }

    pub fn pawn_drop(&self) -> bool {
        self.core.pawn_drop()
    }

    pub fn checked_slow(&mut self, king_color: Color) -> bool {
        attacker(self, king_color, true).is_some()
    }

    pub(crate) fn white_king_pos(&mut self) -> Square {
        if self.white_king_pos.is_none() {
            self.white_king_pos = Some((self.kind_bb(Kind::King) & self.white_bb()).singleton());
        }
        self.white_king_pos.unwrap()
    }

    pub(crate) fn black_king_pos(&mut self) -> Option<Square> {
        if self.black_king_pos.is_none() {
            self.black_king_pos = Some((self.kind_bb(Kind::King) & self.black_bb()).next());
        }
        self.black_king_pos.unwrap()
    }

    pub fn do_move(&mut self, movement: &Movement) {
        let turn = self.turn();

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
                if let Some(capture_kind) = capture_kind {
                    self.unset(*dest, turn.opposite(), capture_kind);
                    self.hands_mut().add(turn, capture_kind.maybe_unpromote());
                }
                self.unset(*source, turn, source_kind);
                self.set(*dest, turn, dest_kind);

                self.core.set_pawn_drop(false);
                self.core.set_turn(turn.opposite());
            }
            Movement::Drop(pos, kind) => {
                self.set(*pos, turn, *kind);
                self.hands_mut().remove(turn, *kind);

                self.core.set_pawn_drop(*kind == Kind::Pawn);
                self.core.set_turn(turn.opposite());
            }
        }
    }

    pub fn digest(&self) -> u64 {
        self.core.digest()
    }

    pub fn unset(&mut self, pos: Square, color: Color, kind: Kind) {
        if let Some(bb) = self.occupied.as_mut() {
            bb.unset(pos)
        }
        if color.is_white() {
            if let Some(bb) = self.white_bb.as_mut() {
                bb.unset(pos)
            }
        }

        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = None;
            } else {
                self.white_king_pos = None;
            }
        }

        self.core.unset(pos, color, kind);
    }

    pub fn set(&mut self, pos: Square, color: Color, kind: Kind) {
        debug_assert!(!self.has_stone(pos));
        if let Some(bb) = self.occupied.as_mut() {
            bb.set(pos)
        }
        if color.is_white() {
            if let Some(bb) = self.white_bb.as_mut() {
                bb.set(pos)
            }
        }

        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = Some(Some(pos));
            } else {
                self.white_king_pos = Some(pos);
            }
        }

        self.core.set(pos, color, kind);
    }

    pub fn hands_mut(&mut self) -> &mut Hands {
        self.core.hands_mut()
    }

    pub fn set_turn(&mut self, color: Color) {
        self.core.set_turn(color);
    }

    pub fn shift(&mut self, dir: Direction) {
        if let Some(bb) = self.occupied.as_mut() {
            bb.shift(dir)
        }
        if let Some(bb) = self.white_bb.as_mut() {
            bb.shift(dir)
        }
        if let Some(pos) = self.white_king_pos.as_mut() {
            pos.shift(dir)
        }
        self.black_king_pos
            .as_mut()
            .map(|pos| pos.as_mut().map(|pos| pos.shift(dir)));

        self.core.shift(dir);
    }

    pub(crate) fn must_king_pos(&mut self, king_color: Color) -> Square {
        if king_color.is_black() {
            self.black_king_pos().unwrap()
        } else {
            self.white_king_pos()
        }
    }

    pub(crate) fn must_turn_king_pos(&mut self) -> Square {
        if self.turn().is_black() {
            self.black_king_pos().unwrap()
        } else {
            self.white_king_pos()
        }
    }

    pub(crate) fn white_king_attack_squares(&mut self, kind: Kind) -> BitBoard {
        let white_king_pos = self.white_king_pos();
        reachable_sub(self, Color::WHITE, white_king_pos, kind)
    }

    pub(crate) fn core(&self) -> &Position {
        &self.core
    }

    pub fn set_stone(&mut self, stone: BitBoard) {
        self.stone = Some(stone);
    }

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn sfen(&mut self) -> String {
        sfen::encode_position(self)
    }

    pub fn sfen_url(&mut self) -> String {
        sfen::sfen_to_image_url(&self.sfen())
    }

    pub fn set_pawn_drop(&mut self, pawn_drop: bool) {
        self.core.set_pawn_drop(pawn_drop);
    }

    pub(crate) fn stone(&self) -> &Option<BitBoard> {
        &self.stone
    }

    fn has_stone(&self, pos: Square) -> bool {
        self.stone.as_ref().map_or(false, |stone| stone.get(pos))
    }

    pub fn undo_move(&mut self, token: &super::UndoMove) -> Movement {
        let mut core = self.core.clone();
        let movement = core.undo_move(token);
        *self = Self::new(core);
        movement
    }

    // TODO: remember attackers
}

#[cfg(test)]
mod tests {
    use crate::{
        direction::Direction,
        position::{position::PositionAux, BitBoard, Square},
    };

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
            let mut pos1 = PositionAux::from_sfen(pos1).unwrap();
            let mut pos2 = PositionAux::from_sfen(pos2).unwrap();

            for pawn_drop1 in [true, false] {
                for pawn_drop2 in [true, false] {
                    pos1.set_pawn_drop(pawn_drop1);
                    pos2.set_pawn_drop(pawn_drop2);

                    assert_ne!(pos1, pos2);
                    assert_ne!(pos1.digest(), pos2.digest(), "{:?} {:?}", pos1, pos2);

                    pretty_assertions::assert_eq!(
                        PositionAux::from_sfen(&pos1.sfen()).unwrap(),
                        pos1
                    );
                    pretty_assertions::assert_eq!(
                        PositionAux::from_sfen(&pos2.sfen()).unwrap(),
                        pos2
                    );
                }
            }
        }
    }

    #[test]
    fn position_size() {
        assert_eq!(std::mem::size_of::<Position>(), 96);
    }

    #[test]
    fn test_stone() {
        use crate::position::Position;

        let mut position = PositionAux::new(Position::default());
        let mut stone = BitBoard::default();
        stone.set(Square::new(0, 0));
        position.set_stone(stone);

        assert_eq!(position.sfen(), "8O/9/9/9/9/9/9/9/9 b - 1");
    }
}
