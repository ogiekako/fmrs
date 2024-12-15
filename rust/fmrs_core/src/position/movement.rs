use crate::{piece::Kind, sfen};

use super::Square;

#[derive(Clone, Copy, PartialOrd, Ord)]
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

    pub(crate) fn dest(&self) -> Square {
        match self {
            Movement::Drop(pos, _) => *pos,
            Movement::Move { dest, .. } => *dest,
        }
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
