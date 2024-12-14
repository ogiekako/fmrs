use crate::{piece::Kind, sfen};

use super::Square;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", sfen::encode_move(self))
    }
}
