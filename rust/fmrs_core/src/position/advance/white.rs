use crate::memo::MemoTrait;

use crate::piece::Color;

use crate::position::advance::attack_prevent::attack_preventing_movements;
use crate::position::controller::PositionController;
use crate::position::Movement;

use super::AdvanceOptions;

pub(super) fn advance<M: MemoTrait>(
    controller: &mut PositionController,
    memo: &mut M,
    next_step: u16,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> anyhow::Result</* legal mate */ bool> {
    debug_assert_eq!(controller.turn(), Color::WHITE);
    attack_preventing_movements(controller, memo, next_step, false, options, None, result)
}

#[cfg(test)]
mod tests {
    use crate::{
        memo::Memo,
        position::{controller::PositionController, position::PositionAux},
    };

    #[test]
    fn test_white_advance() {
        for (sfen, want) in [
            (
                "9/G2s4G/LLpNGNpPP/4L4/1sN3N2/1g1bpb1ss/1pL3R2/P4k3/1PPPPP2K w Pr5p 1",
                3,
            ),
            (
                "9/G2s4G/LLpNGNpPP/4L4/1sN3N2/1g1bpb1ss/1pL3k2/P5P2/1PPPPP2K w 2r5p 1",
                4,
            ),
            (
                "9/G2s4G/LLpNGNpPP/4L4/1sN3N2/1g1bpbkss/1pL3P2/P8/1PPPPP2K w 2r5p 1",
                10,
            ),
        ] {
            let position = PositionAux::from_sfen(sfen).unwrap();
            let mut result = vec![];

            let mut controller =
                PositionController::new(position.core().clone(), *position.stone());

            super::advance(
                &mut controller,
                &mut Memo::default(),
                0,
                &Default::default(),
                &mut result,
            )
            .unwrap();

            assert_eq!(result.len(), want, "{:?}", result);
        }
    }
}
