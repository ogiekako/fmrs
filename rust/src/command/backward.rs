use anyhow::Ok;
use fmrs_core::{piece::Color, position::position::PositionAux, search::backward::backward_search};

use super::parse_to_sfen;

pub fn backward(sfen_like: &str) -> anyhow::Result<()> {
    let sfen = parse_to_sfen(sfen_like)?;

    let mut position = PositionAux::from_sfen(&sfen)?;
    if position.checked_slow(Color::WHITE) {
        position.set_turn(Color::WHITE);
    }
    let (step, positions) = backward_search(&position, true)?;

    eprintln!("mate in {}:", step);
    for mut position in positions {
        eprintln!("{}", position.sfen_url());
        println!("{}", position.sfen());
    }
    Ok(())
}
