use anyhow::bail;

use crate::piece::*;

pub enum UndoToken {
    UnDrop((Square, bool /* pawn drop */)),
    UnMove {
        from: Square,
        to: Square,
        promote: bool,
        capture: Option<Kind>,
        pawn_drop: bool,
    },
}

#[derive(Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct Position {
    kind_bb: [BitBoard; NUM_KIND],
    color_bb: [BitBoard; 2],
    hands: Hands,
    pub(super) turn: Color,
    pawn_drop: bool,
}

#[test]
fn test_position_size() {
    // 272 bytes.
    assert_eq!(272, std::mem::size_of::<Position>());
}

use crate::sfen;
use std::fmt;
impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", sfen::encode_position(self))
    }
}

use std::collections::HashMap;

use super::bitboard::BitBoard;
use super::hands::Hands;
use super::Movement;
use super::Square;

impl Position {
    pub fn new() -> Position {
        Position {
            kind_bb: [BitBoard::new(); NUM_KIND],
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
    pub(super) fn pawn_drop(&self) -> bool {
        self.pawn_drop
    }
    pub(super) fn set_pawn_drop(&mut self, x: bool) {
        self.pawn_drop = x;
    }
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        for c in Color::iter() {
            if !self.color_bb[c.index()].get(pos) {
                continue;
            }
            for k in Kind::iter() {
                if self.kind_bb[k.index()].get(pos) {
                    return Some((c, k));
                }
            }
        }
        None
    }
    pub fn was_pawn_drop(&self) -> bool {
        self.pawn_drop
    }
    pub(super) fn king(&self, c: Color) -> Option<Square> {
        for k in self.bitboard(Some(c), Some(King)) {
            return Some(k);
        }
        None
    }
    pub(super) fn kind(&self, pos: Square) -> Option<Kind> {
        for k in Kind::iter() {
            if self.kind_bb[k.index()].get(pos) {
                return Some(k);
            }
        }
        None
    }
    // Attackers with the given color to the given position, excluding king's movement.
    pub(super) fn attackers_to(
        &self,
        to: Square,
        c: Color,
    ) -> impl Iterator<Item = (Square, Kind)> + '_ {
        let occupied = self.bitboard(None, None);
        Kind::iter().flat_map(move |k| {
            let b = if k == King {
                BitBoard::new()
            } else {
                super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                    & self.bitboard(Some(c), Some(k))
            };
            b.map(move |from| (from, k))
        })
    }

    pub(super) fn attackers_to_with_king(
        &self,
        to: Square,
        c: Color,
    ) -> impl Iterator<Item = (Square, Kind)> + '_ {
        let occupied = self.bitboard(None, None);
        Kind::iter().flat_map(move |k| {
            let b = super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                & self.bitboard(Some(c), Some(k));
            b.map(move |from| (from, k))
        })
    }

    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(false, self.color_bb[c.index()].get(pos));
        self.color_bb[c.index()].set(pos);
        debug_assert_eq!(false, self.kind_bb[k.index()].get(pos));
        self.kind_bb[k.index()].set(pos);
    }
    pub(super) fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(self.color_bb[c.index()].get(pos));
        self.color_bb[c.index()].unset(pos);
        debug_assert!(self.kind_bb[k.index()].get(pos));
        self.kind_bb[k.index()].unset(pos);
    }
    pub(super) fn bitboard(&self, color: Option<Color>, kind: Option<Kind>) -> BitBoard {
        if let Some(c) = color {
            if let Some(k) = kind {
                return self.color_bb[c.index()] & self.kind_bb[k.index()];
            }
            return self.color_bb[c.index()];
        }
        if let Some(k) = kind {
            return self.kind_bb[k.index()];
        }
        self.color_bb[0] | self.color_bb[1]
    }
}

pub(super) fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Black => pos.row() < 3,
        White => pos.row() >= 6,
    }
}
