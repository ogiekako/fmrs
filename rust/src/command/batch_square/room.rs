use std::ops::RangeInclusive;

use fmrs_core::position::{BitBoard, Square};
use serde::{Deserialize, Serialize};

use super::csp::Constraint;

#[derive(Clone, Debug, PartialEq, Eq, Default, Hash)]
pub(super) struct Room {
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

    pub(super) fn width(&self) -> u8 {
        self.heights.len() as u8
    }

    pub(super) fn stone(&self) -> BitBoard {
        let mut stone = BitBoard::default();

        for (i, h) in self.heights.iter().copied().enumerate() {
            let mut necessary_height = h + 1;
            if i > 0 {
                necessary_height = necessary_height.max(self.heights[i - 1] + 1);
            }
            if i < self.heights.len() - 1 {
                necessary_height = necessary_height.max(self.heights[i + 1] + 1);
            }
            for j in h..necessary_height {
                stone.set(Square::new(i, (8 - j).into()));
            }
        }
        if self.heights.len() < 9 {
            let necessary_height = self.heights.last().copied().unwrap_or(0) + 1;
            for j in 0..necessary_height {
                stone.set(Square::new(self.heights.len(), (8 - j).into()));
            }
        }
        stone
    }

    pub(super) fn bitboard(&self) -> BitBoard {
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
pub(super) struct RoomFilter {
    pub(super) width: Vec<u8>,
    pub(super) height: RangeInclusive<u8>,
    pub(super) weakly_decreasing: bool,
    pub(super) area: Option<RangeInclusive<u8>>,
    pub(super) feasible_without_stone: bool,
}

impl RoomFilter {
    pub(super) fn unextensible(&self, room: &Room) -> bool {
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

    pub(super) fn matches(&self, room: &Room) -> bool {
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
        if self.feasible_without_stone
            && (Constraint {
                uncaptureable_black: room.stone(),
                uncheckable: room.bitboard(),
                can_use_king: false,
            })
            .search()
            .is_none()
        {
            return false;
        }
        true
    }

    pub(super) fn generate_rooms(&self) -> Vec<Room> {
        let mut rooms = vec![];
        self.dfs(&mut Room::default(), &mut rooms);
        rooms
    }

    fn dfs(&self, room: &mut Room, rooms: &mut Vec<Room>) {
        if self.matches(room) {
            rooms.push(room.clone());
        }
        if self.unextensible(room) {
            return;
        }

        for h in self.height.clone() {
            room.heights.push(h);
            self.dfs(room, rooms);
            room.heights.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RoomFilter;

    #[test]
    fn test_feasible_without_stone() {
        let filter = RoomFilter {
            width: vec![3],
            height: 1..=3,
            area: Some(0..=7),
            weakly_decreasing: false,
            feasible_without_stone: true,
        };
        let rooms = filter.generate_rooms();
        // 1 1 1, 1 1 2, 1 1 3, 1 2 1, 1 2 2, 1 2 3
        // 2 1 1, 2 1 2, 2 1 3, 2 2 1, 2 2 2, 2 2 3, 2 3 1, 2 3 2
        // 3 1 1, 3 1 2, 3 2 1, 3 2 2, 3 3 1
        assert_eq!(rooms.len(), 19);
    }
}
