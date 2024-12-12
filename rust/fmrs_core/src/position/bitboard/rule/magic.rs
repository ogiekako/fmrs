use std::collections::HashMap;

use crate::position::bitboard::{BitBoard, Square};

use super::magic_core::MagicCore;

#[derive(Clone)]
pub(super) struct Magic {
    use_63: bool,
    block_mask_63: u64,
    block_mask: BitBoard,
    magic: MagicCore,
    table: Vec<BitBoard>,
}

impl Magic {
    fn reachable63(&self, occupied: BitBoard) -> BitBoard {
        debug_assert!(self.use_63);
        let block = self.block_mask_63 & one_to_eight(occupied);
        self.table[self.magic.index(block) as usize]
    }

    fn reachable(&self, occupied: BitBoard) -> BitBoard {
        debug_assert!(!self.use_63);
        let block = (self.block_mask & occupied).digest();
        self.table[self.magic.index(block) as usize]
    }
}

pub(super) fn bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    BISHOP_MAGIC[pos.index()].reachable63(occupied)
}

#[inline(always)]
pub(super) fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    ROOK_MAGIC_ROW[pos.index()].reachable63(occupied)
        | ROOK_MAGIC_COL[pos.index()].reachable(occupied)
}

fn one_to_eight(bb: BitBoard) -> u64 {
    (bb.u128() >> 9) as u64
}

lazy_static! {
    static ref BISHOP_MAGIC: Vec<Magic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, -1), (1, 1), (-1, 1), (1, -1)], true).unwrap());
        }
        res
    };
    static ref ROOK_MAGIC_ROW: Vec<Magic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, 0), (1, 0)], true).unwrap());
        }
        res
    };
    static ref ROOK_MAGIC_COL: Vec<Magic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(0, -1), (0, 1)], false).unwrap());
        }
        res
    };
}

struct Pattern {
    block_digest: u64,
    reachable: BitBoard,
}

fn new_magic(pos: Square, dirs: &[(isize, isize)], use_63: bool) -> anyhow::Result<Magic> {
    let mut block_mask = BitBoard::empty();
    for (dc, dr) in dirs {
        for i in 1..9 {
            if add(pos, dc * (i + 1), dr * (i + 1)).is_some() {
                block_mask.set(add(pos, dc * i, dr * i).unwrap());
            } else {
                break;
            }
        }
    }
    let block_mask_63 = one_to_eight(block_mask);

    let patterns: Vec<Pattern> = block_mask
        .subsets()
        .map(|block| {
            let mut reachable = BitBoard::empty();
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
            let block_digest = if use_63 {
                one_to_eight(block)
            } else {
                block.digest()
            };
            Pattern {
                block_digest,
                reachable,
            }
        })
        .collect();

    let reachable_index = {
        let mut index = HashMap::new();
        for pattern in patterns.iter() {
            if index.contains_key(&pattern.reachable.u128()) {
                continue;
            }
            index.insert(pattern.reachable.u128(), index.len());
        }
        index
    };
    let mut targets = vec![vec![]; reachable_index.len()];
    for pattern in patterns.iter() {
        let i = reachable_index.get(&pattern.reachable.u128()).unwrap();
        targets[*i].push(pattern.block_digest);
    }
    let magic = MagicCore::new(&targets)?;
    let mut table = vec![BitBoard::empty(); magic.table_len()];
    for pattern in patterns.iter() {
        table[magic.index(pattern.block_digest) as usize] = pattern.reachable;
    }
    Ok(Magic {
        use_63,
        block_mask,
        block_mask_63,
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
    use crate::position::bitboard::{testing::bitboard, Square};

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
