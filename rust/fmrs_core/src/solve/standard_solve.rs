use std::collections::HashSet;

use crate::memo::{Memo, MemoTrait};

use crate::nohash::NoHashSet64;
use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{BitBoard, Movement, Position, PositionExt};

use super::reconstruct::Reconstructor;
use anyhow::bail;
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
    initial_position_digests: NoHashSet64,
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    tmp_positions: Vec<Position>,
    movements: Vec<Movement>,
    tmp_movements: Vec<Movement>,
    mate_positions: Vec<PositionAux>,
    memo_black_turn: NoHashSet64,
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

        let mut positions = positions.iter().map(|p| p.core().clone()).collect();

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
            initial_position_digests,
            solutions_upto,
            step,
            positions,
            tmp_positions: vec![],
            movements: vec![],
            tmp_movements: vec![],
            mate_positions,
            memo_black_turn: Default::default(),
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
        self.tmp_positions.clear();
        std::mem::swap(&mut self.tmp_positions, &mut self.positions);

        for core in self.tmp_positions.iter() {
            let mut position = PositionAux::new(core.clone(), self.stone);

            self.movements.clear();
            let is_mate =
                advance_aux(&mut position, &Default::default(), &mut self.movements).unwrap();

            if is_mate {
                self.mate_positions.push(position.clone());
            } else if !self.mate_positions.is_empty() {
                continue;
            }

            std::mem::swap(&mut self.tmp_movements, &mut self.movements);
            for m in self.tmp_movements.iter() {
                let digest = position.moved_digest(m);
                if self.memo_black_turn.contains(&digest) {
                    continue;
                }
                self.memo_black_turn.insert(digest);

                let mut np = core.clone();
                np.do_move(m);

                let mut position = PositionAux::new(np.clone(), self.stone);

                self.movements.clear();
                advance_aux(&mut position, &Default::default(), &mut self.movements).unwrap();

                for m in self.movements.iter() {
                    let digest = position.moved_digest(m);
                    if self
                        .memo_white_turn
                        .contains_or_insert(digest, self.step + 2)
                    {
                        continue;
                    }

                    let mut np = np.clone();
                    np.do_move(m);
                    self.positions.push(np);
                }
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
    let mut movements = vec![];
    for core in std::mem::take(positions) {
        let mut position = PositionAux::new(core.clone(), *stone);
        movements.clear();
        let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

        if is_mate {
            mate_positions.push(position.clone());
        }

        for m in movements.iter() {
            let digest = position.moved_digest(m);
            if memo_next.contains_or_insert(digest, step + 1) {
                continue;
            }
            let mut np = core.clone();
            np.do_move(m);
            positions.push(np);
        }
    }
}
