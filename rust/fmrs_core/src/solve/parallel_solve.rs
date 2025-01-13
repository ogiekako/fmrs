use std::sync::Mutex;

use anyhow::anyhow;
use log::info;
use rayon::prelude::*;

use crate::memo::{DashMemo, MemoStub};
use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{AdvanceOptions, BitBoard, Position, PositionExt as _};

use super::{reconstruct_solutions, SolverStatus};

pub struct ParallelSolver {
    initial_position_digest: u64,
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Mutex<Vec<PositionAux>>,
    memo_white_turn: DashMemo,
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

        let mut step = 0;

        if position.turn().is_black() {
            next_positions(&mate_positions, &memo_next, &mut positions, step, &stone);
            std::mem::swap(&mut memo, &mut memo_next);
            step += 1;
        }

        Self {
            initial_position_digest: position.digest(),
            solutions_upto,
            step,
            positions,
            mate_positions,
            memo_white_turn: memo,
            stone,
        }
    }

    pub fn cached_positions(&self) -> usize {
        self.memo_white_turn.len()
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        if self.positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        next_next_positions(
            &self.mate_positions,
            &mut self.memo_white_turn,
            &mut self.positions,
            self.step,
            &self.stone,
        );

        let mate_positions = self
            .mate_positions
            .get_mut()
            .map_err(|e| anyhow!(e.to_string()))?;
        if !mate_positions.is_empty() {
            info!("Found mate in {}; reconstructing solutions", self.step);
            let mut res = vec![];
            for mate_position in mate_positions {
                res.append(&mut reconstruct_solutions(
                    self.initial_position_digest,
                    mate_position,
                    &self.memo_white_turn.as_mut(),
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
            let mut position = PositionAux::new(core.clone(), *stone);
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
    memo_white_turn: &mut DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|core| {
            let mut position = PositionAux::new(core.clone(), *stone);
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut position,
                &mut MemoStub,
                step + 1,
                &AdvanceOptions {
                    no_memo: true,
                    ..Default::default()
                },
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
                let mut nnp = PositionAux::new(np.clone(), *stone);
                advance_aux(
                    &mut nnp,
                    &mut memo_white_turn.as_mut(),
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
