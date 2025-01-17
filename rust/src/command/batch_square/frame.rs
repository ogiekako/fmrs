use fmrs_core::{
    piece::{Color, Kind},
    position::{position::PositionAux, BitBoard, Square},
};
use serde::{Deserialize, Serialize};

use super::room::{Room, RoomFilter};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Frame {
    pub(super) room: Room,
    pub(super) white_pawn: u16,
    pub(super) black_pawn: u16,
}

impl Frame {
    pub(crate) fn new(room: Room, black_pawn: u16, white_pawn: u16) -> Self {
        Self {
            room,
            white_pawn,
            black_pawn,
        }
    }

    pub(super) fn to_position(&self) -> PositionAux {
        let mut position = PositionAux::default();
        let stone = self.room.stone();

        position.set_stone(stone);

        for i in 0..self.room.width() as usize {
            if self.white_pawn & 1 << i != 0 {
                position.set(Self::white_pawn(i), (Color::WHITE, Kind::Pawn).into());
            }
            if self.black_pawn & 1 << i != 0 {
                position.set(Self::black_pawn_row(i), (Color::BLACK, Kind::Pawn).into());
            }
        }

        position
    }

    pub(super) fn white_pawn(col: usize) -> Square {
        Square::new(col, 0)
    }

    pub(super) fn black_pawn_row(col: usize) -> Square {
        Square::new(col, 1)
    }

    pub(crate) fn matches(&self, position: &PositionAux) -> bool {
        if position.stone() != &Some(self.room.stone()) {
            return false;
        }
        if (position.bitboard(Color::WHITE, Kind::Pawn) & BitBoard::ROW1)
            .fold(0, |acc, s| acc | 1 << s.col())
            != self.white_pawn
        {
            return false;
        }
        if (position.bitboard(Color::BLACK, Kind::Pawn) & BitBoard::ROW2)
            .fold(0, |acc, s| acc | 1 << s.col())
            != self.black_pawn
        {
            return false;
        }

        true
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct FrameFilter {
    pub(super) room_filter: RoomFilter,
    pub(super) max_empty_black_pawn_col: Option<u8>,
    pub(super) max_empty_white_pawn_col: Option<u8>,
}

impl FrameFilter {
    pub(crate) fn too_loose(&self, frame: &Frame) -> bool {
        if let Some(max) = self.max_empty_black_pawn_col {
            if max < frame.room.width() - frame.black_pawn.count_ones() as u8 {
                return true;
            }
        }
        if let Some(max) = self.max_empty_white_pawn_col {
            if max < frame.room.width() - frame.white_pawn.count_ones() as u8 {
                return true;
            }
        }
        false
    }
}
