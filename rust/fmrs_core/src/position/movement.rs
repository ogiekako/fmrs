use std::hash::Hash;

use crate::{piece::Kind, sfen};

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
                0.hash(state);
                pos.index().hash(state);
                kind.hash(state);
            }
            Movement::Move {
                source,
                dest,
                promote,
                ..
            } => {
                1.hash(state);
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
                .then_with(|| promote1.cmp(&promote2)),
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
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", sfen::encode_move(self))
    }
}
