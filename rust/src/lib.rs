#![allow(clippy::needless_range_loop)]
#![allow(clippy::module_inception)]

use clap::{Parser, Subcommand};
pub use command::one_way_mate_steps;
use command::OneWayMateGenerator;
use fmrs_core::sfen;
use solver::Algorithm;

mod command;
pub mod solver;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    Bench,
    Solve {
        #[clap(value_enum)]
        algorithm: Algorithm,
        sfen: Option<String>,
    },
    Server,
    OneWayMate {
        #[arg(long, default_value = "beam")]
        #[clap(value_enum)]
        algorithm: OneWayMateGenerator,
        #[arg(long, default_value = "0")]
        seed: u64,
        #[arg(long, default_value = "100000000")] // 100M
        iteration: usize,
        #[arg(long, default_value = "2000")]
        start: usize,
        #[arg(long, default_value = "100000")] // 100K
        bucket: usize,
    },
    FromImage {
        url: String,
    },
}

pub async fn do_main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match args.action {
        Action::Bench => command::bench()?,
        Action::Solve { algorithm, sfen } => command::solve(algorithm, sfen).await?,
        Action::Server => command::server(1234).await?,
        Action::OneWayMate {
            algorithm,
            seed,
            iteration,
            start,
            bucket,
        } => command::one_way_mate(algorithm, seed, iteration, start, bucket).await?,
        Action::FromImage { url } => println!("{}", sfen::from_image_url(&url)?),
    }
    Ok(())
}
