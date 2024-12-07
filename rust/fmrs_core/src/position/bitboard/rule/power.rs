use crate::piece::{Color, Kind};

use super::super::{BitBoard, Square};

pub fn power(color: Color, pos: Square, kind: Kind) -> BitBoard {
    POWERS[index(color, kind)][pos.index()]
}

const fn index(color: Color, kind: Kind) -> usize {
    match (kind, color) {
        (Kind::Pawn, Color::Black) => 0,
        (Kind::Pawn, Color::White) => 1,
        (Kind::Lance, Color::Black) => 2,
        (Kind::Lance, Color::White) => 3,
        (Kind::Knight, Color::Black) => 4,
        (Kind::Knight, Color::White) => 5,
        (Kind::Silver, Color::Black) => 6,
        (Kind::Silver, Color::White) => 7,
        (Kind::Bishop, _) => 8,
        (Kind::Rook, _) => 9,
        (Kind::King, _) => 10,
        (Kind::ProBishop, _) => 11,
        (Kind::ProRook, _) => 12,
        (_, Color::Black) => 13, // Gold
        (_, Color::White) => 14,
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
