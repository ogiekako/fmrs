use std::collections::HashSet;
use std::sync::Mutex;

use anyhow::bail;
use rayon::prelude::*;

use crate::memo::DashMemo;
use crate::nohash::NoHashSet64;
use crate::position::advance::advance::advance_aux;
use crate::position::position::{CachedPosition, PositionAux};
use crate::position::BitBoard;

use super::reconstruct::Reconstructor;
use super::SolverStatus;

pub struct ParallelSolver {
    initial_position_digests: NoHashSet64,
    solutions_upto: usize,
    step: u16,
    positions: Vec<CachedPosition>,
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
        let mut positions: Vec<CachedPosition> = vec![CachedPosition::from_aux(&position)];

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

    /// 複数の根から同時に探索する (LowMemStandardSolver::with_multiple の並列版)。
    /// 全根が同一手番・同一 stone であること。
    pub fn with_multiple(
        positions: Vec<PositionAux>,
        solutions_upto: usize,
    ) -> anyhow::Result<Self> {
        if positions.is_empty() {
            bail!("No initial positions");
        }
        if positions.iter().any(|p| p.is_illegal_initial_position()) {
            bail!("Illegal initial position");
        }

        let turns = positions.iter().map(|p| p.turn()).collect::<HashSet<_>>();
        if turns.len() > 1 {
            bail!("Multiple turns");
        }
        let turn = turns.iter().next().copied().unwrap();

        let stones = positions.iter().map(|p| p.stone()).collect::<HashSet<_>>();
        if stones.len() > 1 {
            bail!("Multiple stone formations");
        }
        let stone = stones.iter().next().and_then(|s| **s);

        let initial_position_digests: NoHashSet64 = positions.iter().map(|p| p.digest()).collect();

        let mut memo = DashMemo::default();
        for digest in initial_position_digests.iter() {
            memo.insert(*digest, 0);
        }

        let mate_positions: Mutex<Vec<PositionAux>> = Mutex::new(vec![]);
        let mut positions: Vec<CachedPosition> =
            positions.iter().map(CachedPosition::from_aux).collect();
        let mut step = 0;

        if turn.is_black() {
            let memo_next = DashMemo::default();
            next_positions(&mate_positions, &memo_next, &mut positions, step, &stone);
            memo = memo_next;
            step += 1;
        }

        Ok(Self {
            initial_position_digests,
            solutions_upto,
            step,
            positions,
            mate_positions,
            memo_white_turn: memo,
            stone,
        })
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
            &self.memo_white_turn,
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
    positions: &mut Vec<CachedPosition>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .par_iter()
        .flat_map_iter(|cp| {
            let mut movements = vec![];
            let mut position = cp.to_aux(*stone);
            let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
            }

            let cp = cp.clone();
            movements.into_iter().filter_map(move |m| {
                let digest = position.moved_digest(&m);
                if memo_next.par_contains_or_insert(digest, step + 1) {
                    return None;
                }
                Some(cp.after_movement(&m))
            })
        })
        .collect()
}

fn next_next_positions(
    mate_positions: &Mutex<Vec<PositionAux>>,
    memo_white_turn: &DashMemo,
    positions: &mut Vec<CachedPosition>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .par_iter()
        .flat_map_iter(|cp| {
            let mut position = cp.to_aux(*stone);
            let mut movements = vec![];
            let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position);
            } else if !mate_positions.lock().unwrap().is_empty() {
                movements.clear();
            }

            let cp = cp.clone();
            movements.into_iter().flat_map(move |m| {
                let outer_next = cp.after_movement(&m);
                let mut movements = vec![];
                let mut nnp = outer_next.to_aux(*stone);
                advance_aux(&mut nnp, &Default::default(), &mut movements).unwrap();

                movements.into_iter().filter_map(move |m| {
                    let digest = nnp.moved_digest(&m);
                    if memo_white_turn.par_contains_or_insert(digest, step + 2) {
                        return None;
                    }
                    Some(outer_next.after_movement(&m))
                })
            })
        })
        .collect()
}
