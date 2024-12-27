use crate::memo::Memo;

use crate::piece::{Color, COLOR_BLACK, COLOR_WHITE};

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
    attack_preventing_movements::<COLOR_WHITE, COLOR_BLACK>(
        position, memo, next_step, false, options, None, result,
    )
}
