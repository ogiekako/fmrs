use crate::{
    piece::{Color, Kind},
    position::bitboard::{BitBoard, ColorBitBoard, Square},
};

use super::{gold_power, king_power, knight_power, magic, pawn_power, silver_power};

pub fn reachable(
    color_bb: &ColorBitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
    capture_same_color: bool,
) -> BitBoard {
    let exclude = if color.is_white() == capture_same_color {
        color_bb.black()
    } else {
        color_bb.white()
    };
    match kind {
        Kind::Lance => lance_reachable(color_bb.both(), color, pos),
        Kind::Bishop => magic::bishop_reachable(color_bb.both(), pos),
        Kind::Rook => rook_reachable(color_bb.both(), pos),
        Kind::ProBishop => king_power(pos) | magic::bishop_reachable(color_bb.both(), pos),
        Kind::ProRook => king_power(pos) | rook_reachable(color_bb.both(), pos),
        Kind::Gold | Kind::ProPawn | Kind::ProLance | Kind::ProKnight | Kind::ProSilver => {
            gold_power(color, pos)
        }
        Kind::Pawn => pawn_power(color, pos),
        Kind::Knight => knight_power(color, pos),
        Kind::Silver => silver_power(color, pos),
        Kind::King => king_power(pos),
    }
    .and_not(exclude)
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
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => magic::bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => king_power(pos) | magic::bishop_reachable(occupied, pos),
        Kind::ProRook => king_power(pos) | rook_reachable(occupied, pos),
        Kind::Pawn => pawn_power(color, pos),
        Kind::Knight => knight_power(color, pos),
        Kind::Silver => silver_power(color, pos),
        Kind::King => king_power(pos),
        _ => gold_power(color, pos),
    }
}

fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    magic::rook_reachable_row(occupied, pos) | rook_reachable_col(occupied, pos)
}

fn rook_reachable_col(occupied: BitBoard, pos: Square) -> BitBoard {
    let (occ, p, shift) = if pos.index() >= 63 {
        (
            (occupied.u128() >> 63) as u64,
            pos.index() as u64 - 63,
            true,
        )
    } else {
        (occupied.u128() as u64, pos.index() as u64, false)
    };
    let upper = if UPPER & 1 << p != 0 {
        0
    } else {
        let occ = (occ | UPPER) & ((1 << p) - 1);
        (1 << p) - (1 << u64::BITS - 1 - occ.leading_zeros())
    };
    let lower = if LOWER & 1 << p != 0 {
        0
    } else {
        let occ = occ | LOWER;
        occ ^ occ - (1 << p + 1)
    };
    let b = upper | lower;
    if shift {
        BitBoard::from_u128((b as u128) << 63)
    } else {
        BitBoard::from_u128(b as u128)
    }
}

const UPPER: u64 = 0b1000000001000000001000000001000000001000000001000000001000000001;
const LOWER: u64 = 0b100000000100000000100000000100000000100000000100000000100000000;

fn lance_reachable(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
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
            (1 << p) - (1 << u64::BITS - 1 - occ.leading_zeros())
        }
        Color::WHITE => {
            if LOWER >> p & 1 != 0 {
                return BitBoard::default();
            }
            let occ = occ | LOWER;
            occ ^ occ - (1 << p + 1)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(2, 3), Kind::Lance)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(2, 1), Kind::Lance)
        );
        assert_eq!(
            BitBoard::empty(),
            super::reachable_sub(occupied, Color::BLACK, Square::new(2, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::WHITE, Square::new(2, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::WHITE, Square::new(2, 1), Kind::Lance)
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
            super::reachable_sub(occupied, Color::WHITE, Square::new(2, 4), Kind::Lance)
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
            super::reachable_sub(occupied, Color::WHITE, Square::new(0, 0), Kind::Lance)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(0, 8), Kind::Lance)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(0, 0), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(1, 1), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(1, 2), Kind::Bishop)
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
            super::reachable_sub(occupied, Color::BLACK, Square::new(5, 1), Kind::Rook)
        );
    }
}