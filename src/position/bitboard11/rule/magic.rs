use std::collections::{HashMap};




use crate::position::bitboard11::{BitBoard, Square};

use super::magic_core::MagicCore;

#[derive(Clone)]
pub(super) struct Magic {
    block_mask: BitBoard,
    magic: MagicCore,
    table: Vec<BitBoard>,
}

impl Magic {
    fn reachable(&self, occupied: BitBoard) -> BitBoard {
        let block = self.block_mask & occupied;
        self.table[self.magic.index(digest(block))]
    }
    fn zero() -> Self {
        Magic {
            block_mask: BitBoard::new(),
            magic: MagicCore::zero(),
            table: vec![],
        }
    }
}

pub(super) fn bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    let (m1, m2) = &BISHOP_MAGIC[pos.index81()];
    m1.reachable(occupied) | m2.reachable(occupied)
}

pub(super) fn rook_reachable_row(occupied: BitBoard, pos: Square) -> BitBoard {
    ROOK_MAGIC_ROW[pos.index81()].reachable(occupied)
}

lazy_static! {
    static ref INNER: BitBoard = {
        let mut res = BitBoard::new();
        for col in 1..8 {
            for row in 1..8 {
                res.set(Square::new(col, row))
            }
        }
        res
    };
    static ref BISHOP_MAGIC: Vec<(Magic, Magic)> = {
        let mut res = vec![(Magic::zero(), Magic::zero()); 81];
        for pos in Square::iter() {
            res[pos.index81()] = (
                new_magic(pos, &[(-1, -1), (1, 1)]).unwrap(),
                new_magic(pos, &[(-1, 1), (1, -1)]).unwrap(),
            );
        }
        res
    };
    static ref ROOK_MAGIC_ROW: Vec<Magic> = {
        let mut res = vec![Magic::zero(); 81];
        for pos in Square::iter() {
            res[pos.index81()] = new_magic(pos, &[(-1, 0), (1, 0)]).unwrap();
        }
        res
    };
}

struct Pattern {
    block: BitBoard,
    reachable: BitBoard,
}

fn digest(block: BitBoard) -> usize {
    (block.x >> 64) as usize ^ block.x as usize
}

fn new_magic(pos: Square, dirs: &[(isize, isize)]) -> anyhow::Result<Magic> {
    let mut block_mask = BitBoard::new();
    for (dc, dr) in dirs {
        for i in 1..9 {
            if add(pos, dc * (i + 1), dr * (i + 1)).is_some() {
                block_mask.set(add(pos, dc * i, dr * i).unwrap());
            } else {
                break;
            }
        }
    }
    let patterns: Vec<Pattern> = block_mask
        .subsets()
        .map(|block| {
            let mut reachable = BitBoard::new();
            for (dc, dr) in dirs {
                for i in 1..9 {
                    match add(pos, dc * i, dr * i) {
                        Some(p) => {
                            reachable.set(p);
                            if block.get(p) {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
            Pattern { block, reachable }
        })
        .collect();
    let reachable_index = {
        let mut index = HashMap::new();
        for pattern in patterns.iter() {
            if index.contains_key(&pattern.reachable) {
                continue;
            }
            index.insert(pattern.reachable, index.len());
        }
        index
    };
    let mut targets = vec![vec![]; reachable_index.len()];
    for pattern in patterns.iter() {
        let i = reachable_index.get(&pattern.reachable).unwrap();
        targets[*i].push(digest(pattern.block));
    }
    let magic = MagicCore::new(&targets)?;
    let mut table = vec![BitBoard::new(); magic.table_len()];
    for pattern in patterns.iter() {
        table[magic.index(digest(pattern.block))] = pattern.reachable;
    }
    Ok(Magic {
        block_mask,
        magic,
        table,
    })
}

fn add(pos: Square, col: isize, row: isize) -> Option<Square> {
    let col = pos.col() as isize + col;
    let row = pos.row() as isize + row;
    if (0..9).contains(&col) && (0..9).contains(&row) {
        Some(Square::new(col as usize, row as usize))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::position::bitboard11::{testing::bitboard, Square};

    #[test]
    fn bishop_reachable() {
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
            super::bishop_reachable(occupied, Square::new(0, 0))
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
            super::bishop_reachable(occupied, Square::new(1, 1))
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
            super::bishop_reachable(occupied, Square::new(1, 2))
        );
    }
}
