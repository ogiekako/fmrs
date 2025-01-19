use crate::{piece::Color, position::position::PositionAux};

use super::attack_prevent::attacker;

pub fn checked(position: &mut PositionAux, color: Color) -> bool {
    if color.is_black() && position.black_king_pos().is_none() {
        return false;
    }
    attacker(position, color, true).is_some()
}
