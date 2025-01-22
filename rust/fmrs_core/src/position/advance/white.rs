use crate::piece::Color;

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::position::PositionAux;
use crate::position::Movement;

use super::AdvanceOptions;

pub(super) fn advance(
    position: &mut PositionAux,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> anyhow::Result</* legal mate */ bool> {
    debug_assert_eq!(position.turn(), Color::WHITE);
    attack_preventing_movements(position, false, options, None, result)
}
