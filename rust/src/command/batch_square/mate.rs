use std::{ops::RangeInclusive, sync::Mutex};

use fmrs_core::{
    nohash::NoHashMap64,
    piece::{Color, Kind},
    position::{position::PositionAux, Square},
    solve::standard_solve::standard_solve,
};
use log::{debug, info};
use rayon::iter::{IntoParallelIterator, ParallelIterator as _};
use serde::{Deserialize, Serialize};

use crate::opt::{zero_region_of_almost_monotone_func, BoxKd};

use super::{
    frame::{Frame, FrameFilter},
    room::Room,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct MateFilter {
    pub(super) frame_filter: FrameFilter,
    pub(super) attackers: Vec<Kind>,
    pub(super) no_redundant: bool,
    pub(super) no_less_pro_pawn: u8,
    pub(super) max_extra_white_hand_pawn: Option<u8>,
    pub(super) skip_known_mates: bool,
}

impl MateFilter {
    const MAX_WHITE_HAND_PAWNS: usize = 7;

    pub(crate) fn generate_mates(&self) -> Vec<PositionAux> {
        let rooms = self.frame_filter.room_filter.generate_rooms();

        info!("rooms: {}", rooms.len());

        // Categorize per king position.
        let position_bases = rooms
            .into_iter()
            .flat_map(|room| {
                room.bitboard()
                    .map(move |king_pos| (room.clone(), king_pos))
            })
            .collect::<Vec<_>>();

        let total_len = position_bases.len();
        let iter = Mutex::new(0);

        info!("position bases: {}", total_len);

        let mut mates = position_bases
            .into_par_iter()
            .flat_map_iter(|(room, king_pos)| {
                {
                    let mut i = iter.lock().unwrap();
                    *i += 1;
                    if *i % 100 == 0 {
                        info!("position {}/{}", *i, total_len);
                    } else {
                        debug!("position {}/{}", *i, total_len);
                    }
                }

                let mut good_mates = NoHashMap64::default();

                let base = PositionBase::new(
                    room,
                    king_pos,
                    self.attackers.clone(),
                    0..=Self::MAX_WHITE_HAND_PAWNS as i32,
                );

                let mut f = |x: &[i32]| {
                    let frame_position = base.frame_position(x);
                    if self.frame_filter.too_loose(&frame_position.frame) {
                        return 1;
                    }

                    let mut position = frame_position.position();
                    let digest = position.digest();
                    if good_mates.contains_key(&digest) {
                        return 0;
                    }

                    let mut solutions = standard_solve(position.clone(), 1, true).unwrap();
                    if solutions.is_empty() {
                        return -1;
                    }

                    for m in solutions.remove(0) {
                        position.do_move(&m);
                    }
                    if self.is_good_mate(&position) {
                        good_mates.insert(digest, position);
                        0
                    } else {
                        1
                    }
                };

                let (initial_region, strictly_monotone) = base.region();
                for region in
                    zero_region_of_almost_monotone_func(&mut f, &initial_region, strictly_monotone)
                {
                    for x in region.iter() {
                        let v = f(&x);
                        assert_eq!(v, 0);
                    }
                }
                good_mates.into_values()
            })
            .collect::<Vec<_>>();
        mates.sort_by_key(|p| p.digest());
        mates.dedup();

        if let Some(max_extra_white_hand_pawn) = self.max_extra_white_hand_pawn {
            let mut mate_set = NoHashMap64::default();
            mates.iter().for_each(|p| {
                mate_set.insert(p.digest(), p);
            });
            for p in mates.iter() {
                let n = p.hands().count(Color::WHITE, Kind::Pawn);
                for c in 0..n.saturating_sub(max_extra_white_hand_pawn as usize) {
                    let mut np = p.clone();
                    np.hands_mut().remove_n(Color::WHITE, Kind::Pawn, n - c);
                    if mate_set.contains_key(&np.digest()) {
                        mate_set.remove(&p.digest());
                        break;
                    }
                }
            }
            mates = mate_set.into_values().cloned().collect();
            mates.sort_by_key(|p| p.digest());
        }

        mates
    }

    fn is_good_mate(&self, position: &PositionAux) -> bool {
        if self.no_redundant && !position.hands().is_empty(Color::BLACK) {
            return false;
        }
        if position.bitboard(Color::WHITE, Kind::ProPawn).count_ones()
            < self.no_less_pro_pawn as u32
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PositionBase {
    room: Room,
    king_pos: Square,
    attackers: Vec<Kind>,
    hand_white_pawns: RangeInclusive<i32>,
}

impl PositionBase {
    fn new(
        room: Room,
        king_pos: Square,
        attackers: Vec<Kind>,
        hand_white_pawns: RangeInclusive<i32>,
    ) -> Self {
        Self {
            room,
            king_pos,
            attackers,
            hand_white_pawns,
        }
    }

    fn frame_position(&self, x: &[i32]) -> FramePosition {
        let white_hand_pawn = x[0];
        let black_pawn = x[1..1 + self.room.width() as usize]
            .iter()
            .enumerate()
            .fold(0, |acc, (i, &b)| acc | (1 - b) << i) as u16;
        let white_pawn = x[1 + self.room.width() as usize..1 + self.room.width() as usize * 2]
            .iter()
            .enumerate()
            .fold(0, |acc, (i, &b)| acc | (1 - b) << i) as u16;

        FramePosition::new(
            Frame::new(self.room.clone(), black_pawn, white_pawn),
            self.king_pos,
            self.attackers.clone(),
            white_hand_pawn as u8,
        )
    }

    fn region(&self) -> (BoxKd, u64) {
        let mut ranges = vec![];
        let mut strictly_monotone = 0;
        ranges.push(self.hand_white_pawns.clone());
        for _ in 0..self.room.width() {
            strictly_monotone |= 1 << ranges.len();
            ranges.push(0..=1); // black
        }
        for _ in 0..self.room.width() {
            ranges.push(0..=1); // white
        }
        (ranges.into(), strictly_monotone)
    }
}

#[derive(Clone, Debug)]
struct FramePosition {
    frame: Frame,
    king_pos: Square,
    attackers: Vec<Kind>,
    white_hand_pawn: u8,
}

impl FramePosition {
    fn new(frame: Frame, king_pos: Square, attackers: Vec<Kind>, white_hand_pawn: u8) -> Self {
        Self {
            frame,
            king_pos,
            attackers,
            white_hand_pawn,
        }
    }

    fn position(&self) -> PositionAux {
        let mut position = self.frame.to_position();
        position.set(self.king_pos, Color::WHITE, Kind::King);
        let hands = position.hands_mut();
        hands.add_n(Color::WHITE, Kind::Pawn, self.white_hand_pawn as usize);
        for &kind in &self.attackers {
            hands.add(Color::BLACK, kind);
        }
        position
    }
}
