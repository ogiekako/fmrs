use std::collections::HashMap;

use crate::position::bitboard::{BitBoard, Square};

use super::legacy_magic_core::LegacyMagicCore;

#[derive(Clone)]
pub(super) struct LegacyMagic {
    use_63: bool,
    block_mask_63: u64,
    block_mask: BitBoard,
    magic: LegacyMagicCore,
    table: Vec<BitBoard>,
}

impl LegacyMagic {
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
    MAGICS[pos.index()].bishop.reachable63(occupied)
}

pub(super) fn pro_bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    MAGICS[pos.index()].pro_bishop.reachable63(occupied)
}

pub(super) fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    MAGICS[pos.index()].rook_row.reachable63(occupied)
        | MAGICS[pos.index()].rook_col.reachable(occupied)
}

pub(super) fn pro_rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    MAGICS[pos.index()].pro_rook_row.reachable63(occupied)
        | MAGICS[pos.index()].rook_col.reachable(occupied)
}

fn one_to_eight(bb: BitBoard) -> u64 {
    (bb.u128() >> 9) as u64
}

pub fn init_magic() {
    let b = bishop_reachable(BitBoard::default(), Square::new(0, 0))
        | pro_bishop_reachable(BitBoard::default(), Square::new(0, 0))
        | rook_reachable(BitBoard::default(), Square::new(0, 0))
        | pro_rook_reachable(BitBoard::default(), Square::new(0, 0));
    assert!(!b.contains(Square::new(0, 0)));
}

struct Magics {
    bishop: LegacyMagic,
    rook_row: LegacyMagic,
    rook_col: LegacyMagic,
    pro_bishop: LegacyMagic,
    pro_rook_row: LegacyMagic,
}

lazy_static! {
    static ref MAGICS: [Magics; 81] = {
        std::array::from_fn(|i| Magics {
            bishop: BISHOP_MAGIC[i].clone(),
            rook_row: ROOK_MAGIC_ROW[i].clone(),
            rook_col: ROOK_MAGIC_COL[i].clone(),
            pro_bishop: PROBISHOP_MAGIC[i].clone(),
            pro_rook_row: PROROOK_MAGIC_ROW[i].clone(),
        })
    };
    static ref BISHOP_MAGIC: Vec<LegacyMagic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, -1), (1, 1), (-1, 1), (1, -1)], true, false).unwrap());
        }
        res
    };
    static ref PROBISHOP_MAGIC: Vec<LegacyMagic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, -1), (1, 1), (-1, 1), (1, -1)], true, true).unwrap());
        }
        res
    };
    static ref ROOK_MAGIC_ROW: Vec<LegacyMagic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, 0), (1, 0)], true, false).unwrap());
        }
        res
    };
    static ref PROROOK_MAGIC_ROW: Vec<LegacyMagic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(-1, 0), (1, 0)], true, true).unwrap());
        }
        res
    };
    static ref ROOK_MAGIC_COL: Vec<LegacyMagic> = {
        let mut res = vec![];
        for pos in Square::iter() {
            res.push(new_magic(pos, &[(0, -1), (0, 1)], false, false).unwrap());
        }
        res
    };
}

struct Pattern {
    block_digest: u64,
    reachable: BitBoard,
}

fn new_magic(
    pos: Square,
    dirs: &[(isize, isize)],
    use_63: bool,
    with_king: bool,
) -> anyhow::Result<LegacyMagic> {
    let mut block_mask = BitBoard::default();
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
            let mut reachable = BitBoard::default();
            for (dc, dr) in dirs {
                for i in 1..9 {
                    match add(pos, dc * i, dr * i) {
                        Some(p) => {
                            reachable.set(p);
                            if block.contains(p) {
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
            if with_king {
                reachable |= super::king_power(pos);
            }
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
    let magic = LegacyMagicCore::new(&targets)?;
    let mut table = vec![BitBoard::default(); magic.table_len()];
    for pattern in patterns.iter() {
        table[magic.index(pattern.block_digest) as usize] = pattern.reachable;
    }
    Ok(LegacyMagic {
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
    use crate::{bitboard, position::bitboard::{legacy_magic, Square}};

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
            legacy_magic::bishop_reachable(occupied, Square::new(0, 0))
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
            legacy_magic::bishop_reachable(occupied, Square::new(1, 1))
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
            legacy_magic::bishop_reachable(occupied, Square::new(1, 2))
        );
    }
}
