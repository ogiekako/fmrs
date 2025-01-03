use crate::memo::{Memo, MemoTrait};

use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{Position, PositionExt};

use super::{reconstruct_solutions, Solution};
use log::info;

pub struct StandardSolver {
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Vec<Position>,
    memo: Memo,
    memo_next: Memo,
}

#[derive(PartialEq, Eq)]
pub enum SolverStatus {
    Intermediate(u32),
    Mate(Vec<Solution>),
    NoSolution,
}

impl StandardSolver {
    pub fn new(position: Position, solutions_upto: usize) -> Self {
        let mut memo = Memo::default();
        memo.contains_or_insert(position.digest(), 0);
        let mut memo_next = Memo::default();

        let mut mate_positions = vec![];

        let mut positions = vec![position];

        next_positions(&mut mate_positions, &mut memo_next, &mut positions, 0);
        std::mem::swap(&mut memo, &mut memo_next);

        Self {
            solutions_upto,
            step: 1,
            positions,
            mate_positions,
            memo,
            memo_next,
        }
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        if self.positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        next_next_positions(
            &mut self.mate_positions,
            &mut self.memo,
            &mut self.memo_next,
            &mut self.positions,
            self.step,
        );

        if !self.mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in self.mate_positions.iter() {
                res.append(&mut reconstruct_solutions(
                    mate_position,
                    &self.memo_next,
                    &self.memo,
                    self.solutions_upto - res.len(),
                ));
            }
            res.sort();

            info!(
                "Found {} solutions searching {} positions",
                res.len(),
                self.memo.len() + self.memo_next.len(),
            );

            return Ok(SolverStatus::Mate(res));
        }
        self.step += 2;
        Ok(SolverStatus::Intermediate(self.step as u32))
    }
}

fn next_positions(
    mate_positions: &mut Vec<Position>,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    *positions = positions
        .iter()
        .flat_map(|position| {
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut PositionAux::new(position.clone()),
                memo_next,
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.push(position.clone());
            }

            movements.into_iter().map(move |m| {
                let mut np = position.clone();
                np.do_move(&m);
                np
            })
        })
        .collect()
}

fn next_next_positions(
    mate_positions: &mut Vec<Position>,
    memo: &mut Memo,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    let mut prev = vec![];
    std::mem::swap(&mut prev, positions);

    for position in prev {
        let mut movements = vec![];
        let is_mate = advance_aux(
            &mut PositionAux::new(position.clone()),
            memo_next,
            step + 1,
            &Default::default(),
            &mut movements,
        )
        .unwrap();

        if is_mate {
            mate_positions.push(position.clone());
        } else if !mate_positions.is_empty() {
            movements.clear();
        }

        for m in movements {
            let mut np = position.clone();
            np.do_move(&m);

            let mut movements = vec![];
            advance_aux(
                &mut PositionAux::new(np.clone()),
                memo,
                step + 2,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            for m in movements {
                let mut np = np.clone();
                np.do_move(&m);
                positions.push(np);
            }
        }
    }
}
