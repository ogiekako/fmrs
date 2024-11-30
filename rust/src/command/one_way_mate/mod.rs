mod sa;

use sa::generate_one_way_mate_with_sa;

pub async fn one_way_mate(seed: u64, iteration: usize) -> anyhow::Result<()> {
    generate_one_way_mate_with_sa(seed, iteration)
}
