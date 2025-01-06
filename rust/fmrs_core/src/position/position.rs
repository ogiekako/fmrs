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
        write!(f, "{}", PositionAux::new(self.clone(), None).sfen_url())
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
        PositionAux::new(self.clone(), None).sfen()
    }
}

#[derive(Clone, Default)]
pub struct PositionAux {
    core: Position,
    occupied: BitBoard,
    white_bb: BitBoard,
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
    pub fn new(core: Position, stone: Option<BitBoard>) -> Self {
        let mut occupied = core.kind_bb().occupied();
        let white_bb = occupied.and_not(core.black());
        if let Some(stone) = &stone {
            occupied |= *stone;
        }
        Self {
            core,
            stone,
            occupied,
            white_bb,
            ..Default::default()
        }
    }

    pub(crate) fn moved_digest(&self, movement: &Movement) -> u64 {
        self.core.moved_digest(movement)
    }

    pub(crate) fn kind_bb(&self, kind: Kind) -> BitBoard {
        self.core.kind_bb().bitboard(kind)
    }

    pub fn bitboard(&self, color: Color, kind: Kind) -> BitBoard {
        self.kind_bb(kind) & self.color_bb(color)
    }

    pub(crate) fn occupied_bb(&self) -> BitBoard {
        self.occupied
    }

    pub(crate) fn capturable_by(&self, color: Color) -> BitBoard {
        if color.is_black() {
            self.white_bb()
        } else {
            self.core.black()
        }
    }

    pub(crate) fn color_bb_and_stone(&self, color: Color) -> BitBoard {
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

    pub fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub fn white_bb(&self) -> BitBoard {
        self.white_bb
    }

    pub fn color_bb(&self, color: Color) -> BitBoard {
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

    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
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

    pub(crate) fn bishopish(&self) -> BitBoard {
        self.core.kind_bb.bishopish()
    }

    pub(crate) fn rookish(&self) -> BitBoard {
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
        self.occupied.unset(pos);
        if color.is_white() {
            self.white_bb.unset(pos);
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
        self.occupied.set(pos);
        if color.is_white() {
            self.white_bb.set(pos);
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
        if let Some(stone) = self.stone.as_mut() { stone.shift(dir) }
        self.occupied.shift(dir);
        self.white_bb.shift(dir);
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

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn sfen(&self) -> String {
        sfen::encode_position(self)
    }

    pub fn sfen_url(&self) -> String {
        sfen::sfen_to_image_url(&self.sfen())
    }

    pub fn set_pawn_drop(&mut self, pawn_drop: bool) {
        self.core.set_pawn_drop(pawn_drop);
    }

    pub fn stone(&self) -> &Option<BitBoard> {
        &self.stone
    }

    fn has_stone(&self, pos: Square) -> bool {
        self.stone.as_ref().map_or(false, |stone| stone.get(pos))
    }

    pub fn undo_move(&mut self, token: &super::UndoMove) -> Movement {
        let mut core = self.core.clone();
        let res = core.undo_move(token);
        *self = Self::new(core, self.stone);
        res
    }

    pub fn col_has_pawn(&self, color: Color, col: usize) -> bool {
        let pawn_bb = self.bitboard(color, Kind::Pawn).u128();
        let mask = (1 << (col * 9 + 9)) - (1 << (col * 9));
        pawn_bb & mask != 0
    }

    pub fn flipped(&self) -> Self {
        let mut core = Position::default();
        let mut stone = BitBoard::default();
        for pos in Square::iter() {
            if let Some((color, kind)) = self.get(pos) {
                core.set(pos.flipped(), color.opposite(), kind);
            }
            if self.stone().map_or(false, |stone| stone.get(pos)) {
                stone.set(pos.flipped());
            }
        }
        core.set_turn(self.turn().opposite());
        Self {
            core,
            stone: (!stone.is_empty()).then_some(stone),
            ..Default::default()
        }
    }

    pub fn set_stone(&mut self, stone: BitBoard) {
        self.stone = Some(stone);
        self.occupied |= stone;
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

        let mut stone = BitBoard::default();
        stone.set(Square::new(0, 0));
        let position = PositionAux::new(Position::default(), stone.into());

        assert_eq!(position.sfen(), "8O/9/9/9/9/9/9/9/9 b - 1");
    }
}
