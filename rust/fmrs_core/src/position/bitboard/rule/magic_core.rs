use anyhow::bail;
use rand::{rngs::SmallRng, Rng, SeedableRng};

#[derive(Clone)]
pub(super) struct MagicCore {
    magic: u64,
    shift: u64,
}

const GIVE_UP: usize = 100_000;
impl MagicCore {
    pub(super) fn new(targets: &[Vec<u64>]) -> anyhow::Result<Self> {
        let mut n = 0;
        for ts in targets {
            n += ts.len();
        }
        let shift = (n as u64).leading_zeros() as u64;

        let mut rng = SmallRng::seed_from_u64(0);
        for _ in 0..GIVE_UP {
            let cand = Self {
                magic: rng.gen(),
                shift,
            };
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

fn is_valid_magic(magic: &MagicCore, targets: &[Vec<u64>]) -> bool {
    let mut mapping = vec![None; magic.table_len()];
    for (i, ts) in targets.iter().enumerate() {
        for target in ts {
            let j = magic.index(*target) as usize;
            if mapping[j] != None && mapping[j] != Some(i as u64) {
                return false;
            }
            mapping[j] = Some(i as u64)
        }
    }
    true
}
