use crate::piece::{Color, Kind, NUM_KIND};

use super::super::{BitBoard, Square};

pub fn power(color: Color, pos: Square, kind: Kind) -> BitBoard {
    const CK: [u8; NUM_KIND * 2] = [
        ColorKind::BlackPawn.index() as u8,
        ColorKind::BlackLance.index() as u8,
        ColorKind::BlackKnight.index() as u8,
        ColorKind::BlackSilver.index() as u8,
        ColorKind::BlackGold.index() as u8,
        ColorKind::Bishop.index() as u8,
        ColorKind::Rook.index() as u8,
        ColorKind::King.index() as u8,
        ColorKind::BlackGold.index() as u8,
        ColorKind::BlackGold.index() as u8,
        ColorKind::BlackGold.index() as u8,
        ColorKind::BlackGold.index() as u8,
        ColorKind::ProBishop.index() as u8,
        ColorKind::ProRook.index() as u8,
        ColorKind::WhitePawn.index() as u8,
        ColorKind::WhiteLance.index() as u8,
        ColorKind::WhiteKnight.index() as u8,
        ColorKind::WhiteSilver.index() as u8,
        ColorKind::WhiteGold.index() as u8,
        ColorKind::Bishop.index() as u8,
        ColorKind::Rook.index() as u8,
        ColorKind::King.index() as u8,
        ColorKind::WhiteGold.index() as u8,
        ColorKind::WhiteGold.index() as u8,
        ColorKind::WhiteGold.index() as u8,
        ColorKind::WhiteGold.index() as u8,
        ColorKind::ProBishop.index() as u8,
        ColorKind::ProRook.index() as u8,
    ];

    POWERS[pos.index()][CK[kind.index() + color.index() * NUM_KIND] as usize]
}

pub fn pawn_power(color: Color, pos: Square) -> BitBoard {
    const CK: [usize; 2] = [ColorKind::BlackPawn.index(), ColorKind::WhitePawn.index()];
    POWERS[pos.index()][CK[color.index()]]
}

pub fn knight_power(color: Color, pos: Square) -> BitBoard {
    const CK: [usize; 2] = [
        ColorKind::BlackKnight.index(),
        ColorKind::WhiteKnight.index(),
    ];
    POWERS[pos.index()][CK[color.index()]]
}

pub fn silver_power(color: Color, pos: Square) -> BitBoard {
    const CK: [usize; 2] = [
        ColorKind::BlackSilver.index(),
        ColorKind::WhiteSilver.index(),
    ];
    POWERS[pos.index()][CK[color.index()]]
}

pub fn gold_power(color: Color, pos: Square) -> BitBoard {
    const CK: [usize; 2] = [ColorKind::BlackGold.index(), ColorKind::WhiteGold.index()];
    POWERS[pos.index()][CK[color.index()]]
}

pub fn king_power(pos: Square) -> BitBoard {
    POWERS[pos.index()][ColorKind::King.index()]
}

pub fn lion_king_power(pos: Square) -> BitBoard {
    LION_KING_POWER[pos.index()]
}

pub fn king_then_king_or_night_power(color: Color, pos: Square) -> BitBoard {
    KING_THEN_KING_OR_NIGHT_POWER[color.index()][pos.index()]
}

pub fn lance_power(color: Color, pos: Square) -> BitBoard {
    const CK: [usize; 2] = [ColorKind::BlackLance.index(), ColorKind::WhiteLance.index()];
    POWERS[pos.index()][CK[color.index()]]
}

pub fn bishop_power(pos: Square) -> BitBoard {
    POWERS[pos.index()][ColorKind::Bishop.index()]
}

pub fn rook_power(pos: Square) -> BitBoard {
    POWERS[pos.index()][ColorKind::Rook.index()]
}

pub fn pro_bishop_power(pos: Square) -> BitBoard {
    POWERS[pos.index()][ColorKind::ProBishop.index()]
}

pub fn pro_rook_power(pos: Square) -> BitBoard {
    POWERS[pos.index()][ColorKind::ProRook.index()]
}

pub fn queen_power(pos: Square) -> BitBoard {
    QUEEN_POWER[pos.index()]
}

pub fn king_and_any_power(color: Color, pos: Square) -> BitBoard {
    KING_AND_ANY_POWER[color.index()][pos.index()]
}

type KindPower = [[BitBoard; 81]; 2];

lazy_static! {
    static ref LION_KING_POWER: [BitBoard; 81] = powers_sub(
        [
            (-2, -2),
            (-2, -1),
            (-2, 0),
            (-2, 1),
            (-2, 2),
            (-1, -2),
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (-1, 2),
            (0, -2),
            (0, -1),
            (0, 1),
            (0, 2),
            (1, -2),
            (1, -1),
            (1, 0),
            (1, 1),
            (1, 2),
            (2, -2),
            (2, -1),
            (2, 0),
            (2, 1),
            (2, 2)
        ]
        .into_iter()
    );
    static ref QUEEN_POWER: [BitBoard; 81] = {
        let mut res = [BitBoard::default(); 81];
        for pos in Square::iter() {
            let mut p = BitBoard::default();
            for k in [Kind::ProBishop, Kind::ProRook] {
                p |= power(Color::BLACK, pos, k);
            }
            res[pos.index()] = p;
        }
        res
    };
    static ref KING_AND_ANY_POWER: KindPower = {
        let mut res = [[BitBoard::default(); 81]; 2];
        for color in Color::iter() {
            for pos in Square::iter() {
                let mut p = BitBoard::default();
                for pos2 in power(color, pos, Kind::King) {
                    for k in [Kind::Knight, Kind::ProBishop, Kind::ProRook] {
                        p |= power(color, pos2, k);
                    }
                }
                res[color.index()][pos.index()] = p;
            }
        }
        res
    };
    static ref KING_THEN_KING_OR_NIGHT_POWER: KindPower = {
        let mut res = [[BitBoard::default(); 81]; 2];
        for color in Color::iter() {
            for pos in Square::iter() {
                let mut p = BitBoard::default();
                for pos2 in king_power(pos) {
                    p |= king_power(pos2);
                    p |= knight_power(color, pos2);
                }
                res[color.index()][pos.index()] = p;
            }
        }
        res
    };
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorKind {
    BlackPawn,
    BlackLance,
    BlackKnight,
    BlackSilver,
    BlackGold,
    WhitePawn,
    WhiteLance,
    WhiteKnight,
    WhiteSilver,
    WhiteGold,
    Bishop,
    Rook,
    King,
    ProBishop,
    ProRook,
    //
    RuLd,
    RdLu,
    RL,
    UD,
}

const COLOR_KIND_NUM: usize = 19;

const COLOR_KINDS: [ColorKind; COLOR_KIND_NUM] = [
    ColorKind::BlackPawn,
    ColorKind::BlackLance,
    ColorKind::BlackKnight,
    ColorKind::BlackSilver,
    ColorKind::BlackGold,
    ColorKind::WhitePawn,
    ColorKind::WhiteLance,
    ColorKind::WhiteKnight,
    ColorKind::WhiteSilver,
    ColorKind::WhiteGold,
    ColorKind::Bishop,
    ColorKind::Rook,
    ColorKind::King,
    ColorKind::ProBishop,
    ColorKind::ProRook,
    //
    ColorKind::RuLd,
    ColorKind::RdLu,
    ColorKind::RL,
    ColorKind::UD,
];

impl ColorKind {
    const fn slides(&self) -> [Option<(i8, i8)>; 4] {
        match self {
            ColorKind::BlackLance => [Some((0, -1)), None, None, None],
            ColorKind::WhiteLance => [Some((0, 1)), None, None, None],
            ColorKind::Bishop | ColorKind::ProBishop => {
                [Some((-1, -1)), Some((-1, 1)), Some((1, -1)), Some((1, 1))]
            }
            ColorKind::Rook | ColorKind::ProRook => {
                [Some((-1, 0)), Some((0, -1)), Some((0, 1)), Some((1, 0))]
            }
            ColorKind::RuLd => [Some((-1, -1)), Some((1, 1)), None, None],
            ColorKind::RdLu => [Some((-1, 1)), Some((1, -1)), None, None],
            ColorKind::RL => [Some((-1, 0)), Some((1, 0)), None, None],
            ColorKind::UD => [Some((0, -1)), Some((0, 1)), None, None],
            _ => [None; 4],
        }
    }
    const fn steps(&self) -> [Option<(i8, i8)>; 8] {
        match self {
            ColorKind::BlackPawn => [Some((0, -1)), None, None, None, None, None, None, None],
            ColorKind::BlackKnight => [
                Some((-1, -2)),
                Some((1, -2)),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            ColorKind::BlackSilver => [
                Some((-1, -1)),
                Some((-1, 1)),
                Some((0, -1)),
                Some((1, -1)),
                Some((1, 1)),
                None,
                None,
                None,
            ],
            ColorKind::BlackGold => [
                Some((-1, -1)),
                Some((-1, 0)),
                Some((0, -1)),
                Some((0, 1)),
                Some((1, -1)),
                Some((1, 0)),
                None,
                None,
            ],
            ColorKind::WhitePawn => [Some((0, 1)), None, None, None, None, None, None, None],
            ColorKind::WhiteKnight => [
                Some((-1, 2)),
                Some((1, 2)),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            ColorKind::WhiteSilver => [
                Some((-1, -1)),
                Some((-1, 1)),
                Some((0, 1)),
                Some((1, -1)),
                Some((1, 1)),
                None,
                None,
                None,
            ],
            ColorKind::WhiteGold => [
                Some((-1, 1)),
                Some((-1, 0)),
                Some((0, 1)),
                Some((0, -1)),
                Some((1, 1)),
                Some((1, 0)),
                None,
                None,
            ],
            ColorKind::King => [
                Some((-1, -1)),
                Some((-1, 0)),
                Some((-1, 1)),
                Some((0, -1)),
                Some((0, 1)),
                Some((1, -1)),
                Some((1, 0)),
                Some((1, 1)),
            ],
            ColorKind::ProBishop => [
                Some((-1, 0)),
                Some((0, -1)),
                Some((0, 1)),
                Some((1, 0)),
                None,
                None,
                None,
                None,
            ],
            ColorKind::ProRook => [
                Some((-1, -1)),
                Some((-1, 1)),
                Some((1, -1)),
                Some((1, 1)),
                None,
                None,
                None,
                None,
            ],
            _ => [None; 8],
        }
    }

    const fn power_naive(&self, pos: Square) -> BitBoard {
        let slides = self.slides();
        let steps = self.steps();

        let mut res = BitBoard::EMPTY;

        let mut i = 0;
        while i < 4 {
            let Some((dx, dy)) = slides[i] else { break };

            let (mut x, mut y) = (pos.col() as i8, pos.row() as i8);
            let mut j = 0;
            while j < 8 {
                x += dx;
                y += dy;
                if x < 0 || x >= 9 || y < 0 || y >= 9 {
                    break;
                }
                res.set(Square::new(x as usize, y as usize));
                j += 1;
            }

            i += 1;
        }
        i = 0;
        while i < 8 {
            let Some((dx, dy)) = steps[i] else {
                break;
            };

            let x = pos.col() as i8 + dx;
            let y = pos.row() as i8 + dy;
            if x >= 0 && x < 9 && y >= 0 && y < 9 {
                res.set(Square::new(x as usize, y as usize));
            }

            i += 1;
        }
        res
    }

    const fn index(&self) -> usize {
        *self as usize
    }

    pub const fn power(&self, pos: Square) -> BitBoard {
        POWERS[pos.index()][self.index()]
    }

    pub const fn reachable_naive(&self, occupied: BitBoard, pos: Square) -> BitBoard {
        let slides = self.slides();
        let steps = self.steps();

        let mut res = BitBoard::EMPTY;
        let mut i = 0;
        while i < steps.len() {
            let Some((dx, dy)) = steps[i] else {
                break;
            };
            let (x, y) = (pos.col() as i8 + dx, pos.row() as i8 + dy);
            if x >= 0 && x < 9 && y >= 0 && y < 9 {
                res.set(Square::new(x as usize, y as usize));
            }
            i += 1;
        }
        let mut i = 0;
        while i < slides.len() {
            let Some((dx, dy)) = slides[i] else {
                break;
            };
            let (mut x, mut y) = (pos.col() as i8, pos.row() as i8);
            let mut j = 0;
            while j < 8 {
                x += dx;
                y += dy;
                if x < 0 || x >= 9 || y < 0 || y >= 9 {
                    break;
                }
                let p = Square::new(x as usize, y as usize);
                res.set(p);
                if occupied.contains(p) {
                    break;
                }
                j += 1;
            }
            i += 1;
        }

        res
    }
}

const POWERS: [[BitBoard; COLOR_KIND_NUM]; 81] = construct_powers();

const fn construct_pos_powers(pos: Square) -> [BitBoard; COLOR_KIND_NUM] {
    let mut res = [BitBoard::const_default(); COLOR_KIND_NUM];

    let mut i = 0;
    while i < COLOR_KIND_NUM {
        res[i] = COLOR_KINDS[i].power_naive(pos);
        i += 1;
    }
    res
}

const fn construct_powers() -> [[BitBoard; COLOR_KIND_NUM]; 81] {
    let mut res = [[BitBoard::const_default(); COLOR_KIND_NUM]; 81];

    let mut i = 0;
    while i < 81 {
        let pos = Square::from_index(i);
        res[i] = construct_pos_powers(pos);
        i += 1;
    }
    res
}

#[cfg(test)]
mod tests {
    use crate::{
        bitboard,
        piece::{Color, Kind},
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::Silver)
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
            super::power(Color::WHITE, Square::new(0, 0), Kind::Pawn)
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
            super::power(Color::WHITE, Square::new(0, 0), Kind::ProSilver)
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::King)
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
            super::power(Color::BLACK, Square::new(1, 2), Kind::ProRook)
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
            super::power(Color::BLACK, Square::new(0, 0), Kind::Rook)
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
            super::power(Color::WHITE, Square::new(1, 2), Kind::Lance)
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
            super::power(Color::WHITE, Square::new(1, 3), Kind::Bishop)
        );
    }
}
