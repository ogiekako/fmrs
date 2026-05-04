#![allow(clippy::needless_range_loop, clippy::module_inception)]

use clap::{Parser, Subcommand};
use command::{
    backward::backward,
    batch_square::batch_square,
    bench::BenchCommand,
    magic::{gen_magic, MagicAttribute},
    OneWayMateGenerator, SingleKingSmokeCommand,
};
use solver::Algorithm;

pub mod bit;
mod command;
pub mod opt;
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
        #[arg(long)]
        solutions_upto: Option<usize>,
    },
    Solve {
        #[clap(value_enum)]
        algorithm: Algorithm,
        sfen_like: Option<String>,
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
    BatchSquare {
        filter_file: Option<String>,
    },
    Backward {
        sfen_like: Option<String>,
        #[arg(long, default_value = "0")]
        forward: usize,
        #[arg(long)]
        parallel: Option<usize>,
        #[arg(long, default_value_t = false)]
        allow_white: bool,
        #[arg(long, default_value_t = false)]
        one_way: bool,
        #[arg(long, default_value_t = false)]
        no_black_goldish: bool,
        #[arg(long, default_value_t = false)]
        bare_white_king: bool,
        #[arg(long)]
        dump_frontier_dir: Option<String>,
        #[arg(long)]
        resume_frontier: Option<String>,
    },
    GenMagic {
        attr: MagicAttribute,
    },
    SingleKingSmoke {
        #[command(subcommand)]
        cmd: SingleKingSmokeCommand,
    },
}

pub async fn do_main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match args.action {
        Action::Bench {
            cmd,
            file,
            algo,
            solutions_upto,
        } => command::bench(cmd, algo, &file, solutions_upto)?,
        Action::Solve {
            algorithm,
            sfen_like,
        } => command::solve(algorithm, sfen_like)?,
        Action::Server => command::server(1234).await?,
        Action::OneWayMate {
            algorithm,
            seed,
            parallel,
            goal,
        } => command::one_way_mate(algorithm, seed, parallel, goal)?,
        Action::BatchSquare { filter_file } => {
            batch_square(filter_file)?;
        }
        Action::Backward {
            sfen_like,
            forward,
            parallel,
            allow_white,
            one_way,
            no_black_goldish,
            bare_white_king,
            dump_frontier_dir,
            resume_frontier,
        } => backward(
            sfen_like.as_deref(),
            forward,
            parallel,
            !allow_white,
            one_way,
            no_black_goldish,
            bare_white_king,
            dump_frontier_dir.as_deref(),
            resume_frontier.as_deref(),
        )?,
        Action::GenMagic { attr } => gen_magic(attr)?,
        Action::SingleKingSmoke { cmd } => command::single_king_smoke(cmd)?,
    }
    Ok(())
}
