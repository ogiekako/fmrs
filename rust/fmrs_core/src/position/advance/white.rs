use rustc_hash::FxHashMap;

use crate::piece::{Color, Kind};

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::Digest;
use crate::position::Position;

pub(super) fn advance_old(position: &Position) -> anyhow::Result<Vec<Position>> {
    advance(position, &mut FxHashMap::default(), 0).map(|x| x.0)
}

pub(super) fn advance(
    position: &Position,
    memo: &mut FxHashMap<Digest, u32>,
    next_step: u32,
) -> anyhow::Result<(Vec<Position>, /* is mate */ bool)> {
    debug_assert_eq!(position.turn(), Color::White);
    let king_pos = position
        .bitboard(Color::White.into(), Kind::King.into())
        .next()
        .ok_or_else(|| anyhow::anyhow!("white king not found"))?;
    Ok(attack_preventing_movements(position, memo, next_step, king_pos, false).unwrap())
}
