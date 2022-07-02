use crate::piece::{Color, Kind};

use super::super::{BitBoard, Square};

pub fn power(color: Color, pos: Square, kind: Kind) -> BitBoard {
    match kind {
        Kind::King => non_line_power(*KING_ATTACK00, pos),
        Kind::Pawn => non_line_power(PAWN_ATTACK00[color.index()], pos),
        Kind::Knight => non_line_power(KNIGHT_ATTACK00[color.index()], pos),
        Kind::Silver => non_line_power(SILVER_ATTACK00[color.index()], pos),
        Kind::Gold | Kind::ProPawn | Kind::ProLance | Kind::ProKnight | Kind::ProSilver => {
            non_line_power(GOLD_ATTACK00[color.index()], pos)
        }
        Kind::Lance => lance_power(color, pos),
        Kind::Rook => rook_power(pos),
        Kind::ProRook => rook_power(pos) | non_line_power(*KING_ATTACK00, pos),
        Kind::Bishop => bishop_power(pos),
        Kind::ProBishop => bishop_power(pos) | non_line_power(*KING_ATTACK00, pos),
    }
}

pub(super) fn bishop_power(pos: Square) -> BitBoard {
    BitBoard::from_u128(SAME_DIAG1[diag1(pos)] ^ SAME_DIAG2[diag2(pos)])
}

fn diag1(pos: Square) -> usize {
    pos.col() + pos.row()
}

fn diag2(pos: Square) -> usize {
    pos.col() + 8 - pos.row()
}

lazy_static! {
    static ref SAME_DIAG1: [u128; 17] = {
        let mut res = [0; 17];
        Square::iter().for_each(|pos| res[diag1(pos)] |= 1 << pos.index());
        res
    };
    static ref SAME_DIAG2: [u128; 17] = {
        let mut res = [0; 17];
        Square::iter().for_each(|pos| res[diag2(pos)] |= 1 << pos.index());
        res
    };
}

pub(super) fn rook_power(pos: Square) -> BitBoard {
    BitBoard::from_u128(SAME_COLUMN[pos.col()] ^ SAME_ROW[pos.row()])
}

lazy_static! {
    static ref SAME_COLUMN: [u128; 9] = {
        let mut res = [0; 9];
        for col in 0..9 {
            for row in 0..9 {
                res[col] |= 1 << Square::new(col, row).index()
            }
        }
        res
    };
    static ref SAME_ROW: [u128; 9] = {
        let mut res = [0; 9];
        for row in 0..9 {
            for col in 0..9 {
                res[row] |= 1 << Square::new(col, row).index()
            }
        }
        res
    };
}

pub(super) fn lance_power(color: Color, pos: Square) -> BitBoard {
    if color == Color::Black {
        let pos_bb = 1u128 << pos.index();
        BitBoard::from_u128(pos_bb - (pos_bb >> pos.row()))
    } else {
        let pos_bb = 1u128 << pos.index() + 1;
        BitBoard::from_u128((pos_bb << 8 - pos.row()) - pos_bb)
    }
}

fn non_line_power(attack00: u128, pos: Square) -> BitBoard {
    BitBoard::from_u128(attack00 << shift_usize(pos.col(), pos.row()))
}

lazy_static! {
    static ref PAWN_ATTACK00: [u128; 2] = power00(&[(0, -1)]);
    static ref KNIGHT_ATTACK00: [u128; 2] = power00(&[(-1, -2), (1, -2)]);
    static ref SILVER_ATTACK00: [u128; 2] = power00(&[(-1, -1), (-1, 1), (0, -1), (1, -1), (1, 1)]);
    static ref GOLD_ATTACK00: [u128; 2] =
        power00(&[(-1, -1), (-1, 0), (0, -1), (0, 1), (1, -1), (1, 0)]);
    static ref KING_ATTACK00: u128 = SILVER_ATTACK00[0] | GOLD_ATTACK00[0];
}

fn power00(black_shifts: &[(isize, isize)]) -> [u128; 2] {
    let black = power00_sub(black_shifts.into_iter().map(|(col, row)| shift(*col, *row)));
    let white = power00_sub(black_shifts.into_iter().map(|(col, row)| shift(*col, -row)));
    [black, white]
}

fn power00_sub(shifts: impl Iterator<Item = isize>) -> u128 {
    let base = 1u128 << Square::new(0, 0).index();
    let mut res = 0;
    for shift in shifts {
        res |= if shift < 0 {
            base >> (-shift) as usize
        } else {
            base << shift as usize
        };
    }
    res
}

fn shift(col: isize, row: isize) -> isize {
    col * 11 + row
}

fn shift_usize(col: usize, row: usize) -> usize {
    col * 11 + row
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::{Color, Kind},
        position::bitboard11::testing::bitboard,
        position::bitboard11::Square,
    };

    #[test]
    fn power() {
        assert_eq!(
            bitboard!(
                ".........",
                "......***",
                ".........",
                "......*.*",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::power(Color::Black, Square::new(1, 2), Kind::Silver)
        );
        assert_eq!(
            bitboard!(
                ".........",
                "........*",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::power(Color::White, Square::new(0, 0), Kind::Pawn)
        );
        assert_eq!(
            bitboard!(
                ".......*.",
                ".......**",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::power(Color::White, Square::new(0, 0), Kind::ProSilver)
        );
        assert_eq!(
            bitboard!(
                ".........",
                "......***",
                "......*.*",
                "......***",
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ),
            super::power(Color::Black, Square::new(1, 2), Kind::King)
        );
        assert_eq!(
            bitboard!(
                ".......*.",
                "......***",
                "*******.*",
                "......***",
                ".......*.",
                ".......*.",
                ".......*.",
                ".......*.",
                ".......*.",
            ),
            super::power(Color::Black, Square::new(1, 2), Kind::ProRook)
        );
        assert_eq!(
            bitboard!(
                "********.",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
                "........*",
            ),
            super::power(Color::Black, Square::new(0, 0), Kind::Rook)
        );
        assert_eq!(
            bitboard!(
                ".........",
                ".........",
                ".........",
                ".......*.",
                ".......*.",
                ".......*.",
                ".......*.",
                ".......*.",
                ".......*.",
            ),
            super::power(Color::White, Square::new(1, 2), Kind::Lance)
        );
        assert_eq!(
            bitboard!(
                "....*....",
                ".....*...",
                "......*.*",
                ".........",
                "......*.*",
                ".....*...",
                "....*....",
                "...*.....",
                "..*......",
            ),
            super::power(Color::White, Square::new(1, 3), Kind::Bishop)
        );
    }
}
