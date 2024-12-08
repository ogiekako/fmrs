use crate::{
    bits::highest_one_bit,
    piece::{Color, EssentialKind},
    position::bitboard::{BitBoard, ColorBitBoard, Square},
};

use super::{king_power, magic, power};

pub fn reachable(
    color_bb: &ColorBitBoard,
    color: Color,
    pos: Square,
    ek: EssentialKind,
    capture_same_color: bool,
) -> BitBoard {
    let mask = reachable_sub(color_bb.both(), color, pos, ek);
    mask.and_not(*color_bb.bitboard(match capture_same_color {
        true => color.opposite(),
        false => color,
    }))
}

fn reachable_sub(occupied: &BitBoard, color: Color, pos: Square, ek: EssentialKind) -> BitBoard {
    match ek {
        EssentialKind::Lance => lance_reachable(occupied, color, pos),
        EssentialKind::Bishop => magic::bishop_reachable(occupied, pos),
        EssentialKind::Rook => rook_reachable(occupied, pos),
        EssentialKind::ProBishop => king_power(pos) | &magic::bishop_reachable(occupied, pos),
        EssentialKind::ProRook => king_power(pos) | &rook_reachable(occupied, pos),
        _ => *power(color, pos, ek),
    }
}

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
        piece::{Color, EssentialKind},
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(2, 3),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(2, 1),
                EssentialKind::Lance
            )
        );
        assert_eq!(
            BitBoard::default(),
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(2, 0),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::White,
                Square::new(2, 0),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::White,
                Square::new(2, 1),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::White,
                Square::new(2, 4),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::White,
                Square::new(0, 0),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(0, 8),
                EssentialKind::Lance
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(0, 0),
                EssentialKind::Bishop
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(1, 1),
                EssentialKind::Bishop
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(1, 2),
                EssentialKind::Bishop
            )
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
            super::reachable_sub(
                &occupied,
                Color::Black,
                Square::new(5, 1),
                EssentialKind::Rook
            )
        );
    }
}
