pub(super) mod action;
mod beam;
mod sa;
pub mod solve;

use beam::generate_one_way_mate_with_beam;
use sa::generate_one_way_mate_with_sa;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OneWayMateGenerator {
    Beam,
    Sa,
}

pub fn one_way_mate(
    algo: OneWayMateGenerator,
    seed: u64,
    // SA
    iteration: usize,
    // Beam
    start: usize,
    parallel: usize,
) -> anyhow::Result<()> {
    match algo {
        OneWayMateGenerator::Beam => generate_one_way_mate_with_beam(seed, start, parallel),
        OneWayMateGenerator::Sa => generate_one_way_mate_with_sa(seed, iteration),
    }
}
