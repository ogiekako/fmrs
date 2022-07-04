use std::{cell::RefCell, collections::HashMap};

use crate::{
    position::{previous, Movement, Position, PositionExt},
    solver::solve::{digest, Digest},
};

pub(super) fn reconstruct_solutions(
    mut mate: Position,
    memo_black_turn: &HashMap<Digest, usize>,
    memo_white_turn: &HashMap<Digest, usize>,
) -> Vec<Vec<Movement>> {
    debug_assert!(memo_white_turn.contains_key(&digest(&mate)));
    let step = *memo_white_turn.get(&digest(&mate)).unwrap();
    let ctx = Context::new(memo_black_turn, memo_white_turn, step);
    ctx.reconstruct(&mut mate, step);
    ctx.result.take()
}

struct Context<'a> {
    memo_black_turn: &'a HashMap<Digest, usize>,
    memo_white_turn: &'a HashMap<Digest, usize>,
    mate_in: usize,
    result: RefCell<Vec<Vec<Movement>>>,
    solution: RefCell<Vec<Movement>>, // reverse order
}

impl<'a> Context<'a> {
    fn new(
        memo_black_turn: &'a HashMap<Digest, usize>,
        memo_white_turn: &'a HashMap<Digest, usize>,
        mate_in: usize,
    ) -> Self {
        Self {
            memo_black_turn,
            memo_white_turn,
            mate_in,
            result: vec![].into(),
            solution: vec![].into(),
        }
    }

    fn reconstruct(&self, position: &mut Position, step: usize) {
        let (memo, memo_previous) = if step % 2 == 0 {
            (self.memo_black_turn, self.memo_white_turn)
        } else {
            (self.memo_white_turn, self.memo_black_turn)
        };
        debug_assert!(memo.contains_key(&digest(position)));
        debug_assert_eq!(memo.get(&digest(position)), Some(&step));

        if step == 0 {
            self.push_solution();
            return;
        }

        let mut has_previous = false;
        for undo_move in previous(position.clone(), step < self.mate_in) {
            let movement = position.undo_move(&undo_move);
            if memo_previous.get(&digest(position)) == Some(&(step - 1)) {
                has_previous = true;
                self.solution.borrow_mut().push(movement);
                self.reconstruct(position, step - 1);
                self.solution.borrow_mut().pop().unwrap();
            }
            position.do_move(&movement);
        }
        assert!(has_previous, "previous not found: {:?}", position);
    }

    fn push_solution(&self) {
        self.result
            .borrow_mut()
            .push(self.solution.borrow().clone().into_iter().rev().collect());
    }
}
