use crate::memo::Memo;

use crate::piece::{Color, Kind};

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::position::PositionAux;
use crate::position::{Movement, Position};

use super::AdvanceOptions;

pub(super) fn advance(
    position: &mut Position,
    memo: &mut Memo,
    next_step: u32,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> anyhow::Result</* legal mate */ bool> {
    debug_assert_eq!(position.turn(), Color::WHITE);

    let king_pos = position
        .bitboard(Color::WHITE, Kind::King)
        .next()
        .ok_or_else(|| anyhow::anyhow!("white king not found"))?;

    let mut position = PositionAux::new(position);

    attack_preventing_movements(
        &mut position,
        memo,
        next_step,
        king_pos,
        false,
        options,
        None,
        result,
    )
}
