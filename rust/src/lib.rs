#![allow(clippy::needless_range_loop, clippy::module_inception)]

use clap::{Parser, Subcommand};
pub use command::one_way_mate_steps;
use command::{bench::BenchCommand, OneWayMateGenerator};
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
        cmd: BenchCommand,
        #[arg(long, default_value = "./problems/forest-06-10_97.sfen")]
        file: String,
        #[arg(long, default_value = "standard")]
        #[clap(value_enum)]
        algo: Algorithm,
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
        #[arg(long, default_value = "12")]
        parallel: usize,
        #[arg(long)]
        goal: Option<usize>,
    },
    FromImage {
        url: String,
    },
    DirectMate {
        sfen_or_file: String,
    },
}

pub async fn do_main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match args.action {
        Action::Bench { cmd, file, algo } => command::bench(cmd, algo, &file)?,
        Action::Solve {
            algorithm,
            sfen_or_file,
        } => command::solve(algorithm, sfen_or_file)?,
        Action::Server => command::server(1234).await?,
        Action::OneWayMate {
            algorithm,
            seed,
            parallel,
            goal,
        } => command::one_way_mate(algorithm, seed, parallel, goal)?,
        Action::FromImage { url } => println!("{}", sfen::from_image_url(&url)?),
        Action::DirectMate { sfen_or_file } => {
            command::direct_mate(&if sfen_or_file.ends_with(".sfen") {
                std::fs::read_to_string(sfen_or_file)?
            } else {
                sfen_or_file
            })?;
        }
    }
    Ok(())
}
