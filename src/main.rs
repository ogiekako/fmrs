#[macro_use]
extern crate lazy_static;
extern crate arr_macro;
extern crate rand;
extern crate serde;

pub mod command;
pub mod piece;
pub mod position;
pub mod sfen;
pub mod solver;

use clap::Parser;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand)]
enum Action {
    Solve,
    Server,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.action {
        Action::Solve => command::solve()?,
        Action::Server => command::server(1234)?,
    }
    Ok(())
}
