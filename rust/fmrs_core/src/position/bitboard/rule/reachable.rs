use crate::{
    bits::highest_one_bit,
    piece::{Color, Kind},
    position::bitboard::{BitBoard, ColorBitBoard, Square},
};

use super::{king_power, magic, power};

pub fn reachable(
    color_bb: &ColorBitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
    capture_same_color: bool,
) -> BitBoard {
    let mask = reachable_sub(color_bb.both(), color, pos, kind);
    mask.and_not(*color_bb.bitboard(match capture_same_color {
        true => color.opposite(),
        false => color,
    }))
}

const LINE_PIECE_MASK: usize = 1 << Kind::Lance.index()
    | 1 << Kind::Bishop.index()
    | 1 << Kind::Rook.index()
    | 1 << Kind::ProBishop.index()
    | 1 << Kind::ProRook.index();

// #[inline(never)]
fn reachable_sub(occupied: &BitBoard, color: Color, pos: Square, kind: Kind) -> BitBoard {
    if LINE_PIECE_MASK >> kind.index() & 1 == 0 {
        return power(color, pos, kind);
    }
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => magic::bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => king_power(pos) | magic::bishop_reachable(occupied, pos),
        Kind::ProRook => king_power(pos) | rook_reachable(occupied, pos),
        _ => unreachable!(),
    }
}

// #[inline(never)]
fn rook_reachable(occupied: &BitBoard, pos: Square) -> BitBoard {
    magic::rook_reachable_row(occupied, pos)
        | lance_reachable(occupied, Color::Black, pos)
        | lance_reachable(occupied, Color::White, pos)
}

const UPPER: BitBoard = BitBoard::from_u128(
    0b1000000001000000001000000001000000001000000001000000001000000001000000001u128,
);
const LOWER: BitBoard = BitBoard::from_u128(
    0b100000000100000000100000000100000000100000000100000000100000000100000000100000000u128,
);

// #[inline(never)]
fn lance_reachable(occupied: &BitBoard, color: Color, pos: Square) -> BitBoard {
    match color {
        Color::Black => {
            if UPPER.get(pos) {
                return BitBoard::default();
            }
            let occ = (occupied | &UPPER).u128() & ((1 << pos.index()) - 1);
            BitBoard::from_u128((1 << pos.index()) - highest_one_bit(occ))
        }
        Color::White => {
            if LOWER.get(pos) {
                return BitBoard::default();
            }
            let occ = (occupied | &LOWER).u128();
            BitBoard::from_u128(occ ^ occ - (1 << pos.index() + 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::{Color, Kind},
        position::bitboard::{testing::bitboard, BitBoard, Square},
    };

    #[test]
    fn test_lance_movable_positions() {
        let occupied = bitboard!(
            ".........",
            "......*..",
            ".........",
            ".........",
            "......*..",
            ".........",
            ".........",
            ".........",
            ".........",
        );
        assert_eq!(
            bitboard!(
                ".........",
                "......*..",
                "......*..",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(2, 3), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                "......*..",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(2, 1), Kind::Lance)
        );
        assert_eq!(
            BitBoard::default(),
            super::reachable_sub(&occupied, Color::Black, Square::new(2, 0), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                ".........",
                "......*..",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::White, Square::new(2, 0), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                ".........",
                ".........",
                "......*..",
                "......*..",
                "......*..",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::White, Square::new(2, 1), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                "......*..",
                "......*..",
                "......*..",
                "......*..",
            ),
            super::reachable_sub(&occupied, Color::White, Square::new(2, 4), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                ".........",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
            ),
            super::reachable_sub(&occupied, Color::White, Square::new(0, 0), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(0, 8), Kind::Lance)
        );
    }
    #[test]
    fn test_bishop_movable_positions() {
        let occupied = bitboard!(
            ".........",
            "......*..",
            ".........",
            ".........",
            "......*..",
            "...*.....",
            "..*......",
            ".........",
            ".........",
        );
        assert_eq!(
            bitboard!(
                ".........",
                ".......*.",
                "......*..",
                ".....*...",
                "....*....",
                "...*.....",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(0, 0), Kind::Bishop)
        );
        assert_eq!(
            bitboard!(
                "......*.*",
                ".........",
                "......*.*",
                ".....*...",
                "....*....",
                "...*.....",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(1, 1), Kind::Bishop)
        );
        assert_eq!(
            bitboard!(
                ".........",
                "......*.*",
                ".........",
                "......*.*",
                ".....*...",
                "....*....",
                "...*.....",
                "..*......",
                ".*.......",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(1, 2), Kind::Bishop)
        );
    }

    #[test]
    fn test_rook_movable_positions() {
        let occupied = bitboard!(
            ".........",
            "......*..",
            ".........",
            ".........",
            "......*..",
            "...*.....",
            "..*......",
            ".........",
            ".........",
        );
        assert_eq!(
            bitboard!(
                "...*.....",
                "***.***..",
                "...*.....",
                "...*.....",
                "...*.....",
                "...*.....",
                ".........",
                ".........",
                ".........",
            ),
            super::reachable_sub(&occupied, Color::Black, Square::new(5, 1), Kind::Rook)
        );
    }
}
