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
    Bench {
        file: String,
    },
    Solve {
        #[clap(value_enum)]
        algorithm: Algorithm,
        sfen_or_file: Option<String>,
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
        #[arg(long, default_value = "12")]
        parallel: usize,
    },
    FromImage {
        url: String,
    },
}

pub async fn do_main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match args.action {
        Action::Bench { file } => command::bench(&file)?,
        Action::Solve {
            algorithm,
            sfen_or_file,
        } => command::solve(algorithm, sfen_or_file)?,
        Action::Server => command::server(1234).await?,
        Action::OneWayMate {
            algorithm,
            seed,
            iteration,
            parallel,
        } => command::one_way_mate(algorithm, seed, iteration, parallel)?,
        Action::FromImage { url } => println!("{}", sfen::from_image_url(&url)?),
    }
    Ok(())
}
