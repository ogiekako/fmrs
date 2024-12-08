use crate::piece::{Color, EssentialKind};

use super::super::{BitBoard, Square};

pub fn power(color: Color, pos: Square, ek: EssentialKind) -> BitBoard {
    let i = if ek.index() < EssentialKind::Bishop.index() {
        ek.index() << 1 | color.index()
    } else {
        ek.index() + EssentialKind::Bishop.index()
    } | pos.index() * 16;
    debug_assert!(i < POWERS2.len());
    unsafe { *POWERS2.get_unchecked(i) }
}

pub fn king_power(pos: Square) -> BitBoard {
    debug_assert!(pos.index() < KING_POWER.len());
    unsafe { *KING_POWER.get_unchecked(pos.index()) }
}

pub fn lion_king_power(pos: Square) -> BitBoard {
    debug_assert!(pos.index() < LION_KING_POWER.len());
    unsafe { *LION_KING_POWER.get_unchecked(pos.index()) }
}

type KindPower = [BitBoard; 128];
type KindPowerPair = [KindPower; 2];

lazy_static! {
    static ref POWERS: Vec<BitBoard> = [
        PAWN_POWER[0], // 0
        PAWN_POWER[1],
        LANCE_POWER[0], // 2
        LANCE_POWER[1],
        KNIGHT_POWER[0], // 4
        KNIGHT_POWER[1],
        SILVER_POWER[0], // 6
        SILVER_POWER[1],
        GOLD_POWER[0], // 8
        GOLD_POWER[1],
        BISHOP_POWER.clone(), // 10
        ROOK_POWER.clone(), // 11
        KING_POWER.clone(), // 12
        PRO_BISHOP_POWER.clone(), // 13
        PRO_ROOK_POWER.clone(), // 14
        // 15
    ]
    .concat();
    static ref POWERS2: Vec<BitBoard> = {
        let mut res = vec![BitBoard::default(); 128 * 16]; // pos * 16 + kind
        for kind in 0..15 {
            for pos in 0..81 {
                res[pos * 16 + kind] = POWERS[kind * 128 + pos];
            }
        }
        res
    };
    static ref PAWN_POWER: KindPowerPair = powers([(0, -1)].into_iter());
    static ref KNIGHT_POWER: KindPowerPair = powers([(-1, -2), (1, -2)].into_iter());
    static ref SILVER_POWER: KindPowerPair =
        powers([(-1, -1), (-1, 1), (0, -1), (1, -1), (1, 1)].into_iter());
    static ref GOLD_POWER: KindPowerPair =
        powers([(-1, -1), (-1, 0), (0, -1), (0, 1), (1, -1), (1, 0)].into_iter());
    static ref KING_POWER: KindPower = powers_sub(
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
    static ref LANCE_POWER: KindPowerPair = powers(run((0, -1)));
    static ref BISHOP_POWER: KindPower = powers_sub(
        run((-1, -1))
            .chain(run((-1, 1)))
            .chain(run((1, -1)))
            .chain(run((1, 1)))
    );
    static ref ROOK_POWER: KindPower = powers_sub(
        run((-1, 0))
            .chain(run((0, -1)))
            .chain(run((0, 1)))
            .chain(run((1, 0)))
    );
    static ref PRO_BISHOP_POWER: KindPower = powers_sub(
        run((-1, -1))
            .chain(run((-1, 1)))
            .chain(run((1, -1)))
            .chain(run((1, 1)).chain([(-1, 0), (0, -1), (0, 1), (1, 0)].into_iter()))
    );
    static ref PRO_ROOK_POWER: KindPower = powers_sub(
        run((-1, 0))
            .chain(run((0, -1)))
            .chain(run((0, 1)))
            .chain(run((1, 0)).chain([(-1, -1), (-1, 1), (1, -1), (1, 1)].into_iter()))
    );
    static ref LION_KING_POWER: KindPower = powers_sub((-2..=2).into_iter().flat_map(|dc| {
        (-2..=2)
            .into_iter()
            .filter_map(move |dr| (dc != 0 || dr != 0).then(|| (dc, dr)))
            .into_iter()
    }));
}

fn run(dir: (isize, isize)) -> impl Iterator<Item = (isize, isize)> {
    (1..9).into_iter().map(move |i| (dir.0 * i, dir.1 * i))
}

fn powers(black_shifts: impl Iterator<Item = (isize, isize)>) -> KindPowerPair {
    let black_shifts = black_shifts.collect::<Vec<_>>();
    let black = powers_sub(black_shifts.iter().map(|(col, row)| (*col, *row)));
    let white = powers_sub(black_shifts.iter().map(|(col, row)| (*col, -row)));
    [black, white]
}

fn powers_sub(shifts: impl Iterator<Item = (isize, isize)>) -> KindPower {
    let shifts = shifts.collect::<Vec<_>>();
    let mut res = [BitBoard::default(); 128];
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
            super::power(Color::Black, Square::new(1, 2), EssentialKind::Silver)
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
            super::power(Color::White, Square::new(0, 0), EssentialKind::Pawn)
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
            super::power(Color::White, Square::new(0, 0), EssentialKind::Gold)
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
            super::power(Color::Black, Square::new(1, 2), EssentialKind::King)
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
            super::power(Color::Black, Square::new(1, 2), EssentialKind::ProRook)
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
            super::power(Color::Black, Square::new(0, 0), EssentialKind::Rook)
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
            super::power(Color::White, Square::new(1, 2), EssentialKind::Lance)
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
            super::power(Color::White, Square::new(1, 3), EssentialKind::Bishop)
        );
    }
}
