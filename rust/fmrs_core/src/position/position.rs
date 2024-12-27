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
use std::fmt::Debug;

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.sfen_url())
    }
}

use super::advance::attack_prevent::attacker;
use super::bitboard::reachable_sub;
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
#[derive(Clone, Default)]
pub struct PositionAux {
    core: Position,
    occupied: BitBoard,
    white_bb: BitBoard,
    kind_bb: [BitBoard; NUM_KIND],
    white_king_pos: Square,
    black_king_pos: Option<Square>,
    // aux data (updated in update_aux)
    bishopish: BitBoard,
    rookish: BitBoard,
}

impl Debug for PositionAux {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.core.sfen_url())
    }
}

impl PositionAux {
    pub fn new(core: Position) -> Self {
        let mut res = Self::default();
        *res.core.hands_mut() = core.hands();

        let mut updater = res.updater();
        for pos in Square::iter() {
            if let Some((color, kind)) = core.get(pos) {
                updater.set(pos, color, kind);
            }
        }
        updater.commit();

        res
    }

    pub fn moved(&self, movement: &Movement) -> Position {
        let mut position = self.core.clone();
        position.do_move(movement);
        position
    }

    pub fn kind_bb(&mut self, kind: Kind) -> BitBoard {
        self.kind_bb[kind.index()]
    }

    pub fn bitboard(&mut self, color: Color, kind: Kind) -> BitBoard {
        self.kind_bb(kind) & self.color_bb(color)
    }

    pub fn occupied_bb(&mut self) -> BitBoard {
        self.occupied
    }

    pub fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub fn white_bb(&mut self) -> BitBoard {
        self.white_bb
    }

    pub fn color_bb(&mut self, color: Color) -> BitBoard {
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

    pub fn pawn_silver_goldish(&self) -> BitBoard {
        self.core.kind_bb().pawn_silver_goldish()
    }

    pub fn bishopish(&mut self) -> BitBoard {
        self.bishopish
    }

    pub fn rookish(&mut self) -> BitBoard {
        self.rookish
    }

    pub fn goldish(&self) -> BitBoard {
        self.core.kind_bb().goldish()
    }

    pub fn pawn_drop(&self) -> bool {
        self.core.pawn_drop()
    }

    pub fn checked_slow(&mut self, king_color: Color) -> bool {
        attacker(self, king_color, true).is_some()
    }

    pub fn white_king_pos(&mut self) -> Square {
        self.white_king_pos
    }

    pub fn black_king_pos(&mut self) -> Option<Square> {
        self.black_king_pos
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
                    self.hands_mut().add(turn, capture_kind.maybe_unpromote());
                }

                let mut updater = self.updater();
                if let Some(capture_kind) = capture_kind {
                    updater.unset(*dest, turn.opposite(), capture_kind);
                }
                updater.unset(*source, turn, source_kind);
                updater.set(*dest, turn, dest_kind);
                updater.commit();

                self.core.set_pawn_drop(false);
                self.core.set_turn(turn.opposite());
            }
            Movement::Drop(pos, kind) => {
                self.set(*pos, turn, *kind);
                self.update_aux();

                self.hands_mut().remove(turn, *kind);

                self.core.set_pawn_drop(*kind == Kind::Pawn);
                self.core.set_turn(turn.opposite());
            }
        }
    }

    pub fn digest(&self) -> u64 {
        self.core.digest()
    }

    pub fn updater(&mut self) -> PositionUpdater {
        PositionUpdater::new(self)
    }

    fn unset(&mut self, pos: Square, color: Color, kind: Kind) {
        self.occupied.unset(pos);
        if color.is_white() {
            self.white_bb.unset(pos);
        }
        self.kind_bb[kind.index()].unset(pos);

        if kind == Kind::King && color == Color::BLACK {
            self.black_king_pos = None;
        }

        self.core.unset(pos, color, kind);
    }

    fn set(&mut self, pos: Square, color: Color, kind: Kind) {
        self.occupied.set(pos);
        if color.is_white() {
            self.white_bb.set(pos);
        }
        self.kind_bb[kind.index()].set(pos);

        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = Some(pos);
            } else {
                self.white_king_pos = pos;
            }
        }

        self.core.set(pos, color, kind);
    }

    fn update_aux(&mut self) {
        self.bishopish = self.kind_bb[Kind::Bishop.index()] | self.kind_bb[Kind::ProBishop.index()];
        self.rookish = self.kind_bb[Kind::Rook.index()] | self.kind_bb[Kind::ProRook.index()];
    }

    pub fn hands_mut(&mut self) -> &mut Hands {
        self.core.hands_mut()
    }

    pub fn set_turn(&mut self, color: Color) {
        self.core.set_turn(color);
    }

    pub fn shift(&mut self, dir: Direction) {
        // TODO: consider using more efficient algorithm
        let mut res = Self::default();
        *res.hands_mut() = self.hands();
        for pos in Square::iter() {
            if let Some((color, kind)) = self.get(pos) {
                let mut new_pos = pos;
                new_pos.shift(dir);
                res.set(new_pos, color, kind);
            }
        }
        *self = res;
        self.update_aux();
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

    pub fn core(&self) -> &Position {
        &self.core
    }

    // TODO: remember attackers
}

pub struct PositionUpdater<'a> {
    position: &'a mut PositionAux,
    ops: Vec<PositionUpdate>,
}

impl<'a> PositionUpdater<'a> {
    fn new(position: &'a mut PositionAux) -> Self {
        Self {
            position,
            ops: vec![],
        }
    }

    pub fn set(&mut self, pos: Square, color: Color, kind: Kind) -> &mut Self {
        self.ops.push(PositionUpdate::set(pos, color, kind));
        self
    }

    pub fn unset(&mut self, pos: Square, color: Color, kind: Kind) -> &mut Self {
        self.ops.push(PositionUpdate::unset(pos, color, kind));
        self
    }

    pub fn commit(&mut self) {
        for op in self.ops.iter() {
            match op {
                &PositionUpdate::Set(pos, color, kind) => self.position.set(pos, color, kind),
                &PositionUpdate::Unset(pos, color, kind) => self.position.unset(pos, color, kind),
            }
        }
        self.position.update_aux();
    }
}

enum PositionUpdate {
    Set(Square, Color, Kind),
    Unset(Square, Color, Kind),
}

impl PositionUpdate {
    fn set(pos: Square, color: Color, kind: Kind) -> Self {
        Self::Set(pos, color, kind)
    }
    fn unset(pos: Square, color: Color, kind: Kind) -> Self {
        Self::Unset(pos, color, kind)
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
