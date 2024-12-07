use rustc_hash::FxHashMap;

use crate::position::{self, Digest, Position};

use super::{reconstruct_solutions, Solution};

pub struct StandardSolver {
    solutions_upto: usize,
    step: u32,
    current: Vec<Position>,
    memo: FxHashMap<Digest, u32>,
    memo_next: FxHashMap<Digest, u32>,
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
        let mut memo = FxHashMap::default();
        memo.insert(current[0].digest(), step);
        Self {
            solutions_upto,
            step,
            current,
            memo,
            memo_next: FxHashMap::default(),
        }
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        self.step += 1;

        let mut mate_positions = vec![];
        let mut all_next_positions = vec![];
        while let Some(position) = self.current.pop() {
            let (mut new_next_positions, is_mate) =
                position::advance(&position, &mut self.memo_next, self.step)?;
            all_next_positions.append(&mut new_next_positions);
            if is_mate && !position.pawn_drop() {
                mate_positions.push(position);
            }
        }

        if !mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in mate_positions {
                res.append(&mut reconstruct_solutions(
                    &mate_position,
                    &self.memo_next,
                    &self.memo,
                    self.solutions_upto - res.len(),
                ))
            }
            res.sort();
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
