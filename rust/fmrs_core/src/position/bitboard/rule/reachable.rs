use crate::{
    piece::{Color, EssentialKind},
    position::bitboard::{BitBoard, ColorBitBoard, Square},
};

use super::{essential_power, magic};

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
        EssentialKind::ProBishop => {
            essential_power(color, pos, EssentialKind::King)
                | &magic::bishop_reachable(occupied, pos)
        }
        EssentialKind::ProRook => {
            essential_power(color, pos, EssentialKind::King) | &rook_reachable(occupied, pos)
        }
        _ => *essential_power(color, pos, ek),
    }
}

fn rook_reachable(occupied: &BitBoard, pos: Square) -> BitBoard {
    magic::rook_reachable_row(occupied, pos)
        | lance_reachable(occupied, Color::Black, pos)
        | lance_reachable(occupied, Color::White, pos)
}

#[inline(never)]
fn lance_reachable(occupied: &BitBoard, color: Color, pos: Square) -> BitBoard {
    let power = essential_power(color, pos, EssentialKind::Lance);
    let block = (occupied & power).u128();
    if block == 0 {
        return *power;
    }
    match color {
        Color::Black => power.and_not(BitBoard::from_u128(black_lance_unreachable(block))),
        Color::White => &BitBoard::from_u128((block - 1) ^ block) & power,
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
