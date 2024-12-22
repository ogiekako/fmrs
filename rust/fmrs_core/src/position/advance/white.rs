use crate::memo::Memo;

use crate::piece::{Color, Kind};

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::position::PositionAux;
use crate::position::Movement;

use super::AdvanceOptions;

pub(super) fn advance<'a>(
    position: &'a mut PositionAux,
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

    attack_preventing_movements(
        position, memo, next_step, king_pos, false, options, None, result,
    )
}
