use crate::{piece::Kind, sfen};

use super::Square;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", sfen::encode_move(self))
    }
}
