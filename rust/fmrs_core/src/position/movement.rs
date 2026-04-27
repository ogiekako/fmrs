use std::hash::Hash;

use crate::{
    piece::{Kind, NUM_KIND},
    sfen,
};

use super::Square;

#[derive(Clone, Copy)]
pub enum Movement {
    Drop(Square, Kind),
    Move {
        source: Square,
        source_kind_hint: Option<Kind>,
        dest: Square,
        promote: bool,
        capture_kind_hint: Option<Option<Kind>>,
    },
}

impl Hash for Movement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Movement::Drop(pos, kind) => {
                1.hash(state);
                pos.index().hash(state);
                kind.hash(state);
            }
            Movement::Move {
                source,
                dest,
                promote,
                ..
            } => {
                2.hash(state);
                source.index().hash(state);
                dest.index().hash(state);
                promote.hash(state);
            }
        }
    }
}

impl PartialOrd for Movement {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Movement {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Movement::Drop(pos1, kind1), Movement::Drop(pos2, kind2)) => {
                pos1.cmp(pos2).then_with(|| kind1.cmp(kind2))
            }
            (
                Movement::Move {
                    source: source1,
                    dest: dest1,
                    promote: promote1,
                    ..
                },
                Movement::Move {
                    source: source2,
                    dest: dest2,
                    promote: promote2,
                    ..
                },
            ) => source1
                .cmp(source2)
                .then_with(|| dest1.cmp(dest2))
                .then_with(|| promote1.cmp(promote2)),
            (Movement::Drop(_, _), Movement::Move { .. }) => std::cmp::Ordering::Less,
            (Movement::Move { .. }, Movement::Drop(_, _)) => std::cmp::Ordering::Greater,
        }
    }
}

impl Eq for Movement {}

impl PartialEq for Movement {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Movement::Drop(pos1, kind1), Movement::Drop(pos2, kind2)) => {
                pos1 == pos2 && kind1 == kind2
            }
            (
                Movement::Move {
                    source: source1,
                    dest: dest1,
                    promote: promote1,
                    ..
                },
                Movement::Move {
                    source: source2,
                    dest: dest2,
                    promote: promote2,
                    ..
                },
            ) => source1 == source2 && dest1 == dest2 && promote1 == promote2,
            _ => false,
        }
    }
}

impl Movement {
    pub(crate) fn move_without_hint(source: Square, dest: Square, promote: bool) -> Movement {
        Movement::Move {
            source,
            source_kind_hint: None,
            dest,
            promote,
            capture_kind_hint: None,
        }
    }
    pub(crate) fn move_with_hint(
        source: Square,
        source_kind_hint: Kind,
        dest: Square,
        promote: bool,
        capture_kind_hint: Option<Kind>,
    ) -> Movement {
        Movement::Move {
            source,
            source_kind_hint: Some(source_kind_hint),
            dest,
            promote,
            capture_kind_hint: Some(capture_kind_hint),
        }
    }

    pub fn is_move(&self) -> bool {
        matches!(self, Movement::Move { .. })
    }

    pub fn is_pawn_drop(&self) -> bool {
        matches!(self, Movement::Drop(_, Kind::Pawn))
    }

    pub fn flipped(&self) -> Movement {
        match *self {
            Movement::Drop(pos, kind) => Movement::Drop(pos.flipped(), kind),
            Movement::Move {
                source,
                source_kind_hint,
                dest,
                promote,
                capture_kind_hint,
            } => Movement::Move {
                source: source.flipped(),
                source_kind_hint,
                dest: dest.flipped(),
                promote,
                capture_kind_hint,
            },
        }
    }
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", sfen::encode_move(self))
    }
}

#[derive(Clone)]
pub(crate) struct MovementSet {
    bits: [u64; Self::LIMBS],
}

impl Default for MovementSet {
    fn default() -> Self {
        Self {
            bits: [0; Self::LIMBS],
        }
    }
}

impl MovementSet {
    const SQUARES: usize = 81;
    const MOVE_KEYS: usize = Self::SQUARES * Self::SQUARES * 2;
    const DROP_KEYS: usize = Self::SQUARES * NUM_KIND;
    const KEYS: usize = Self::MOVE_KEYS + Self::DROP_KEYS;
    const LIMBS: usize = Self::KEYS.div_ceil(u64::BITS as usize);

    pub(crate) fn contains(&self, movement: &Movement) -> bool {
        let key = Self::key(movement);
        self.bits[key / 64] & (1 << (key % 64)) != 0
    }

    pub(crate) fn insert(&mut self, movement: Movement) {
        let key = Self::key(&movement);
        self.bits[key / 64] |= 1 << (key % 64);
    }

    fn key(movement: &Movement) -> usize {
        match *movement {
            Movement::Drop(pos, kind) => Self::MOVE_KEYS + pos.index() * NUM_KIND + kind.index(),
            Movement::Move {
                source,
                dest,
                promote,
                ..
            } => (source.index() * Self::SQUARES + dest.index()) * 2 + promote as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_set_uses_movement_equality() {
        let mut seen = MovementSet::default();
        let movement = Movement::move_with_hint(Square::S11, Kind::Pawn, Square::S12, false, None);
        let same_without_hint = Movement::move_without_hint(Square::S11, Square::S12, false);
        let different_promote = Movement::move_without_hint(Square::S11, Square::S12, true);

        seen.insert(movement);

        assert!(seen.contains(&same_without_hint));
        assert!(!seen.contains(&different_promote));
    }
}
