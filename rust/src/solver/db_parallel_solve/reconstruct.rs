use std::cell::RefCell;

use crate::{
    piece::Color,
    position::{previous, Digest, Movement, Position, PositionExt},
};

use super::db::{Database, DatabaseGet};

pub(super) fn reconstruct_solutions(
    initial_position_digest: Digest,
    mut mate_position: Position,
    memo_white_positions: &Database,
    solutions_upto: usize,
) -> anyhow::Result<Vec<Vec<Movement>>> {
    let half_step = memo_white_positions.get(&mate_position.digest())?.unwrap();
    let ctx = Context::new(
        initial_position_digest,
        memo_white_positions,
        half_step,
        solutions_upto,
    );
    ctx.reconstruct_white(&mut mate_position, half_step)?;
    Ok(ctx.result.take())
}

struct Context<'a> {
    initial_position_digest: Digest,
    memo_white_positions: &'a Database,
    mate_in: i32,
    result: RefCell<Vec<Vec<Movement>>>,
    solution: RefCell<Vec<Movement>>, // reverse order
    solutions_upto: usize,
}

impl<'a> Context<'a> {
    fn new(
        initial_position_digest: Digest,
        memo_white_positions: &'a Database,
        mate_in: i32,
        solutions_upto: usize,
    ) -> Self {
        Self {
            initial_position_digest,
            memo_white_positions,
            mate_in,
            result: vec![].into(),
            solution: vec![].into(),
            solutions_upto,
        }
    }

    fn reconstruct_white(&self, position: &mut Position, half_step: i32) -> anyhow::Result<()> {
        debug_assert_eq!(position.turn(), Color::White);

        if self.result.borrow().len() >= self.solutions_upto {
            return Ok(());
        }

        for black_undo in previous(position.clone(), half_step < self.mate_in) {
            let black_movement = position.undo_move(&black_undo);
            self.solution.borrow_mut().push(black_movement);

            if position.checked_slow(Color::White) {
                // Do nothing
            } else if half_step == 0 {
                if position.digest() == self.initial_position_digest {
                    self.push_solution();
                }
            } else {
                for white_undo in previous(position.clone(), true) {
                    let white_movement = position.undo_move(&white_undo);
                    self.solution.borrow_mut().push(white_movement);

                    if self.memo_white_positions.get(&position.digest())? == Some(half_step - 1) {
                        self.reconstruct_white(position, half_step - 1)?;
                    }

                    self.solution.borrow_mut().pop();
                    position.do_move(&white_movement);
                }
            }

            self.solution.borrow_mut().pop();
            position.do_move(&black_movement);
        }
        Ok(())
    }

    fn push_solution(&self) {
        self.result
            .borrow_mut()
            .push(self.solution.borrow().clone().into_iter().rev().collect());
    }
}
