use serde::{Deserialize, Serialize};

/*
9 8 7 6 5 4 3 2 1
              9 0 一
                1 二
    ...       ...
                7 八
80              8 九
*/
#[derive(Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
pub struct BitBoard {
    x: u128,
}

#[cfg(test)]
macro_rules! bitboard {
    ($($x:expr,)*) => {
        {
            let v = vec![$($x),*];
            if v.len() != 9 {
                panic!("Exactly 9 elements should be given.");
            }
            let mut res = crate::position::bitboard::BitBoard::new();
            for i in 0..9 {
                if v[i].len() != 9 {
                    panic!("v[{}] = {:?} should contain exactly 9 characters.", i, v[i]);
                }
                for (j, c) in v[i].chars().rev().enumerate() {
                    if c == '*' {
                        res.set(Square::new(j, i));
                    }
                }
            }
            res
        }
    }
}

macro_rules! def_op {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            type Output = Self;

            fn $op(self, rhs: Self) -> Self {
                BitBoard {
                    x: self.x.$op(rhs.x),
                }
            }
        }
    };
}

def_op!(Mul, mul);
def_op!(Shr, shr);
def_op!(BitAnd, bitand);
def_op!(BitOr, bitor);

macro_rules! def_op_assign {
    ($ty: ident, $op: ident) => {
        impl std::ops::$ty for BitBoard {
            fn $op(&mut self, rhs: Self) {
                self.x.$op(rhs.x);
            }
        }
    };
}

def_op_assign!(MulAssign, mul_assign);
def_op_assign!(ShrAssign, shr_assign);
def_op_assign!(BitAndAssign, bitand_assign);
def_op_assign!(BitOrAssign, bitor_assign);

impl std::ops::Not for BitBoard {
    type Output = Self;

    fn not(self) -> BitBoard {
        BitBoard { x: self.x.not() }
    }
}

impl Iterator for BitBoard {
    type Item = Square;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        Some(self.pop())
    }
}

#[test]
fn test_bitboard_next() {
    let x = Square::new(1, 2);
    let mut tmpl = BitBoard::new();
    tmpl.set(x);
    let tmpl = tmpl;

    let mut b = tmpl;
    assert_eq!(Some(x), b.next());
    assert_eq!(None, b.next());
}

use std::fmt;
impl fmt::Display for BitBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..9 {
            for col in (0..9).rev() {
                write!(
                    f,
                    "{}",
                    if self.get(Square::new(col, row)) {
                        "*"
                    } else {
                        "."
                    }
                )?
            }
            writeln!(f)?
        }
        Ok(())
    }
}

impl fmt::Debug for BitBoard {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "\n{}", self)
    }
}

impl BitBoard {
    // Assumes self is not empty.

    fn pop(&mut self) -> Square {
        let res = Square::from_index(self.x.trailing_zeros() as usize);
        self.x = self.x & (self.x - 1);
        res
    }

    pub fn new() -> BitBoard {
        BitBoard { x: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.x == 0
    }
    pub fn set(&mut self, pos: Square) {
        self.x |= 1 << pos.index();
    }
    pub fn unset(&mut self, pos: Square) {
        self.x &= !(1 << pos.index());
    }
    pub fn get(&self, pos: Square) -> bool {
        (self.x >> pos.index() & 1) == 1
    }
    pub fn subsets(&self) -> impl Iterator<Item = BitBoard> {
        let orig = self.x;
        let mut x = self.x;
        (0..(1u128 << self.x.count_ones())).map(move |_| {
            x = orig & (x.wrapping_sub(1));
            BitBoard { x }
        })
    }
}
#[test]
fn test_bitboard_subsets() {
    assert_eq!(
        BitBoard { x: 5 }.subsets().collect::<Vec<BitBoard>>(),
        vec![
            BitBoard { x: 4 },
            BitBoard { x: 1 },
            BitBoard { x: 0 },
            BitBoard { x: 5 },
        ]
    );
}

use crate::piece::*;

// Movable positions assuming occupied are opponent's pieces.
pub fn movable_positions(occupied: BitBoard, pos: Square, c: Color, k: Kind) -> BitBoard {
    match k {
        Lance => lance_movable_positions(occupied, pos, c),
        Bishop => bishop_movable_positions(occupied, pos),
        Rook => rook_movable_positions(occupied, pos),
        ProBishop | ProRook => {
            attacks_from(pos, c, King) | movable_positions(occupied, pos, c, k.unpromote().unwrap())
        }
        _ => attacks_from(pos, c, k),
    }
}

fn bishop_movable_positions(occupied: BitBoard, pos: Square) -> BitBoard {
    BISHOP_MAGIC[pos.index()].compute(occupied)
}

fn rook_movable_positions(occupied: BitBoard, pos: Square) -> BitBoard {
    ROOK_MAGIC[pos.index()].compute(occupied)
}

#[derive(Clone, Deserialize, Serialize)]
struct Magic {
    mask: BitBoard,
    magic: u128,
    shift: usize,
    table: Vec<BitBoard>,
}

impl Magic {
    fn zero() -> Magic {
        Magic {
            mask: BitBoard::new(),
            magic: 0,
            shift: 0,
            table: vec![],
        }
    }
    fn compute(&self, occupied: BitBoard) -> BitBoard {
        self.table[((self.mask & occupied).x.wrapping_mul(self.magic) >> self.shift) as usize]
    }
}
use arr_macro::arr;

lazy_static! {
    static ref BISHOP_MAGIC: [Magic; 81] = bishop_magics().expect("Failed to compute bishop magic");
    static ref ROOK_MAGIC: [Magic; 81] = rook_magics().expect("Failed to compute rook magic");
}

fn deserialize_magic(contents: &[u8]) -> Result<[Magic; 81], String> {
    let v: Vec<Magic> = bincode::deserialize(&contents).map_err(|err| err.to_string())?;
    let mut res = arr![Magic::zero(); 81];
    if v.len() != res.len() {
        return Err(format!(
            "Unexpected vector length {}, want {}",
            v.len(),
            res.len()
        ));
    }
    for i in 0..81 {
        res[i] = v[i].clone();
    }
    Ok(res)
}

fn bishop_magics() -> Result<[Magic; 81], String> {
    match deserialize_magic(include_bytes!("data/bishop_magic.bin")) {
        Ok(x) => return Ok(x),
        Err(x) => {
            eprintln!("Generating bishop magic: failed to load magic file: {}", x);
        }
    }
    let mut res = arr![Magic::zero(); 81];
    for pos in Square::iter() {
        res[pos.index()] = bishop_magic(pos);
    }
    Ok(res)
}

fn rook_magics() -> Result<[Magic; 81], String> {
    match deserialize_magic(include_bytes!("data/rook_magic.bin")) {
        Ok(x) => return Ok(x),
        Err(x) => {
            eprintln!("Generating rook magic: failed to load magic file: {}", x);
        }
    }
    let mut res = arr![Magic::zero(); 81];
    for pos in Square::iter() {
        res[pos.index()] = rook_magic(pos);
    }
    Ok(res)
}

use crate::rand::{Rng, SeedableRng};

use super::Square;
fn magic(pos: Square, mask: BitBoard, dirs: Vec<(isize, isize)>) -> Magic {
    let n = mask.x.count_ones() as usize;
    let nn = 1 << n;

    let subs: Vec<BitBoard> = mask.subsets().collect();
    let expected: Vec<BitBoard> = subs
        .iter()
        .map(|sub| {
            let mut movable = BitBoard::new();
            // Naive computation of bishop's movable positions.
            for (dc, dr) in &dirs {
                for i in 1..9 {
                    match pos.add(dc * i, dr * i) {
                        Some(p) => {
                            movable.set(p);
                            if sub.get(p) {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
            movable
        })
        .collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut table = vec![None; nn];

    let mut iter = 0;
    let mut magic;
    loop {
        iter += 1;
        magic = 0u128;
        for i in 0..128 {
            // Set 1 with 15% of probability.
            if rng.gen_range(0..100) < 15 {
                magic |= 1 << i;
            }
        }
        let mut ok = true;
        for i in 0..nn {
            let id = (subs[i].x.wrapping_mul(magic) >> (128 - n)) as usize;
            if table[id].is_some() {
                ok = false;
                break;
            }
            table[id] = Some(expected[i]);
        }
        if ok {
            break;
        }
        for i in 0..nn {
            table[i] = None;
        }
    }
    eprintln!("Magic has found with {} iterations.", iter);
    Magic {
        mask,
        magic,
        shift: 128 - n,
        table: table.iter().map(|x| x.unwrap()).collect(),
    }
}

fn bishop_magic(pos: Square) -> Magic {
    let dirs = vec![(-1, -1), (-1, 1), (1, -1), (1, 1)];
    let mask = BISHOP_MASKS[pos.index()];
    magic(pos, mask, dirs)
}

fn rook_magic(pos: Square) -> Magic {
    let dirs = vec![(-1, 0), (0, -1), (0, 1), (1, 0)];
    let mask = ROOK_MASKS[pos.index()];
    eprintln!("INFO: Computig rook magic for {:?}", pos);
    magic(pos, mask, dirs)
}

fn lance_movable_positions(occupied: BitBoard, pos: Square, c: Color) -> BitBoard {
    let attacks = attacks_from(pos, c, Lance);
    let occu = LANCE_MASKS[pos.index()][c.index()] & occupied;
    if occu.x == 0 {
        return attacks;
    }
    match c {
        Black => BitBoard {
            // highest bit .. pos
            x: (1 << pos.index()) - ((occu.x + 1).next_power_of_two() >> 1),
        },
        White => BitBoard {
            // lowest bit .. pos
            x: ((occu.x - 1) ^ occu.x) & attacks.x,
        },
    }
}

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
        movable_positions(occupied, Square::new(2, 3), Black, Lance)
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
        movable_positions(occupied, Square::new(2, 1), Black, Lance)
    );
    assert_eq!(
        BitBoard::new(),
        movable_positions(occupied, Square::new(2, 0), Black, Lance)
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
        movable_positions(occupied, Square::new(2, 0), White, Lance)
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
        movable_positions(occupied, Square::new(2, 1), White, Lance)
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
        movable_positions(occupied, Square::new(2, 4), White, Lance)
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
        movable_positions(occupied, Square::new(0, 0), White, Lance)
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
        movable_positions(occupied, Square::new(0, 8), Black, Lance)
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
        movable_positions(occupied, Square::new(0, 0), Black, Bishop)
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
        movable_positions(occupied, Square::new(1, 1), Black, Bishop)
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
        movable_positions(occupied, Square::new(1, 2), Black, Bishop)
    );
}

#[test]
fn test_rook_movable_positions() {
    assert_eq!(
        bitboard!(
            ".*******.",
            "........*",
            "........*",
            "........*",
            "........*",
            "........*",
            "........*",
            "........*",
            ".........",
        ),
        ROOK_MASKS[Square::new(0, 0).index()]
    );
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
        movable_positions(occupied, Square::new(5, 1), Black, Rook)
    );
}

// Attackes on the empty board.
pub fn attacks_from(pos: Square, c: Color, k: Kind) -> BitBoard {
    ATTACKS[pos.index()][c.index()][k.index()]
}

#[test]
fn test_attacks_from() {
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
        attacks_from(Square::new(1, 2), Black, Silver)
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
        attacks_from(Square::new(1, 2), Black, King)
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
        attacks_from(Square::new(1, 2), Black, ProRook)
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
        attacks_from(Square::new(0, 0), Black, Rook)
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
        attacks_from(Square::new(1, 2), White, Lance)
    );
}

lazy_static! {
    static ref LANCE_MASKS: [[BitBoard; 2]; 81] = {
        let mut res = [[BitBoard::new(); 2]; 81];
        for pos in Square::iter() {
            for c in Color::iter() {
                let mut mask = attacks_from(pos, c, Lance);
                for p in vec![Square::new(0, pos.row()), Square::new(8, pos.row())] {
                    mask.unset(p);
                }
                res[pos.index()][c.index()] = mask;
            }
        }
        res
    };
    static ref BISHOP_MASKS: [BitBoard; 81] = {
        let mut mask = BitBoard::new();
        for c in 1..8 {
            for r in 1..8 {
                mask.set(Square::new(c, r));
            }
        }
        let mut res = [BitBoard::new(); 81];
        for pos in Square::iter() {
            res[pos.index()] = attacks_from(pos, Black, Bishop) & mask;
        }
        res
    };
    static ref ROOK_MASKS: [BitBoard; 81] = {
        let mut res = [BitBoard::new(); 81];
        for pos in Square::iter() {
            let mut mask = attacks_from(pos, Black, Rook);
            for p in vec![Square::new(0, pos.row()), Square::new(8, pos.row()),
                          Square::new(pos.col(), 0), Square::new(pos.col(), 8)] {
                mask.unset(p);
            }
            res[pos.index()] = mask;
        }
        res
    };

    // pos, color, kind
    static ref ATTACKS: [[[BitBoard; NUM_KIND]; 2]; 81] = {
        let mut res = [[[BitBoard::new(); NUM_KIND]; 2]; 81];

        type Control = [&'static str; 3];
        const CONTROL: [Control; NUM_HAND_KIND] = [[
            ".*.",
            ".x.",
            "...",
        ], [
            ".+.",
            ".x.",
            "...",
        ],[
            "*.*",
            "...",
            ".x.",
        ], [
            "***",
            ".x.",
            "*.*",
        ],[
            "***",
            "*x*",
            ".*.",
        ], [
            "+.+",
            ".x.",
            "+.+",
        ], [
            ".+.",
            "+x+",
            ".+.",
        ]];

        fn fill(ats: &mut[BitBoard; NUM_KIND], pos: Square, color: Color, k: Kind) {
            ats[k.index()] = match k {
                Pawn | Lance | Knight | Silver | Gold | Bishop | Rook => {
                    let (oi, oj) = (0..3).find_map(|i|
                        CONTROL[k.index()][i].chars().enumerate().find_map(|(j,x)|
                            if x=='x' {Some((i,j))} else {None})).expect("x not found");

                    let mut board = BitBoard::new();
                    for i in 0..3 {
                        for (j, x) in CONTROL[k.index()][i].chars().enumerate() {
                            let (dc, mut dr) = (j.wrapping_sub(oj) as isize, i.wrapping_sub(oi) as isize);
                            if color == White {
                                dr *= -1
                            }
                            let n = match x {
                                '*' => 2,
                                '+' => 9,
                                _ => continue,
                            };
                            for l in 1..n {
                                if let Some(p) = pos.add(dc * l, dr * l) {
                                    board.set(p);
                                } else {
                                    break
                                }
                            }
                        }
                    }
                    board
                },
                King => ats[Gold.index()] | ats[Silver.index()],
                ProPawn | ProLance | ProKnight | ProSilver => ats[Gold.index()],
                ProBishop | ProRook => ats[King.index()] | ats[k.unpromote().unwrap().index()],
            }
        }

        for pos in Square::iter() {
            for c in Color::iter() {
                let ats = &mut res[pos.index()][c.index()];
                for k in Kind::iter() {
                    fill(ats, pos, c, k);
                }
            }
        }
        res
    };
}
