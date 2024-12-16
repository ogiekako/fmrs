use crate::memo::Memo;

use crate::position::{self, Position, PositionExt};

use super::{reconstruct_solutions, Solution};
use log::info;

pub struct StandardSolver {
    solutions_upto: usize,
    step: u32,
    current: Vec<Position>,
    memo: Memo,
    memo_next: Memo,
}

#[derive(PartialEq, Eq)]
pub enum SolverStatus {
    Intermediate,
    Mate(Vec<Solution>),
    NoSolution,
}

impl StandardSolver {
    pub fn new(position: Position, solutions_upto: usize) -> Self {
        let step = 0;
        let current = vec![position];
        let mut memo = Memo::default();
        memo.insert(current[0].digest(), step);
        Self {
            solutions_upto,
            step,
            current,
            memo,
            memo_next: Memo::default(),
        }
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        self.step += 1;

        let mut mate_positions = vec![];
        let mut all_next_positions = vec![];
        let mut movements = vec![];

        while let Some(mut position) = self.current.pop() {
            let is_mate = position::advance(
                &mut position,
                &mut self.memo_next,
                self.step,
                &Default::default(),
                &mut movements,
            )?;
            if is_mate {
                mate_positions.push(position);
                continue;
            }
            while let Some(movement) = movements.pop() {
                let undo = position.do_move(&movement);
                all_next_positions.push(position.clone());
                position.undo_move(&undo);
            }
        }

        if !mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in mate_positions.iter() {
                res.append(&mut reconstruct_solutions(
                    mate_position,
                    &self.memo_next,
                    &self.memo,
                    self.solutions_upto - res.len(),
                ))
            }
            assert_ne!(res.len(), 0, "{:?}", mate_positions);

            res.sort();
            info!(
                "Found {} solutions searching {} positions",
                res.len(),
                self.memo.len() + self.memo_next.len(),
            );
            return Ok(SolverStatus::Mate(res));
        }
        if all_next_positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        std::mem::swap(&mut self.memo, &mut self.memo_next);
        std::mem::swap(&mut self.current, &mut all_next_positions);
        Ok(SolverStatus::Intermediate)
    }
}
