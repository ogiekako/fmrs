use anyhow::bail;
use rand::{Rng, SeedableRng};

#[derive(Clone)]
pub(super) struct MagicCore {
    magic: usize,
    shift: usize,
}

const GIVE_UP: usize = 100_000;
impl MagicCore {
    pub(super) fn new(targets: &[Vec<usize>]) -> anyhow::Result<Self> {
        let mut n = 0;
        for ts in targets {
            n += ts.len();
        }
        let shift = n.leading_zeros() as usize;

        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        for _ in 0..GIVE_UP {
            let mut magic = 0;
            for i in 0..usize::BITS {
                let one_probability = 15;
                if rng.gen_range(0u8..100) < one_probability {
                    magic |= 1 << i;
                }
            }
            let cand = Self { magic, shift };
            if is_valid_magic(&cand, targets) {
                return Ok(cand);
            }
        }
        bail!("magic not found: {:?}", targets);
    }

    pub(super) fn index(&self, target: usize) -> usize {
        target.wrapping_mul(self.magic) >> self.shift
    }

    pub(super) fn table_len(&self) -> usize {
        1 << (usize::BITS as usize - self.shift)
    }
}

fn is_valid_magic(magic: &MagicCore, targets: &[Vec<usize>]) -> bool {
    let mut mapping = vec![None; magic.table_len()];
    for (i, ts) in targets.iter().enumerate() {
        for target in ts {
            let j = magic.index(*target);
            if mapping[j] != None && mapping[j] != Some(i) {
                return false;
            }
            mapping[j] = Some(i)
        }
    }
    true
}
