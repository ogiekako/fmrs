use crate::piece::{Color, Kind};

use super::super::{BitBoard, Square};

pub fn power(color: Color, pos: Square, kind: Kind) -> BitBoard {
    POWERS[kind.index()][color.index()][pos.index()]
}

pub fn pawn_power(color: Color, pos: Square) -> BitBoard {
    PAWN_POWER[color.index()][pos.index()]
}

pub fn knight_power(color: Color, pos: Square) -> BitBoard {
    KNIGHT_POWER[color.index()][pos.index()]
}

pub fn silver_power(color: Color, pos: Square) -> BitBoard {
    SILVER_POWER[color.index()][pos.index()]
}

pub fn gold_power(color: Color, pos: Square) -> BitBoard {
    GOLD_POWER[color.index()][pos.index()]
}

pub fn king_power(pos: Square) -> BitBoard {
    KING_POWER[pos.index()]
}

pub fn lance_power(color: Color, pos: Square) -> BitBoard {
    LANCE_POWER[color.index()][pos.index()]
}

type KindPower = [[BitBoard; 81]; 2];

lazy_static! {
    static ref POWERS: Vec<KindPower> = {
        let mut res = vec![];
        for kind in Kind::iter() {
            res.push(match kind.index() {
                Kind::PAWN_ID => *PAWN_POWER,
                Kind::LANCE_ID => *LANCE_POWER,
                Kind::KNIGHT_ID => *KNIGHT_POWER,
                Kind::SILVER_ID => *SILVER_POWER,
                Kind::GOLD_ID => *GOLD_POWER,
                Kind::BISHOP_ID => *BISHOP_POWER,
                Kind::ROOK_ID => *ROOK_POWER,
                Kind::KING_ID => [*KING_POWER, *KING_POWER],
                Kind::PRO_PAWN_ID => *GOLD_POWER,
                Kind::PRO_LANCE_ID => *GOLD_POWER,
                Kind::PRO_KNIGHT_ID => *GOLD_POWER,
                Kind::PRO_SILVER_ID => *GOLD_POWER,
                Kind::PRO_BISHOP_ID => *PRO_BISHOP_POWER,
                Kind::PRO_ROOK_ID => *PRO_ROOK_POWER,
                _ => unreachable!(),
            });
        }
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::SILVER)
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
            super::power(Color::WHITE, Square::new(0, 0), Kind::PAWN)
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
            super::power(Color::WHITE, Square::new(0, 0), Kind::PRO_SILVER)
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::KING)
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::PRO_ROOK)
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
            super::power(Color::BLACK, Square::new(0, 0), Kind::ROOK)
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
            super::power(Color::WHITE, Square::new(1, 2), Kind::LANCE)
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
            super::power(Color::WHITE, Square::new(1, 3), Kind::BISHOP)
        );
    }
}
