use std::sync::Mutex;

use rayon::prelude::*;

use crate::memo::{DashMemo, MemoTrait};
use crate::nohash::NoHashSet64;
use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{BitBoard, Position, PositionExt as _};

use super::reconstruct::Reconstructor;
use super::SolverStatus;

pub struct ParallelSolver {
    initial_position_digests: NoHashSet64,
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
            initial_position_digests: std::iter::once(position.digest()).collect(),
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

        if self.mate_positions.get_mut().is_ok_and(|mp| !mp.is_empty()) {
            let mate_positions = std::mem::take(&mut self.mate_positions)
                .into_inner()
                .unwrap();
            let memo_white_turn = std::mem::take(&mut self.memo_white_turn);

            let reconstructor = Reconstructor::new(
                std::mem::take(&mut self.initial_position_digests),
                mate_positions,
                Box::new(memo_white_turn),
                self.solutions_upto,
            );
            return Ok(SolverStatus::Mate(reconstructor));
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
            let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
            }

            movements.into_iter().filter_map(move |m| {
                let digest = position.moved_digest(&m);
                if memo_next.as_mut().contains_or_insert(digest, step + 1) {
                    return None;
                }

                let mut np = core.clone();
                np.do_move(&m);
                np.into()
            })
        })
        .collect()
}

fn next_next_positions(
    mate_positions: &Mutex<Vec<PositionAux>>,
    memo_white_turn: &DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|core| {
            let mut position = PositionAux::new(core.clone(), *stone);
            let mut movements = vec![];
            let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position);
            } else if !mate_positions.lock().unwrap().is_empty() {
                movements.clear();
            }

            movements.into_iter().flat_map(move |m| {
                let mut np = core.clone();
                np.do_move(&m);

                let mut movements = vec![];
                let mut nnp = PositionAux::new(np.clone(), *stone);
                advance_aux(&mut nnp, &Default::default(), &mut movements).unwrap();

                movements.into_iter().filter_map(move |m| {
                    let digest = nnp.moved_digest(&m);
                    if memo_white_turn
                        .as_mut()
                        .contains_or_insert(digest, step + 2)
                    {
                        return None;
                    }

                    let mut np = np.clone();
                    np.do_move(&m);
                    np.into()
                })
            })
        })
        .collect()
}
