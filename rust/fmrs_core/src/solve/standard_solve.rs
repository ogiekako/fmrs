use std::collections::HashSet;

use crate::memo::{Memo, MemoStub, MemoTrait};

use crate::nohash::NoHashSet64;
use crate::position::advance::advance::{advance, advance_aux};
use crate::position::controller::PositionController;
use crate::position::position::PositionAux;
use crate::position::{AdvanceOptions, BitBoard, Position, PositionExt};

use super::reconstruct::Reconstructor;
use anyhow::{anyhow, bail};
use log::info;

pub fn standard_solve(
    position: PositionAux,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    standard_solve_mult(vec![position], solutions_upto, silent)
}

pub fn standard_solve_mult(
    positions: Vec<PositionAux>,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    let mut solver = StandardSolver::with_multiple(positions, solutions_upto, silent)?;
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(reconstructor) => return Ok(reconstructor),
            SolverStatus::NoSolution => return Ok(Reconstructor::no_solution()),
        }
    }
}

pub struct StandardSolver {
    controller: PositionController,

    initial_position_digests: NoHashSet64,
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Vec<PositionAux>,
    memo_white_turn: Memo,
    stone: Option<BitBoard>,
    silent: bool,
}

#[derive(PartialEq, Eq)]
pub enum SolverStatus {
    Intermediate(u32),
    Mate(Reconstructor),
    NoSolution,
}

impl StandardSolver {
    pub fn new(position: PositionAux, solutions_upto: usize, silent: bool) -> anyhow::Result<Self> {
        Self::with_multiple(vec![position], solutions_upto, silent)
    }

    pub fn with_multiple(
        positions: Vec<PositionAux>,
        solutions_upto: usize,
        silent: bool,
    ) -> anyhow::Result<Self> {
        if positions.is_empty() {
            bail!("No initial positions");
        }

        if positions.iter().any(|p| p.is_illegal_initial_position()) {
            bail!("Illegal initial position");
        }

        let initial_position_digests: NoHashSet64 = positions.iter().map(|p| p.digest()).collect();

        let mut memo = Memo::default();
        for digest in initial_position_digests.iter() {
            memo.contains_or_insert(*digest, 0);
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

        let mut mate_positions = vec![];

        let mut positions: Vec<Position> = positions.iter().map(|p| p.core().clone()).collect();

        let controller = PositionController::new(positions[0].clone(), stone);

        let mut step = 0;

        if turn.is_black() {
            let mut memo_next = Memo::default();
            next_positions(
                &mut mate_positions,
                &mut memo_next,
                &mut positions,
                step,
                &stone,
            );
            std::mem::swap(&mut memo, &mut memo_next);
            step += 1;
        }

        Ok(Self {
            controller,

            initial_position_digests,
            solutions_upto,
            step,
            positions,
            mate_positions,
            memo_white_turn: memo,
            stone,
            silent,
        })
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        if self.positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        self.next_next_positions();

        if !self.mate_positions.is_empty() {
            if !self.silent {
                info!(
                    "Found {} mates searching {} positions",
                    self.mate_positions.len(),
                    self.memo_white_turn.len(),
                );
            }

            let mate_positions = std::mem::take(&mut self.mate_positions);
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

    fn next_next_positions(&mut self) {
        let mut prev = vec![];
        std::mem::swap(&mut prev, &mut self.positions);

        let mut movements = vec![];
        let mut movements2 = vec![];

        for core in prev {
            self.controller.set_core(&core);

            movements.clear();
            let is_mate = advance(
                &mut self.controller,
                &mut MemoStub,
                self.step + 1,
                &AdvanceOptions {
                    no_memo: true,
                    ..Default::default()
                },
                &mut movements,
            )
            .map_err(|e| anyhow!("advance failed: {} {:?}", e, self.controller))
            .unwrap();

            if is_mate {
                self.mate_positions
                    .push(PositionAux::new(core.clone(), self.stone));
            } else if !self.mate_positions.is_empty() {
                movements.clear();
            }

            for m in movements.iter() {
                self.controller.push();

                self.controller.do_move(m);

                movements2.clear();
                advance(
                    &mut self.controller,
                    &mut self.memo_white_turn,
                    self.step + 2,
                    &Default::default(),
                    &mut movements2,
                )
                .unwrap();

                for m in movements2.iter() {
                    let mut np = self.controller.core().clone();
                    np.do_move(m);
                    self.positions.push(np);
                }

                self.controller.pop();
            }
        }
    }
}

fn next_positions(
    mate_positions: &mut Vec<PositionAux>,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .iter()
        .flat_map(|core| {
            let mut position = PositionAux::new(core.clone(), *stone);

            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut position,
                memo_next,
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.push(PositionAux::new(core.clone(), *stone));
            }

            movements.into_iter().map(move |m| {
                let mut np = core.clone();
                np.do_move(&m);
                np
            })
        })
        .collect()
}
