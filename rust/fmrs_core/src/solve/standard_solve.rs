use crate::memo::{Memo, MemoStub, MemoTrait};

use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{AdvanceOptions, BitBoard, Position, PositionExt};

use super::{reconstruct_solutions, Solution};
use log::info;

pub fn standard_solve(
    position: PositionAux,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Vec<Solution>> {
    let mut solver = StandardSolver::new(position, solutions_upto, silent);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(solutions) => return Ok(solutions),
            SolverStatus::NoSolution => return Ok(vec![]),
        }
    }
}

pub struct StandardSolver {
    initial_position: PositionAux,
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
    Mate(Vec<Solution>),
    NoSolution,
}

impl StandardSolver {
    pub fn new(position: PositionAux, solutions_upto: usize, silent: bool) -> Self {
        let initial_position = position.clone();

        let mut memo = Memo::default();
        memo.contains_or_insert(position.digest(), 0);

        let mut mate_positions = vec![];

        let stone = *position.stone();
        let mut positions = vec![position.core().clone()];

        let mut step = 0;

        if position.turn().is_black() {
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

        Self {
            initial_position,
            solutions_upto,
            step,
            positions,
            mate_positions,
            memo_white_turn: memo,
            stone,
            silent,
        }
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        if self.positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        next_next_positions(
            &mut self.mate_positions,
            &mut self.memo_white_turn,
            &mut self.positions,
            self.step,
            &self.stone,
        );

        if !self.mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in self.mate_positions.iter() {
                if self.solutions_upto > res.len() {
                    let mut sol = reconstruct_solutions(
                        self.initial_position.digest(),
                        mate_position,
                        &self.memo_white_turn,
                        self.solutions_upto - res.len(),
                    );
                    assert!(
                        !sol.is_empty(),
                        "{:?} {:?}",
                        self.initial_position,
                        mate_position
                    );
                    res.append(&mut sol);
                }
            }
            res.sort();

            if !self.silent {
                info!(
                    "Found {} solutions searching {} positions",
                    res.len(),
                    self.memo_white_turn.len(),
                );
            }

            return Ok(SolverStatus::Mate(res));
        }
        self.step += 2;
        Ok(SolverStatus::Intermediate(self.step as u32))
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
                mate_positions.push(position);
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
    mate_positions: &mut Vec<PositionAux>,
    memo_white_turn: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    let mut prev = vec![];
    std::mem::swap(&mut prev, positions);

    for core in prev {
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
            mate_positions.push(position);
        } else if !mate_positions.is_empty() {
            movements.clear();
        }

        for m in movements {
            let mut np = core.clone();
            np.do_move(&m);

            let mut position = PositionAux::new(np.clone(), *stone);

            let mut movements = vec![];
            advance_aux(
                &mut position,
                memo_white_turn,
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
