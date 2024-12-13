mod bitboard;
mod color_bitboard;
mod kind_bitboard;
pub mod rule;
mod square;

pub use bitboard::BitBoard;
pub use color_bitboard::ColorBitBoard;
pub use kind_bitboard::KindBitBoard;
pub use rule::*;
pub use square::Square;

#[cfg(test)]
#[macro_use]
pub mod testing;
