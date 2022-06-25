use crate::{board::Square, piece::Kind, sfen};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
