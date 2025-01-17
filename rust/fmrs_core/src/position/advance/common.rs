use crate::{piece::Color, position::controller::PositionController};

use super::attack_prevent::attacker;

pub fn checked(controller: &mut PositionController, color: Color) -> bool {
    if color.is_black() && controller.black_king_pos().is_none() {
        return false;
    }
    attacker(controller, color, true).is_some()
}
