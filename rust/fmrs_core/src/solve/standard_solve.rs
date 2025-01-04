use crate::memo::{Memo, MemoTrait};

use crate::position::advance::advance::advance_aux;
use crate::position::position::PositionAux;
use crate::position::{BitBoard, Position, PositionExt};

use super::{reconstruct_solutions, Solution};
use log::info;

pub struct StandardSolver {
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    mate_positions: Vec<PositionAux>,
    memo: Memo,
    memo_next: Memo,
    stone: Option<BitBoard>,
}

#[derive(PartialEq, Eq)]
pub enum SolverStatus {
    Intermediate(u32),
    Mate(Vec<Solution>),
    NoSolution,
}

impl StandardSolver {
    pub fn new(position: PositionAux, solutions_upto: usize) -> Self {
        let mut memo = Memo::default();
        memo.contains_or_insert(position.digest(), 0);
        let mut memo_next = Memo::default();

        let mut mate_positions = vec![];

        let stone = *position.stone();
        let mut positions = vec![position.core().clone()];

        next_positions(
            &mut mate_positions,
            &mut memo_next,
            &mut positions,
            0,
            &stone,
        );
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
            &mut self.mate_positions,
            &mut self.memo,
            &mut self.memo_next,
            &mut self.positions,
            self.step,
            &self.stone,
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
    mate_positions: &mut Vec<PositionAux>,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    *positions = positions
        .iter()
        .flat_map(|core| {
            let mut position = PositionAux::new(core.clone());
            if let Some(stone) = stone {
                position.set_stone(*stone);
            }
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
    memo: &mut Memo,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    let mut prev = vec![];
    std::mem::swap(&mut prev, positions);

    for core in prev {
        let mut position = PositionAux::new(core.clone());
        if let Some(stone) = stone {
            position.set_stone(*stone);
        }
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
        } else if !mate_positions.is_empty() {
            movements.clear();
        }

        for m in movements {
            let mut np = core.clone();
            np.do_move(&m);

            let mut position = PositionAux::new(np.clone());
            if let Some(stone) = stone {
                position.set_stone(*stone);
            }

            let mut movements = vec![];
            advance_aux(
                &mut position,
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
