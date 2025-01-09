use anyhow::Ok;
use fmrs_core::{piece::Color, position::position::PositionAux, search::backward::backward_search};

use super::parse_to_sfen;

pub fn backward(sfen_like: &str, forward: usize) -> anyhow::Result<()> {
    let sfen = parse_to_sfen(sfen_like)?;

    let mut position = PositionAux::from_sfen(&sfen)?;
    if position.checked_slow(Color::WHITE) {
        position.set_turn(Color::WHITE);
    }

    let builder = std::thread::Builder::new().stack_size(32 * 1024 * 1024); // 32 MB
    let handler = builder.spawn(move || {
        let (step, positions) = backward_search(&position, false, forward).unwrap();

        eprintln!("mate in {}:", step);
        for position in positions {
            eprintln!("{}", position.sfen_url());
            println!("{}", position.sfen());
        }
    })?;
    handler.join().unwrap();

    Ok(())
}
