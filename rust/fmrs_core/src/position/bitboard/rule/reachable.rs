use crate::{
    piece::{Color, Kind},
    position::bitboard::{BitBoard, Square},
};

use super::{magic, power::power};

pub fn reachable(
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    let mask = reachable_sub(black_pieces | white_pieces, color, pos, kind);
    match color {
        Color::Black => mask.and_not(black_pieces),
        Color::White => mask.and_not(white_pieces),
    }
}

pub fn reachable2(
    capturable: BitBoard,
    uncapturable: BitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    let mask = reachable_sub(capturable | uncapturable, color, pos, kind);
    mask.and_not(uncapturable)
}

fn reachable_sub(occupied: BitBoard, color: Color, pos: Square, kind: Kind) -> BitBoard {
    if !kind.is_line_piece() {
        return power(color, pos, kind);
    }
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => magic::bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => power(color, pos, Kind::King) | magic::bishop_reachable(occupied, pos),
        Kind::ProRook => power(color, pos, Kind::King) | rook_reachable(occupied, pos),
        _ => unreachable!(),
    }
}

fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    magic::rook_reachable_row(occupied, pos)
        | lance_reachable(occupied, Color::Black, pos)
        | lance_reachable(occupied, Color::White, pos)
}

#[inline(never)]
fn lance_reachable(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
    let power = power(color, pos, Kind::Lance);
    let block = (occupied & power).u128();
    if block == 0 {
        return power;
    }
    match color {
        Color::Black => power.and_not(BitBoard::from_u128(black_lance_unreachable(block))),
        Color::White => BitBoard::from_u128((block - 1) ^ block) & power,
    }
}

fn black_lance_unreachable(mut block: u128) -> u128 {
    block |= block >> 1;
    block |= block >> 2;
    block |= block >> 4;
    block >> 1
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
            super::reachable_sub(occupied, Color::Black, Square::new(2, 3), Kind::Lance)
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
            super::reachable_sub(occupied, Color::Black, Square::new(2, 1), Kind::Lance)
        );
        assert_eq!(
            BitBoard::empty(),
            super::reachable_sub(occupied, Color::Black, Square::new(2, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::White, Square::new(2, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::White, Square::new(2, 1), Kind::Lance)
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
            super::reachable_sub(occupied, Color::White, Square::new(2, 4), Kind::Lance)
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
            super::reachable_sub(occupied, Color::White, Square::new(0, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::Black, Square::new(0, 8), Kind::Lance)
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
            super::reachable_sub(occupied, Color::Black, Square::new(0, 0), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::Black, Square::new(1, 1), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::Black, Square::new(1, 2), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::Black, Square::new(5, 1), Kind::Rook)
        );
    }
}
