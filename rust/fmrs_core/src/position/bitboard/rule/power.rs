use crate::piece::{Color, EssentialKind};

use super::super::{BitBoard, Square};

pub fn essential_power(color: Color, pos: Square, ek: EssentialKind) -> &'static BitBoard {
    &POWERS[essential_index(color, ek)][pos.index()]
}

const fn essential_index(color: Color, ek: EssentialKind) -> usize {
    match (ek, color) {
        (EssentialKind::Pawn, Color::Black) => 0,
        (EssentialKind::Pawn, Color::White) => 1,
        (EssentialKind::Lance, Color::Black) => 2,
        (EssentialKind::Lance, Color::White) => 3,
        (EssentialKind::Knight, Color::Black) => 4,
        (EssentialKind::Knight, Color::White) => 5,
        (EssentialKind::Silver, Color::Black) => 6,
        (EssentialKind::Silver, Color::White) => 7,
        (EssentialKind::Bishop, _) => 8,
        (EssentialKind::Rook, _) => 9,
        (EssentialKind::King, _) => 10,
        (EssentialKind::ProBishop, _) => 11,
        (EssentialKind::ProRook, _) => 12,
        (EssentialKind::Gold, Color::Black) => 13,
        (EssentialKind::Gold, Color::White) => 14,
    }
}

type KindPower = [[BitBoard; 81]; 2];

lazy_static! {
    static ref POWERS: Vec<[BitBoard; 81]> = {
        let res = vec![
            PAWN_POWER[0],
            PAWN_POWER[1],
            LANCE_POWER[0],
            LANCE_POWER[1],
            KNIGHT_POWER[0],
            KNIGHT_POWER[1],
            SILVER_POWER[0],
            SILVER_POWER[1],
            BISHOP_POWER[0],
            ROOK_POWER[0],
            KING_POWER.clone(),
            PRO_BISHOP_POWER[0],
            PRO_ROOK_POWER[0],
            GOLD_POWER[0],
            GOLD_POWER[1],
        ];
        res
    };
    static ref PAWN_POWER: KindPower = powers([(0, -1)].into_iter());
    static ref KNIGHT_POWER: KindPower = powers([(-1, -2), (1, -2)].into_iter());
    static ref SILVER_POWER: KindPower =
        powers([(-1, -1), (-1, 1), (0, -1), (1, -1), (1, 1)].into_iter());
    static ref GOLD_POWER: KindPower =
        powers([(-1, -1), (-1, 0), (0, -1), (0, 1), (1, -1), (1, 0)].into_iter());
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
    static ref LANCE_POWER: KindPower = powers(run((0, -1)));
    static ref BISHOP_POWER: KindPower = powers(
        run((-1, -1))
            .chain(run((-1, 1)))
            .chain(run((1, -1)))
            .chain(run((1, 1)))
    );
    static ref ROOK_POWER: KindPower = powers(
        run((-1, 0))
            .chain(run((0, -1)))
            .chain(run((0, 1)))
            .chain(run((1, 0)))
    );
    static ref PRO_BISHOP_POWER: KindPower = powers(
        run((-1, -1))
            .chain(run((-1, 1)))
            .chain(run((1, -1)))
            .chain(run((1, 1)).chain([(-1, 0), (0, -1), (0, 1), (1, 0)].into_iter()))
    );
    static ref PRO_ROOK_POWER: KindPower = powers(
        run((-1, 0))
            .chain(run((0, -1)))
            .chain(run((0, 1)))
            .chain(run((1, 0)).chain([(-1, -1), (-1, 1), (1, -1), (1, 1)].into_iter()))
    );
}

fn run(dir: (isize, isize)) -> impl Iterator<Item = (isize, isize)> {
    (1..9).into_iter().map(move |i| (dir.0 * i, dir.1 * i))
}

fn powers(black_shifts: impl Iterator<Item = (isize, isize)>) -> KindPower {
    let black_shifts = black_shifts.collect::<Vec<_>>();
    let black = powers_sub(black_shifts.iter().map(|(col, row)| (*col, *row)));
    let white = powers_sub(black_shifts.iter().map(|(col, row)| (*col, -row)));
    [black, white]
}

fn powers_sub(shifts: impl Iterator<Item = (isize, isize)>) -> [BitBoard; 81] {
    let shifts = shifts.collect::<Vec<_>>();
    let mut res = [BitBoard::default(); 81];
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
        piece::{Color, EssentialKind},
        position::bitboard::{testing::bitboard, Square},
    };

    #[test]
    fn essential_power() {
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
            *super::essential_power(Color::Black, Square::new(1, 2), EssentialKind::Silver)
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
            *super::essential_power(Color::White, Square::new(0, 0), EssentialKind::Pawn)
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
            *super::essential_power(Color::White, Square::new(0, 0), EssentialKind::Gold)
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
            *super::essential_power(Color::Black, Square::new(1, 2), EssentialKind::King)
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
            *super::essential_power(Color::Black, Square::new(1, 2), EssentialKind::ProRook)
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
            *super::essential_power(Color::Black, Square::new(0, 0), EssentialKind::Rook)
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
            *super::essential_power(Color::White, Square::new(1, 2), EssentialKind::Lance)
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
            *super::essential_power(Color::White, Square::new(1, 3), EssentialKind::Bishop)
        );
    }
}
