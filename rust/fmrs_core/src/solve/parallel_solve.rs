use std::sync::Mutex;

use anyhow::anyhow;
use rayon::prelude::*;

use crate::memo::DashMemo;
use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{BitBoard, Position, PositionExt as _};

use super::{reconstruct_solutions, SolverStatus};

pub struct ParallelSolver {
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Mutex<Vec<PositionAux>>,
    memo: DashMemo,
    memo_next: DashMemo,
    stone: Option<BitBoard>,
}

impl ParallelSolver {
    pub fn new(position: PositionAux, solutions_upto: usize) -> Self {
        let mut memo = DashMemo::default();
        memo.insert(position.digest(), 0);
        let mut memo_next = DashMemo::default();

        let mate_positions: Mutex<Vec<PositionAux>> = Mutex::new(vec![]);

        let stone = *position.stone();
        let mut positions = vec![position.core().clone()];

        next_positions(&mate_positions, &memo_next, &mut positions, 0, &stone);
        std::mem::swap(&mut memo, &mut memo_next);

        Self {
            solutions_upto,
            step: 1,
            positions,
            mate_positions,
            memo,
            memo_next,
            stone,
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
            &self.stone,
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
                    &self.memo_next.as_mut(),
                    &self.memo.as_mut(),
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
    mate_positions: &Mutex<Vec<PositionAux>>,
    memo_next: &DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|core| {
            let mut movements = vec![];
            let mut position = PositionAux::new(core.clone());
            if let Some(stone) = stone {
                position.set_stone(*stone);
            }
            let is_mate = advance_aux(
                &mut position,
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position);
            }

            movements.into_iter().map(move |m| {
                let mut np = core.clone();
                np.do_move(&m);
                np
            })
        })
        .collect()
}

fn next_next_positions(
    mate_positions: &Mutex<Vec<PositionAux>>,
    memo: &mut DashMemo,
    memo_next: &mut DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|core| {
            let mut position = PositionAux::new(core.clone());
            if let Some(stone) = stone {
                position.set_stone(*stone);
            }
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut position,
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position);
            } else if !mate_positions.lock().unwrap().is_empty() {
                movements.clear();
            }

            movements.into_iter().flat_map(|m| {
                let mut np = core.clone();
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
