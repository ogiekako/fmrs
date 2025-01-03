use std::sync::Mutex;

use anyhow::anyhow;
use rayon::prelude::*;

use crate::memo::DashMemo;
use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{Position, PositionExt as _};

use super::{reconstruct_solutions, SolverStatus};

pub struct ParallelSolver {
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Mutex<Vec<Position>>,
    memo: DashMemo,
    memo_next: DashMemo,
}

impl ParallelSolver {
    pub fn new(position: Position, solutions_upto: usize) -> Self {
        let mut memo = DashMemo::default();
        memo.insert(position.digest(), 0);
        let mut memo_next = DashMemo::default();

        let mate_positions: Mutex<Vec<Position>> = Mutex::new(vec![]);

        let mut positions = vec![position];

        next_positions(&mate_positions, &memo_next, &mut positions, 0);
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
            &self.mate_positions,
            &mut self.memo,
            &mut self.memo_next,
            &mut self.positions,
            self.step,
        );

        let mate_positions = self
            .mate_positions
            .get_mut()
            .map_err(|e| anyhow!(e.to_string()))?;
        if !mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in mate_positions {
                res.append(&mut reconstruct_solutions(
                    mate_position,
                    &mut self.memo_next.as_mut(),
                    &mut self.memo.as_mut(),
                    self.solutions_upto - res.len(),
                ));
            }
            res.sort();
            return Ok(SolverStatus::Mate(res));
        }
        self.step += 2;
        Ok(SolverStatus::Intermediate(self.step as u32))
    }
}

fn next_positions(
    mate_positions: &Mutex<Vec<Position>>,
    memo_next: &DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|position| {
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut PositionAux::new(position.clone()),
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
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
    mate_positions: &Mutex<Vec<Position>>,
    memo: &mut DashMemo,
    memo_next: &mut DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|position| {
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut PositionAux::new(position.clone()),
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
            } else if !mate_positions.lock().unwrap().is_empty() {
                movements.clear();
            }

            movements.into_iter().flat_map(|m| {
                let mut np = position.clone();
                np.do_move(&m);

                let mut movements = vec![];
                advance_aux(
                    &mut PositionAux::new(np.clone()),
                    &mut memo.as_mut(),
                    step + 2,
                    &Default::default(),
                    &mut movements,
                )
                .unwrap();

                movements.into_iter().map(move |m| {
                    let mut np = np.clone();
                    np.do_move(&m);
                    np
                })
            })
        })
        .collect()
}
