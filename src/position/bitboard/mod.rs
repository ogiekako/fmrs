mod bitboard;
mod color_bitboard;
mod rule;
mod square;

pub use bitboard::BitBoard;
pub use color_bitboard::ColorBitBoard;
pub use rule::power;
pub use rule::reachable;
pub use square::Square;

#[cfg(test)]
#[macro_use]
pub mod testing;
