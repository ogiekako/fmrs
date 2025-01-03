use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{BitBoard, Square},
        position::PositionAux,
    },
};

use super::{magic, power};

pub fn reachable(
    position: &mut PositionAux,
    color: Color,
    pos: Square,
    kind: Kind,
    capture_same_color: bool,
) -> BitBoard {
    let exclude = if color.is_white() == capture_same_color {
        position.black_bb()
    } else {
        position.white_bb()
    };
    reachable_sub(position, color, pos, kind).and_not(exclude)
}

pub fn reachable_sub(
    position: &mut PositionAux,
    color: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    if !kind.is_line_piece() {
        return power(color, pos, kind);
    }
    let occupied = position.occupied_bb();
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => magic::bishop_reachable(occupied, pos),
        Kind::Rook => magic::rook_reachable(occupied, pos),
        Kind::ProBishop => magic::probishop_reachable(occupied, pos),
        Kind::ProRook => magic::prorook_reachable(occupied, pos),
        _ => unreachable!(),
    }
}

pub fn reachable2(
    capturable: BitBoard,
    uncapturable: BitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    if !kind.is_line_piece() {
        return power(color, pos, kind).and_not(uncapturable);
    }
    let occupied = capturable | uncapturable;
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => magic::bishop_reachable(occupied, pos),
        Kind::Rook => magic::rook_reachable(occupied, pos),
        Kind::ProBishop => magic::probishop_reachable(occupied, pos),
        Kind::ProRook => magic::prorook_reachable(occupied, pos),
        _ => unreachable!(),
    }
    .and_not(uncapturable)
}

const UPPER: u64 = 0b1000000001000000001000000001000000001000000001000000001000000001;
const LOWER: u64 = 0b100000000100000000100000000100000000100000000100000000100000000;

pub fn lance_reachable(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
    let (occ, p, shift) = if pos.index() >= 63 {
        ((occupied.u128() >> 63) as u64, pos.index() - 63, true)
    } else {
        (occupied.u128() as u64, pos.index(), false)
    };
    let b = match color {
        Color::BLACK => {
            if UPPER & 1 << p != 0 {
                return BitBoard::default();
            }
            let occ = (occ | UPPER) & ((1 << p) - 1);
            (1 << p) - (1 << (u64::BITS - 1 - occ.leading_zeros()))
        }
        Color::WHITE => {
            if LOWER >> p & 1 != 0 {
                return BitBoard::default();
            }
            let occ = occ | LOWER;
            occ ^ (occ - (1 << (p + 1)))
        }
    };
    if shift {
        BitBoard::from_u128((b as u128) << 63)
    } else {
        BitBoard::from_u128(b as u128)
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(2, 3),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(2, 1),
                Kind::Lance
            )
        );
        assert_eq!(
            BitBoard::default(),
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(2, 0),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::WHITE,
                Square::new(2, 0),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::WHITE,
                Square::new(2, 1),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::WHITE,
                Square::new(2, 4),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::WHITE,
                Square::new(0, 0),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(0, 8),
                Kind::Lance
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(0, 0),
                Kind::Bishop
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(1, 1),
                Kind::Bishop
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(1, 2),
                Kind::Bishop
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
            super::reachable2(
                occupied,
                BitBoard::default(),
                Color::BLACK,
                Square::new(5, 1),
                Kind::Rook
            )
        );
    }
}
