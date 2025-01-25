use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Index, Range},
};

use anyhow::bail;
use log::{debug, info};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use rand::{Rng, SeedableRng};

use crate::codegen::{DefTokens, DefWriter};

#[derive(Clone, Debug)]
pub(crate) struct Magic<V, T> {
    pub(crate) magic: u64,
    pub(crate) magic_shift: u32,
    pub(crate) table: T,
    pub(crate) _phantom: std::marker::PhantomData<V>,
}

impl<V, T: DefTokens> DefTokens for Magic<V, T> {
    fn def_tokens(w: &mut DefWriter) {
        T::def_tokens(w);
        w.write(
            "Magic",
            quote! {
                use crate::magic::Magic;
            },
        );
    }
}

impl<V, T: ToTokens> ToTokens for Magic<V, T> {
    fn to_tokens(&self, w: &mut TokenStream) {
        let magic_shift = self.magic_shift;
        let magic = self.magic;
        let table = &self.table;
        w.append_all(quote! {
            Magic::new(#magic, #magic_shift, #table)
        });
    }
}

impl<V, T> Magic<V, T> {
    pub(crate) const fn new(mut magic: u64, mut magic_shift: u32, table: T) -> Self {
        if magic_shift == u64::BITS {
            magic_shift = 0;
            magic = 0;
        }
        Magic {
            magic,
            magic_shift,
            table,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<V, T> Magic<V, T>
where
    T: Index<usize, Output = V>,
{
    pub(crate) fn f(&self, a: u64) -> &V {
        &self.table[(self.magic.wrapping_mul(a) >> self.magic_shift) as usize]
    }
}

impl<V, T> Magic<V, T> {
    pub fn clone_with<S>(&self, table: S) -> Magic<V, S> {
        Magic::new(self.magic, self.magic_shift, table)
    }
}

pub struct MagicGenerator<R: SeedableRng + Rng> {
    rng: R,
    one_prob_per_100: Range<u8>,
    max_iter: usize,
    relax: u32,
}

impl<R: SeedableRng + Rng> MagicGenerator<R> {
    pub fn new(rng: R, one_prob_per_100: Range<u8>, max_iter: usize, relax: u32) -> Self {
        Self {
            rng,
            one_prob_per_100,
            max_iter,
            relax,
        }
    }

    pub fn rng(&mut self) -> &mut R {
        &mut self.rng
    }

    pub(crate) fn gen_magic<V: Clone + Default + Eq + Hash>(
        &mut self,
        f: HashMap<u64, V>,
        log_prefix: &str,
    ) -> anyhow::Result<Magic<V, Vec<V>>> {
        let mut res = None;

        let mut rev_f: HashMap<V, Vec<u64>> = HashMap::new();
        for (&k, v) in f.iter() {
            rev_f.entry(v.clone()).or_default().push(k);
        }
        let kvs = rev_f.iter().collect::<Vec<_>>();
        let values = kvs.iter().map(|(_, v)| *v).collect::<Vec<_>>();

        let smallest_table_log2 = rev_f.len().next_power_of_two().ilog2();
        let total_len = rev_f.values().map(Vec::len).sum::<usize>();
        let largest_table_log2 = total_len.next_power_of_two().ilog2() + self.relax;

        debug!(
            "{}: gen_magic {}/{}/{}",
            log_prefix,
            1 << largest_table_log2,
            total_len,
            rev_f.len()
        );

        for table_log2 in (smallest_table_log2..=largest_table_log2).rev() {
            let mut table: Vec<u16> = vec![0; 1 << table_log2];
            for iter in 0..self.max_iter {
                let magic = random_u64(&mut self.rng, &self.one_prob_per_100);

                table.fill(u16::MAX);
                if !create_table(&values, magic, &mut table, table_log2) {
                    continue;
                };

                info!(
                    "{log_prefix}: magic found (iter={iter}) {}/{}",
                    1 << table_log2,
                    rev_f.len(),
                );

                let values: Vec<V> = table
                    .iter()
                    .map(|&x| kvs.get(x as usize).map(|x| x.0.clone()).unwrap_or_default())
                    .collect();

                res = Some(Magic::new(magic, u64::BITS - table_log2, values));
                break;
            }

            if res.is_none() && table_log2 == largest_table_log2 {
                bail!(
                    "{log_prefix}: magic not found after {} iterations {}/{}/{}",
                    self.max_iter,
                    1 << table_log2,
                    total_len,
                    rev_f.len(),
                );
            }
        }
        Ok(res.unwrap())
    }
}

fn create_table(map: &Vec<&Vec<u64>>, magic: u64, table: &mut [u16], table_log2: u32) -> bool {
    let shift = u64::BITS - table_log2;
    for (i, &blocks) in map.iter().enumerate() {
        let i = i as u16;
        for &block in blocks {
            let j = if shift == u64::BITS {
                0
            } else {
                (magic.wrapping_mul(block) >> shift) as usize
            };
            if table[j] == i {
                continue;
            }
            if table[j] != u16::MAX {
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
