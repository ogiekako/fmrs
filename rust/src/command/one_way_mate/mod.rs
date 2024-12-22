pub(super) mod action;
mod beam;
pub mod solve;

use beam::generate_one_way_mate_with_beam;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OneWayMateGenerator {
    Beam,
}

pub fn one_way_mate(
    algo: OneWayMateGenerator,
    seed: u64,
    // Beam
    parallel: usize,
    goal: Option<usize>,
) -> anyhow::Result<()> {
    match algo {
        OneWayMateGenerator::Beam => generate_one_way_mate_with_beam(seed, parallel, goal),
    }
}
