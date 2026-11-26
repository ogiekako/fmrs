use crate::piece::Color;

use crate::position::advance::attack_prevent::{attack_preventing_movements, Attacker};
use crate::position::advance::options::AdvanceResult;
use crate::position::position::PositionAux;
use crate::position::Movement;

use super::AdvanceOptions;

pub(super) fn advance(
    position: &PositionAux,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> AdvanceResult</* legal mate */ bool> {
    debug_assert_eq!(position.turn(), Color::WHITE);
    attack_preventing_movements(position, false, options, None, result)
}

pub(super) fn advance_with_hint(
    position: &PositionAux,
    options: &AdvanceOptions,
    attacker_hint: Option<Attacker>,
    result: &mut Vec<Movement>,
) -> AdvanceResult</* legal mate */ bool> {
    debug_assert_eq!(position.turn(), Color::WHITE);
    attack_preventing_movements(position, false, options, attacker_hint, result)
}
