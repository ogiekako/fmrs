use crate::{
    config::CONFIG,
    piece::{Color, Kind},
    position::{
        bitboard::{generated_magics, BitBoard, Square},
        position::PositionAux,
    },
};

use super::{power, ColorKind};

pub fn reachable(
    position: &PositionAux,
    color: Color,
    pos: Square,
    kind: Kind,
    capture_same_color: bool,
) -> BitBoard {
    let exclude = if color.is_white() ^ capture_same_color {
        position.color_bb_and_stone(Color::WHITE)
    } else {
        position.color_bb_and_stone(Color::BLACK)
    };
    reachable_sub(position, color, pos, kind).and_not(exclude)
}

pub fn reachable_core(occupied: BitBoard, color: Color, pos: Square, kind: Kind) -> BitBoard {
    if !kind.is_line_piece() {
        return power(color, pos, kind);
    }
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => pro_bishop_reachable(occupied, pos),
        Kind::ProRook => pro_rook_reachable(occupied, pos),
        _ => unreachable!(),
    }
}

pub fn reachable_sub(position: &PositionAux, color: Color, pos: Square, kind: Kind) -> BitBoard {
    if !kind.is_line_piece() {
        return power(color, pos, kind);
    }
    let occupied = position.occupied_bb();

    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => pro_bishop_reachable(occupied, pos),
        Kind::ProRook => pro_rook_reachable(occupied, pos),
        _ => unreachable!(),
    }
}

fn pro_bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    const F: fn(BitBoard, Square) -> BitBoard = [
        |occupied, pos| bishop_reachable(occupied, pos) | ColorKind::King.power(pos),
        generated_magics::pro_bishop_reachable,
    ][CONFIG.use_bishop_magic as usize];

    F(occupied, pos)
}

fn pro_rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    const F: fn(BitBoard, Square) -> BitBoard = [
        |occupied, pos| rook_reachable(occupied, pos) | ColorKind::King.power(pos),
        generated_magics::pro_rook_reachable,
    ][CONFIG.use_rook_magic as usize];

    F(occupied, pos)
}

pub fn lance_reachable(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
    const F: fn(BitBoard, Color, Square) -> BitBoard =
        [lance_reachable_no_magic, generated_magics::lance_reachable]
            [CONFIG.use_lance_magic as usize];
    F(occupied, color, pos)
}

pub fn bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    const F: fn(BitBoard, Square) -> BitBoard = [
        bishop_reachable_no_magic,
        generated_magics::bishop_reachable,
    ][CONFIG.use_bishop_magic as usize];
    F(occupied, pos)
}

pub fn bishop_reachable_no_magic(occupied: BitBoard, pos: Square) -> BitBoard {
    let power1 = ColorKind::RuLd.power(pos);
    let power2 = ColorKind::RdLu.power(pos);
    (power1.u128() & (power1 & occupied).surrounding(pos)
        | power2.u128() & (power2 & occupied).surrounding(pos))
    .into()
}

pub fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    const F: fn(BitBoard, Square) -> BitBoard =
        [rook_reachable_no_magic, generated_magics::rook_reachable][CONFIG.use_rook_magic as usize];
    F(occupied, pos)
}

pub fn rook_reachable_no_magic(occupied: BitBoard, pos: Square) -> BitBoard {
    let power1 = ColorKind::RL.power(pos);
    let power2 = ColorKind::UD.power(pos);
    (power1.u128() & (power1 & occupied).surrounding(pos)
        | power2.u128() & (power2 & occupied).surrounding(pos))
    .into()
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
        Kind::Bishop => bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => pro_bishop_reachable(occupied, pos),
        Kind::ProRook => pro_rook_reachable(occupied, pos),
        _ => unreachable!(),
    }
    .and_not(uncapturable)
}

const UPPER: u64 = 0b1000000001000000001000000001000000001000000001000000001000000001;
const LOWER: u64 = 0b100000000100000000100000000100000000100000000100000000100000000;

pub fn lance_reachable_no_magic(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
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
        bitboard,
        piece::{Color, Kind},
        position::bitboard::{BitBoard, Square},
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
