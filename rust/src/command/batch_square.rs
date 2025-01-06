use std::{ops::RangeInclusive, sync::Mutex};

use fmrs_core::{
    piece::{Color, Kind},
    position::{position::PositionAux, BitBoard, Square},
    search::backward::backward_search,
    solve::standard_solve::standard_solve,
};
use log::{debug, info};
// use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub fn batch_square(filter_file: Option<String>) -> anyhow::Result<()> {
    let filter = if let Some(filter_file) = &filter_file {
        serde_json::from_str::<FrameFilter>(&std::fs::read_to_string(filter_file)?)?
    } else {
        FrameFilter {
            room_filter: RoomFilter {
                width: vec![7, 9],
                height: 2..=3,
                weakly_decreasing: true,
                area: Some(16..=27),
            },
            no_black_pawn_count: Some(1..=3),
            no_white_pawn_count: Some(1..=3),
            mate_formation_filter: Some(MateFormationFilter {
                attacker_kind: Kind::Rook,
                no_redundant: true,
                unique: false,
                no_less_pro_pawn: 1,
                pawn_maximally_constrained: true,
            }),
        }
    };

    let frames = frames(&filter);

    let positions: Vec<_> = frames
        .into_iter()
        .filter_map(|(_, metadata)| metadata.mate_with_minimum_pawn)
        .flatten()
        .collect();
    // positions.shuffle(&mut SmallRng::seed_from_u64(20250105));

    eprintln!("{} positions {:?}", positions.len(), positions[0]);

    let chunk_size = 1000;
    let chunks = positions.chunks(chunk_size).collect::<Vec<_>>();

    let mut best_solutions = (0, vec![]);
    for (i, chunk) in chunks.into_iter().enumerate() {
        let solutions = chunk
            .into_par_iter()
            .map(|position| {
                let res = backward_search(position, true).unwrap();
                debug_assert!(!res.1.is_empty(), "{} {:?}", res.0, position);
                res
            })
            .collect::<Vec<_>>();

        for (step, positions) in solutions {
            match step.cmp(&best_solutions.0) {
                std::cmp::Ordering::Less => continue,
                std::cmp::Ordering::Greater => best_solutions = (step, positions),
                std::cmp::Ordering::Equal => best_solutions.1.extend(positions),
            }
        }
        info!(
            "{}/{} best {} {:?}",
            ((i + 1) * chunk_size).min(positions.len()),
            positions.len(),
            best_solutions.0,
            best_solutions.1.last().unwrap(),
        );
    }
    eprintln!("mate in {}:", best_solutions.0);
    for position in best_solutions.1 {
        eprintln!("{}", position.sfen_url());
        println!("{}", position.sfen());
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct Frame {
    room: Room,
    white_pawn: u16,
    black_pawn: u16,
}

impl Frame {
    fn to_position(&self) -> PositionAux {
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
struct MateFormationFilter {
    attacker_kind: Kind,
    no_redundant: bool,
    unique: bool,
    no_less_pro_pawn: u8,
    pawn_maximally_constrained: bool,
}

impl MateFormationFilter {
    fn check(&self, frame: &Frame) -> Vec<PositionAux> {
        let mut mate_positions = vec![];

        let room = frame.room.bitboard();
        if room.is_empty() {
            return mate_positions;
        }

        let has_parity = self.attacker_kind == Kind::Bishop;

        let mut representatives = vec![];

        let frame_position = frame.to_position();

        for king_pos in room {
            if representatives.len() >= 2 || representatives.len() == 1 && !has_parity {
                break;
            }
            let mut position = frame_position.clone();
            position.set(king_pos, Color::WHITE, Kind::King);
            position.hands_mut().add(Color::BLACK, self.attacker_kind);
            representatives.push(position);
        }

        for representative in representatives {
            let mut impossible_max_pawn = 0;
            for bit in [4, 2, 1] {
                let c = impossible_max_pawn | bit;

                let mut position = representative.clone();
                for _ in 0..c {
                    position.hands_mut().add(Color::WHITE, Kind::Pawn);
                }
                let solution = standard_solve(position.clone(), 1, true).unwrap();
                if solution.is_empty() {
                    impossible_max_pawn = c;
                }
            }
            if impossible_max_pawn == 7 {
                continue;
            }
            let min_pawn = impossible_max_pawn + 1;

            let mut position = representative.clone();
            for _ in 0..min_pawn {
                position.hands_mut().add(Color::WHITE, Kind::Pawn);
            }
            let solution = standard_solve(position.clone(), 1, true).unwrap().remove(0);

            let mut black_pawn = frame_position.bitboard(Color::BLACK, Kind::Pawn);
            let mut white_pawn = frame_position.bitboard(Color::WHITE, Kind::Pawn);

            let mut mate_position = position.clone();
            for m in solution {
                mate_position.do_move(&m);
                black_pawn |= mate_position.bitboard(Color::BLACK, Kind::Pawn);
                white_pawn |= mate_position.bitboard(Color::WHITE, Kind::Pawn);
            }

            if self.no_redundant && !mate_position.hands().is_empty(Color::BLACK) {
                continue;
            }
            if (mate_position
                .bitboard(Color::WHITE, Kind::ProPawn)
                .u128()
                .count_ones() as u8)
                < self.no_less_pro_pawn
            {
                continue;
            }
            if self.pawn_maximally_constrained {
                if black_pawn.col_mask().count_ones() != frame.room.width() as u32 {
                    continue;
                }
                if white_pawn.col_mask().count_ones() != frame.room.width() as u32 {
                    continue;
                }
            }

            if !self.unique {
                mate_positions.push(mate_position);
                continue;
            }

            let mut variants = vec![];

            for king_pos in room {
                if has_parity
                    && king_pos.parity()
                        != representative
                            .bitboard(Color::WHITE, Kind::King)
                            .singleton()
                            .parity()
                {
                    continue;
                }

                let mut position = frame_position.clone();
                position.set(king_pos, Color::WHITE, Kind::King);
                position.hands_mut().add(Color::BLACK, self.attacker_kind);
                for _ in 0..min_pawn {
                    position.hands_mut().add(Color::WHITE, Kind::Pawn);
                }

                variants.push(position);
            }

            let mut unique = true;
            for mut position in variants {
                let Some(solution) = standard_solve(position.clone(), 1, true).unwrap().pop()
                else {
                    continue;
                };
                for m in solution {
                    position.do_move(&m);
                }
                if position.digest() != mate_position.digest() {
                    unique = false;
                    break;
                }
            }
            if unique {
                mate_positions.push(mate_position);
            }
        }

        mate_positions
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FrameFilter {
    room_filter: RoomFilter,
    no_black_pawn_count: Option<RangeInclusive<u8>>,
    no_white_pawn_count: Option<RangeInclusive<u8>>,
    mate_formation_filter: Option<MateFormationFilter>,
}

#[derive(Default, Clone, Debug)]
struct FrameMetadata {
    mate_with_minimum_pawn: Option<Vec<PositionAux>>,
}

fn frames(filter: &FrameFilter) -> Vec<(Frame, FrameMetadata)> {
    let rooms = rooms(&filter.room_filter);

    debug!("rooms: {}", rooms.len());

    let mut frames = vec![];

    for room in rooms {
        let w = room.width();

        let black_masks = pawn_masks(w, &filter.no_black_pawn_count);
        let white_masks = pawn_masks(w, &filter.no_white_pawn_count);

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
    let Some(mate_formation_filter) = &filter.mate_formation_filter else {
        return frames
            .into_iter()
            .map(|frame| (frame, FrameMetadata::default()))
            .collect();
    };

    let frames_len = frames.len();
    let frame_i = Mutex::new(0);

    frames
        .into_par_iter()
        .filter_map(|frame| {
            {
                let mut frame_i = frame_i.lock().unwrap();
                *frame_i += 1;
                if *frame_i % 1000 == 0 {
                    eprintln!("frame {}/{}", *frame_i, frames_len);
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

#[derive(Clone, Debug)]
struct Room {
    heights: Vec<u8>,
}

impl Room {
    fn weakly_decreasing(&self) -> bool {
        for i in 1..self.heights.len() {
            if self.heights[i - 1] < self.heights[i] {
                return false;
            }
        }
        true
    }

    fn area(&self) -> u8 {
        self.heights.iter().sum()
    }

    fn width(&self) -> u8 {
        self.heights.len() as u8
    }

    fn stone(&self) -> BitBoard {
        let mut stone = BitBoard::default();
        let max_height = self.heights.iter().copied().max().unwrap_or(0);

        for (i, h) in self.heights.iter().copied().enumerate() {
            for j in h..=max_height {
                stone.set(Square::new(i, (8 - j).into()));
            }
        }
        for j in 0..=max_height {
            stone.set(Square::new(self.heights.len(), (8 - j).into()));
        }
        stone
    }

    fn bitboard(&self) -> BitBoard {
        let mut res = BitBoard::default();
        for (i, h) in self.heights.iter().copied().enumerate() {
            for j in 0..h {
                res.set(Square::new(i, (8 - j).into()));
            }
        }
        res
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RoomFilter {
    width: Vec<u8>,
    height: RangeInclusive<u8>,
    weakly_decreasing: bool,
    area: Option<RangeInclusive<u8>>,
}

impl RoomFilter {
    fn unextensible(&self, room: &Room) -> bool {
        let max_width = *self.width.iter().max().unwrap();
        if room.heights.len() >= max_width as usize {
            return true;
        }
        if let Some(a) = &self.area {
            if *a.end() <= room.area() {
                return true;
            }
        }
        if self.weakly_decreasing && !room.weakly_decreasing() {
            return true;
        }
        false
    }

    fn matches(&self, room: &Room) -> bool {
        if !self.width.contains(&(room.heights.len() as u8)) {
            return false;
        }
        if !room.heights.iter().all(|&h| self.height.contains(&h)) {
            return false;
        }
        if let Some(area) = &self.area {
            if !area.contains(&room.area()) {
                return false;
            }
        }
        if self.weakly_decreasing && !room.weakly_decreasing() {
            return false;
        }
        true
    }
}

fn rooms(filter: &RoomFilter) -> Vec<Room> {
    let mut rooms = vec![];
    rooms_dfs(&mut Room { heights: vec![] }, filter, &mut rooms);
    rooms
}

fn rooms_dfs(room: &mut Room, filter: &RoomFilter, rooms: &mut Vec<Room>) {
    if filter.matches(room) {
        rooms.push(room.clone());
    }
    if filter.unextensible(room) {
        return;
    }

    for h in filter.height.clone() {
        room.heights.push(h);
        rooms_dfs(room, filter, rooms);
        room.heights.pop();
    }
}
