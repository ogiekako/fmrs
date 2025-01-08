use std::{ops::RangeInclusive, sync::Mutex};

use fmrs_core::{
    piece::{Color, Kind},
    position::{position::PositionAux, Square},
};
use log::{debug, info};
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};
use serde::{Deserialize, Serialize};

use super::{
    mate_formation::MateFormationFilter,
    room::{Room, RoomFilter},
};

#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub(super) room: Room,
    pub(super) white_pawn: u16,
    pub(super) black_pawn: u16,
}

#[derive(Default, Clone, Debug)]
pub(super) struct FrameMetadata {
    pub(super) mate_with_minimum_pawn: Option<Vec<PositionAux>>,
}

impl Frame {
    pub(super) fn to_position(&self) -> PositionAux {
        let mut position = PositionAux::default();
        let stone = self.room.stone();

        position.set_stone(stone);

        for i in 0..self.room.width() as usize {
            if self.white_pawn & 1 << i != 0 {
                position.set(Square::new(i, 0), Color::WHITE, Kind::Pawn);
            }
            if self.black_pawn & 1 << i != 0 {
                position.set(Square::new(i, 1), Color::BLACK, Kind::Pawn);
            }
        }

        position
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct FrameFilter {
    pub(super) room_filter: RoomFilter,
    pub(super) no_black_pawn_count: Option<RangeInclusive<u8>>,
    pub(super) no_white_pawn_count: Option<RangeInclusive<u8>>,
    pub(super) mate_formation_filter: Option<MateFormationFilter>,
}

impl FrameFilter {
    pub(super) fn generate_frames(&self) -> Vec<(Frame, FrameMetadata)> {
        let rooms = self.room_filter.generate_rooms();

        debug!("rooms: {}", rooms.len());

        let mut frames = vec![];

        for room in rooms {
            let w = room.width();

            let black_masks = pawn_masks(w, &self.no_black_pawn_count);
            let white_masks = pawn_masks(w, &self.no_white_pawn_count);

            for black_pawn in black_masks.iter().copied() {
                for white_pawn in white_masks.iter().copied() {
                    frames.push(Frame {
                        room: room.clone(),
                        white_pawn,
                        black_pawn,
                    });
                }
            }
        }
        let Some(mate_formation_filter) = &self.mate_formation_filter else {
            return frames
                .into_iter()
                .map(|frame| (frame, FrameMetadata::default()))
                .collect();
        };

        debug!("frames: {}", frames.len());

        let frames_len = frames.len();
        let frame_i = Mutex::new(0);

        frames
            .into_par_iter()
            .filter_map(|frame| {
                {
                    let mut frame_i = frame_i.lock().unwrap();
                    *frame_i += 1;
                    if *frame_i % 1000 == 0 {
                        info!("frame {}/{}", *frame_i, frames_len);
                    }
                }

                let mates = mate_formation_filter.check(&frame);
                if mates.is_empty() {
                    return None;
                }
                Some((
                    frame,
                    FrameMetadata {
                        mate_with_minimum_pawn: Some(mates),
                    },
                ))
            })
            .collect()
    }
}

fn pawn_masks(w: u8, no_count: &Option<RangeInclusive<u8>>) -> Vec<u16> {
    let mut masks = vec![];
    for i in 0u16..1 << w {
        if let Some(no_count) = &no_count {
            if !no_count.contains(&(w - i.count_ones() as u8)) {
                continue;
            }
        }
        masks.push(i);
    }
    masks
}
