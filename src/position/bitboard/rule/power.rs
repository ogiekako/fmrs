use crate::piece::{Color, Kind};

use super::super::{BitBoard, Square};

#[inline(never)]
pub fn power(color: Color, pos: Square, kind: Kind) -> BitBoard {
    match kind {
        Kind::King => king_power(pos),
        Kind::Pawn => PAWN_POWER[color.index()][pos.index()],
        Kind::Knight => KNIGHT_POWER[color.index()][pos.index()],
        Kind::Silver => SILVER_POWER[color.index()][pos.index()],
        Kind::Gold | Kind::ProPawn | Kind::ProLance | Kind::ProKnight | Kind::ProSilver => {
            GOLD_POWER[color.index()][pos.index()]
        }
        Kind::Lance => lance_power(color, pos),
        Kind::Rook => rook_power(pos),
        Kind::ProRook => rook_power(pos) | king_power(pos),
        Kind::Bishop => bishop_power(pos),
        Kind::ProBishop => bishop_power(pos) | king_power(pos),
    }
}

pub(super) fn king_power(pos: Square) -> BitBoard {
    KING_POWER[pos.index()]
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
    LANCE_POWER[color.index()][pos.index()]
}

lazy_static! {
    static ref PAWN_POWER: [[BitBoard; 81]; 2] = powers(&[(0, -1)]);
    static ref KNIGHT_POWER: [[BitBoard; 81]; 2] = powers(&[(-1, -2), (1, -2)]);
    static ref SILVER_POWER: [[BitBoard; 81]; 2] =
        powers(&[(-1, -1), (-1, 1), (0, -1), (1, -1), (1, 1)]);
    static ref GOLD_POWER: [[BitBoard; 81]; 2] =
        powers(&[(-1, -1), (-1, 0), (0, -1), (0, 1), (1, -1), (1, 0)]);
    static ref KING_POWER: [BitBoard; 81] = powers_sub(
        [
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1)
        ]
        .into_iter()
    );
    static ref LANCE_POWER: [[BitBoard; 81]; 2] = powers(&[
        (0, -1),
        (0, -2),
        (0, -3),
        (0, -4),
        (0, -5),
        (0, -6),
        (0, -7),
        (0, -8)
    ]);
}

fn powers(black_shifts: &[(isize, isize)]) -> [[BitBoard; 81]; 2] {
    let black = powers_sub(black_shifts.iter().map(|(col, row)| (*col, *row)));
    let white = powers_sub(black_shifts.iter().map(|(col, row)| (*col, -row)));
    [black, white]
}

fn powers_sub(shifts: impl Iterator<Item = (isize, isize)>) -> [BitBoard; 81] {
    let shifts = shifts.collect::<Vec<_>>();
    let mut res = [BitBoard::empty(); 81];
    for col in 0..9 {
        for row in 0..9 {
            let pos = Square::new(col, row);
            for (dc, dr) in shifts.iter() {
                let col = col as isize + dc;
                let row = row as isize + dr;
                if (0..9).contains(&col) && (0..9).contains(&row) {
                    res[pos.index()].set(Square::new(col as usize, row as usize));
                }
            }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::{Color, Kind},
        position::bitboard::testing::bitboard,
        position::bitboard::Square,
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
