use anyhow::bail;
use rand::{Rng, SeedableRng};

#[derive(Clone)]
pub(super) struct LegacyMagicCore {
    magic: u64,
    shift: u64,
}

const GIVE_UP: usize = 100_000;
impl LegacyMagicCore {
    pub(super) fn new(targets: &[Vec<u64>]) -> anyhow::Result<Self> {
        let mut n = 0;
        for ts in targets {
            n += ts.len();
        }
        let shift = (n as u64).leading_zeros() as u64;

        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        for _ in 0..GIVE_UP {
            let mut magic = 0;
            for i in 0..u64::BITS {
                let one_probability = 15;
                if rng.gen_range(0u8..100) < one_probability {
                    magic |= 1u64 << i;
                }
            }
            let cand = Self { magic, shift };
            if is_valid_magic(&cand, targets) {
                return Ok(cand);
            }
        }
        bail!("magic not found: {:?}", targets);
    }

    pub(super) fn index(&self, target: u64) -> u64 {
        target.wrapping_mul(self.magic) >> self.shift
    }

    pub(super) fn table_len(&self) -> usize {
        1usize << (u64::BITS as u64 - self.shift)
    }
}

fn is_valid_magic(magic: &LegacyMagicCore, targets: &[Vec<u64>]) -> bool {
    let mut mapping = vec![None; magic.table_len()];
    for (i, ts) in targets.iter().enumerate() {
        for target in ts {
            let j = magic.index(*target) as usize;
            if mapping[j].is_some() && mapping[j] != Some(i as u64) {
                return false;
            }
            mapping[j] = Some(i as u64)
        }
    }
    true
}
