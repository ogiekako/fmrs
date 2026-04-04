use crate::piece::{Color, Kind};
use super::Square;

include!("zobrist_data.rs");

#[inline]
pub(crate) fn zobrist(color: Color, pos: Square, kind: Kind) -> u64 {
    M[pos.index() << 5 | color.index() << 4 | kind.index()]
}

#[inline]
pub(crate) fn zobrist_stone(pos: Square) -> u64 {
    M[pos.index() << 5 | 15]
}
