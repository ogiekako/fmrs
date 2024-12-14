use crate::nohash::NoHashMap;

use crate::piece::{Color, Kind};

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::Position;

use super::AdvanceOptions;

pub(super) fn advance_old(
    position: &mut Position,
    result: &mut Vec<Position>,
) -> anyhow::Result<()> {
    advance(
        position,
        &mut NoHashMap::default(),
        0,
        &AdvanceOptions::default(),
        result,
    )?;
    Ok(())
}

pub(super) fn advance(
    position: &mut Position,
    memo: &mut NoHashMap<u32>,
    next_step: u32,
    options: &AdvanceOptions,
    result: &mut Vec<Position>,
) -> anyhow::Result</* legal mate */ bool> {
    debug_assert_eq!(position.turn(), Color::WHITE);
    let king_pos = position
        .bitboard(Color::WHITE, Kind::King)
        .next()
        .ok_or_else(|| anyhow::anyhow!("white king not found"))?;
    attack_preventing_movements(
        position, memo, next_step, king_pos, false, options, None, result,
    )
}
