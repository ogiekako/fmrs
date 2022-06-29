use crate::{piece::Kind, sfen};

use super::Square;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Movement {
    Drop(Square, Kind),
    Move {
        from: Square,
        to: Square,
        promote: bool,
    },
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", sfen::encode_move(self))
    }
}
