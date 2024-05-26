#![allow(clippy::needless_range_loop)]
#![allow(clippy::module_inception)]

use clap::Parser;
use solver::Algorithm;

mod command;
pub mod solver;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand)]
enum Action {
    Bench,
    Solve {
        #[clap(value_enum)]
        algorithm: Algorithm,
        sfen: Option<String>,
    },
    Server,
}

pub async fn do_main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.action {
        Action::Bench => command::bench()?,
        Action::Solve { algorithm, sfen } => command::solve(algorithm, sefn).await?,
        Action::Server => command::server(1234).await?,
    }
    Ok(())
}
