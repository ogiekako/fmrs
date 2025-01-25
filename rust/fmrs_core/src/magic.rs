use std::{
    collections::HashMap,
    ops::{Index, Range},
};

use log::{debug, info};
use rand::{Rng, SeedableRng};

use crate::position::BitBoard;

#[derive(Clone, Debug)]
pub(crate) struct Magic<T> {
    pub(crate) pack_shift: u32,
    pub(crate) magic_shift: u32,
    pub(crate) full_block: BitBoard,
    pub(crate) magic: u64,
    pub(crate) table: T,
}

impl<T> Magic<T>
where
    T: Index<usize, Output = BitBoard>,
{
    pub(crate) fn f(&self, occupied: BitBoard) -> BitBoard {
        let packed = pack(occupied & self.full_block, self.pack_shift);
        self.table[(self.magic.wrapping_mul(packed) >> self.magic_shift) as usize]
    }
}

impl<T> Magic<T> {
    pub fn clone_with<S>(&self, table: S) -> Magic<S> {
        Magic {
            pack_shift: self.pack_shift,
            magic_shift: self.magic_shift,
            full_block: self.full_block,
            magic: self.magic,
            table,
        }
    }
}

pub struct MagicGenerator<R: SeedableRng + Rng> {
    rng: R,
    one_prob_per_100: Range<u8>,
    max_iter: usize,
    max_retry: usize,
}

impl<R: SeedableRng + Rng> MagicGenerator<R> {
    pub fn new(rng: R, one_prob_per_100: Range<u8>, max_iter: usize, max_retry: usize) -> Self {
        Self {
            rng,
            one_prob_per_100,
            max_iter,
            max_retry,
        }
    }

    pub(crate) fn gen_magic(
        &mut self,
        f: HashMap<BitBoard, BitBoard>,
        log_prefix: &str,
    ) -> anyhow::Result<Magic<Vec<BitBoard>>> {
        let mut full_block = BitBoard::EMPTY;
        for x in f.keys() {
            full_block |= *x;
        }

        let mut res = None;

        for _ in 0..self.max_retry {
            if res.is_some() {
                break;
            }
            for pack_shift in 0..64 {
                if res.is_some() {
                    break;
                }
                if pack(full_block, pack_shift).count_ones() != full_block.count_ones() {
                    continue;
                }

                let mut map: HashMap<_, Vec<u64>> = HashMap::new();
                for block in full_block.subsets() {
                    let reach = f
                        .get(&block)
                        .ok_or_else(|| anyhow::anyhow!("block not found: {block:?}"))?;
                    map.entry(reach).or_default().push(pack(block, pack_shift));
                }
                let smallest_table_log2 = map.len().next_power_of_two().ilog2();
                let largest_table_log2 = map
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
                    .next_power_of_two()
                    .ilog2();

                for table_log2 in (smallest_table_log2..=largest_table_log2).rev() {
                    let kvs = map.iter().collect::<Vec<_>>();
                    let values = kvs.iter().map(|(_, v)| *v).collect::<Vec<_>>();

                    let mut table: Vec<u8> = vec![0; 1 << table_log2];
                    for iter in 0..self.max_iter {
                        let magic = random_u64(&mut self.rng, &self.one_prob_per_100);

                        table.fill(u8::MAX);
                        if !create_table(&values, magic, &mut table, table_log2) {
                            continue;
                        };

                        info!(
                                "{log_prefix}: magic found (iter={iter}) {}/{} (pack_shift={pack_shift})",
                                1 << table_log2,
                                map.len(),
                            );

                        let bitboards: Vec<BitBoard> = table
                            .iter()
                            .map(|&x| kvs.get(x as usize).map(|x| **x.0).unwrap_or_default())
                            .collect();

                        res = Some(Magic {
                            pack_shift,
                            magic_shift: u64::BITS - table_log2,
                            full_block,
                            magic,
                            table: bitboards,
                        });
                        break;
                    }

                    if res.is_none() && table_log2 == largest_table_log2 {
                        debug!(
                                "{log_prefix}: magic not found after {} iterations (shift={pack_shift})",
                                self.max_iter,
                            );
                    }
                }
            }
        }
        res.ok_or_else(|| {
            anyhow::anyhow!(
                "{log_prefix}: magic not found after {} iterations and {} retries",
                self.max_iter,
                self.max_retry,
            )
        })
    }
}

fn pack(bb: BitBoard, shift: u32) -> u64 {
    let x = bb.u128();
    let lower = (x as u64).rotate_left(shift);
    let upper = (x >> 64) as u64;
    lower | upper
}

fn create_table(map: &Vec<&Vec<u64>>, magic: u64, table: &mut [u8], table_log2: u32) -> bool {
    let shift = u64::BITS - table_log2;
    for (i, &blocks) in map.iter().enumerate() {
        let i = i as u8;
        for &block in blocks {
            let j = (magic.wrapping_mul(block) >> shift) as usize;
            if table[j] == i {
                continue;
            }
            if table[j] != u8::MAX {
                return false;
            }
            table[j] = i;
        }
    }
    true
}

fn random_u64<R: Rng>(rng: &mut R, one_prob_per_100: &Range<u8>) -> u64 {
    let mut magic = 0;
    let one_prob_per_100 = rng.gen_range(one_prob_per_100.clone());
    for i in 0..u64::BITS {
        if rng.gen_range(0u8..100) < one_prob_per_100 {
            magic |= 1u64 << i;
        }
    }
    magic
}
