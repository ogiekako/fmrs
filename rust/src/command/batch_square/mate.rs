use std::{ops::RangeInclusive, sync::Mutex};

use fmrs_core::{
    nohash::NoHashMap64,
    piece::{Color, Kind},
    position::position::PositionAux,
    solve::standard_solve::standard_solve_mult,
};
use log::{debug, info, warn};
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

    pub(crate) fn generate_mates(&self) -> Vec<(Frame, PositionAux)> {
        let rooms = self.frame_filter.room_filter.generate_rooms();
        info!("rooms: {}", rooms.len());

        let parity = self.attackers.iter().all(|k| k != &Kind::Rook);
        let bases = rooms
            .into_iter()
            .flat_map(|room| {
                let pawns = 0..=Self::MAX_WHITE_HAND_PAWNS as i32;
                if parity {
                    vec![
                        PositionBase::new(
                            room.clone(),
                            self.attackers.clone(),
                            pawns.clone(),
                            Some(true),
                        ),
                        PositionBase::new(room, self.attackers.clone(), pawns, Some(false)),
                    ]
                } else {
                    vec![PositionBase::new(room, self.attackers.clone(), pawns, None)]
                }
            })
            .collect::<Vec<_>>();

        info!("bases: {}", bases.len());

        let total_len = bases.len();
        let iter = Mutex::new(0);

        let mut mates = bases
            .into_par_iter()
            .flat_map_iter(|base| {
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

                let mut f = |x: &[i32]| {
                    let frame_position = base.frame_position(x);
                    if self.frame_filter.too_loose(&frame_position.frame) {
                        return 1;
                    }
                    let digest = frame_position.template.digest();
                    if good_mates.contains_key(&digest) {
                        return 0;
                    }

                    let positions = frame_position.positions();

                    let reconstructor = standard_solve_mult(positions, 1, true).unwrap();
                    if reconstructor.is_empty() {
                        return -1;
                    }

                    let mate = reconstructor.mates().first().unwrap();
                    if self.is_good_mate(mate) {
                        good_mates.insert(digest, (frame_position.frame, mate.clone()));
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
                        if v != 0 {
                            let position = base.frame_position(&x).positions().remove(0);
                            warn!("v = {}, want 0; {}", v, position.sfen_url());
                        }
                    }
                }
                good_mates.into_values()
            })
            .collect::<Vec<_>>();
        mates.sort_by_key(|(_, p)| p.digest());
        mates.dedup();

        if let Some(max_extra_white_hand_pawn) = self.max_extra_white_hand_pawn {
            let mut mate_set = NoHashMap64::default();
            mates.iter().for_each(|(f, p)| {
                mate_set.insert(p.digest(), (f, p));
            });
            for (_, p) in mates.iter() {
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
            mates = mate_set
                .into_values()
                .map(|(f, p)| (f.clone(), p.clone()))
                .collect();
            mates.sort_by_key(|(_, p)| p.digest());
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
    attackers: Vec<Kind>,
    hand_white_pawns: RangeInclusive<i32>,
    parity: Option<bool>,
}

impl PositionBase {
    fn new(
        room: Room,
        attackers: Vec<Kind>,
        hand_white_pawns: RangeInclusive<i32>,
        parity: Option<bool>,
    ) -> Self {
        Self {
            room,
            attackers,
            hand_white_pawns,
            parity,
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
            self.attackers.clone(),
            white_hand_pawn as u8,
            self.parity,
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
    parity: Option<bool>,
    template: PositionAux,
}

impl FramePosition {
    fn new(frame: Frame, attackers: Vec<Kind>, white_hand_pawn: u8, parity: Option<bool>) -> Self {
        let mut template = frame.to_position();
        let hands = template.hands_mut();
        hands.add_n(Color::WHITE, Kind::Pawn, white_hand_pawn as usize);
        for &kind in &attackers {
            hands.add(Color::BLACK, kind);
        }

        Self {
            frame,
            parity,
            template,
        }
    }

    fn positions(&self) -> Vec<PositionAux> {
        let mut res = vec![];
        for king_pos in self.frame.room.bitboard() {
            if self
                .parity
                .map_or(true, |parity| parity != king_pos.parity())
            {
                let mut position = self.template.clone();
                position.set(king_pos, (Color::WHITE, Kind::King).into());
                res.push(position);
            }
        }
        res
    }
}
