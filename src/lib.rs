use clap::Parser;
use solver::Algorithm;

mod command;
mod converter;
mod jkf;
mod piece;
mod position;
mod sfen;
mod solver;
#[macro_use]
extern crate lazy_static;

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
    },
    Server,
}

pub async fn do_main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.action {
        Action::Bench => command::bench()?,
        Action::Solve { algorithm } => command::solve(algorithm).await?,
        Action::Server => command::server(1234).await?,
    }
    Ok(())
}
