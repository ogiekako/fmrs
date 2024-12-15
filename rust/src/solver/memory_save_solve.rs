use std::{cell::RefCell, collections::BTreeMap};

use fmrs_core::{
    piece::Color,
    position::{advance_old, previous, Digest, Movement, Position, PositionExt},
    solve::Solution,
};

pub(super) fn memory_save_solve(
    mut initial_position: Position,
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
) -> anyhow::Result<Vec<Solution>> {
    let mut black_movements = vec![];
    let mut current_white_positions = vec![];
    advance_old(&mut initial_position, &mut black_movements)?;
    let mut memo_white_positions = BTreeMap::new();
    while let Some(black_movement) = black_movements.pop() {
        let black_undo = initial_position.do_move(&black_movement);
        memo_white_positions.insert(initial_position.digest(), 0i32);
        current_white_positions.push(initial_position.clone());
        initial_position.undo_move(&black_undo);
    }

    let mut mate_positions = vec![];

    for half_step in 1i32.. {
        let mut all_next_white_positions = vec![];

        while let Some(mut white_position) = current_white_positions.pop() {
            let mut has_next_position = false;

            let mut white_movements = vec![];
            advance_old(&mut white_position, &mut white_movements)?;

            let mut white_position_is_deadend = true;
            while let Some(white_movement) = white_movements.pop() {
                has_next_position = true;
                if !mate_positions.is_empty() {
                    white_movements.clear();
                    break;
                }

                let white_undo = white_position.do_move(&white_movement);

                advance_old(&mut white_position, &mut black_movements)?;
                while let Some(black_movement) = black_movements.pop() {
                    let black_undo = white_position.do_move(&black_movement);

                    let digest = white_position.digest();
                    white_position_is_deadend = false;
                    if memo_white_positions.contains_key(&digest) {
                        white_position.undo_move(&black_undo);
                        continue;
                    }
                    memo_white_positions.insert(digest, half_step);
                    all_next_white_positions.push(white_position.clone());
                    white_position.undo_move(&black_undo);
                }

                white_position.undo_move(&white_undo);
            }

            if !has_next_position && !white_position.pawn_drop() {
                mate_positions.push(white_position);
            } else if white_position_is_deadend {
                let digest = white_position.digest();
                assert!(memo_white_positions.contains_key(&digest));
                memo_white_positions.remove(&digest);
            }
        }

        if !mate_positions.is_empty() || all_next_white_positions.is_empty() {
            break;
        }

        current_white_positions = all_next_white_positions;

        progress.unbounded_send(half_step as usize * 2)?;
        eprintln!(
            "step = {}, queue = {}, memo = {}",
            half_step * 2,
            current_white_positions.len(),
            memo_white_positions.len(),
        )
    }
    mate_positions.sort();
    mate_positions.dedup();

    let mut res = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || {
            let mut res = vec![];
            for mate_position in mate_positions {
                res.append(&mut reconstruct_solutions(
                    initial_position.digest(),
                    mate_position,
                    &memo_white_positions,
                    solutions_upto - res.len(),
                ));
            }
            res
        })?
        .join()
        .unwrap();
    res.sort();
    if !res.is_empty() {
        eprintln!("Solved in {} steps", res[0].len());
    }
    Ok(res)
}

fn reconstruct_solutions(
    initial_position_digest: Digest,
    mut mate_position: Position,
    memo_white_positions: &BTreeMap<Digest, i32>,
    solutions_upto: usize,
) -> Vec<Vec<Movement>> {
    let half_step = *memo_white_positions.get(&mate_position.digest()).unwrap();
    let ctx = Context::new(
        initial_position_digest,
        memo_white_positions,
        half_step,
        solutions_upto,
    );
    ctx.reconstruct_white(&mut mate_position, half_step);
    ctx.result.take()
}

struct Context<'a> {
    initial_position_digest: Digest,
    memo_white_positions: &'a BTreeMap<Digest, i32>,
    mate_in: i32,
    result: RefCell<Vec<Vec<Movement>>>,
    solution: RefCell<Vec<Movement>>, // reverse order
    solutions_upto: usize,
}

impl<'a> Context<'a> {
    fn new(
        initial_position_digest: Digest,
        memo_white_positions: &'a BTreeMap<Digest, i32>,
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

    fn reconstruct_white(&self, position: &mut Position, half_step: i32) {
        debug_assert_eq!(position.turn(), Color::WHITE);

        if self.result.borrow().len() >= self.solutions_upto {
            return;
        }

        for black_undo in previous(position.clone(), half_step < self.mate_in) {
            let black_movement = position.undo_move(&black_undo);
            self.solution.borrow_mut().push(black_movement);

            if position.checked_slow(Color::WHITE) {
                // Do nothing
            } else if half_step == 0 {
                if position.digest() == self.initial_position_digest {
                    self.push_solution();
                }
            } else {
                for white_undo in previous(position.clone(), true) {
                    let white_movement = position.undo_move(&white_undo);
                    self.solution.borrow_mut().push(white_movement);

                    if self.memo_white_positions.get(&position.digest()) == Some(&(half_step - 1)) {
                        self.reconstruct_white(position, half_step - 1);
                    }

                    self.solution.borrow_mut().pop();
                    position.do_move(&white_movement);
                }
            }

            self.solution.borrow_mut().pop();
            position.do_move(&black_movement);
        }
    }

    fn push_solution(&self) {
        self.result
            .borrow_mut()
            .push(self.solution.borrow().clone().into_iter().rev().collect());
    }
}
