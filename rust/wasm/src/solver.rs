use fmrs_core::{
    converter,
    piece::Color,
    position::position::PositionAux,
    sfen,
    solve::{parallel_solve::ParallelSolver, Solution, SolverStatus, StandardSolver},
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::utils::set_panic_hook;

pub trait SolverTrait {
    fn advance(&mut self) -> anyhow::Result<SolverStatus>;
}

impl SolverTrait for StandardSolver {
    fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        StandardSolver::advance(self)
    }
}

impl SolverTrait for ParallelSolver {
    fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        ParallelSolver::advance(self)
    }
}

#[wasm_bindgen]
pub struct Solver {
    initial_position: PositionAux,
    inner: Box<dyn SolverTrait>,
    no_solution: bool,
    solutions: Vec<Solution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[wasm_bindgen]
pub enum Algorithm {
    Standard,
    Parallel,
}

#[wasm_bindgen]
impl Solver {
    #[wasm_bindgen(constructor)]
    pub fn new(problem_sfen: String, solutions_upto: u16, algo: Algorithm) -> Result<Self, String> {
        set_panic_hook();

        let mut position = sfen::decode_position(&problem_sfen).unwrap();
        if position.checked_slow(Color::WHITE) {
            position.set_turn(Color::WHITE);
        }
        if position.checked_slow(position.turn().opposite()) {
            return Err("both checked".to_string());
        }

        let inner: Box<dyn SolverTrait> = match algo {
            Algorithm::Standard => Box::new(StandardSolver::new(
                position.clone(),
                solutions_upto as usize,
                true,
            )),
            Algorithm::Parallel => Box::new(ParallelSolver::new(
                position.clone(),
                solutions_upto as usize,
            )),
        };

        Ok(Self {
            initial_position: position,
            inner,
            no_solution: false,
            solutions: vec![],
        })
    }

    /// Returns non-empty string in case of an error.
    pub fn advance(&mut self) -> Result<u32, String> {
        if self.no_solution || !self.solutions.is_empty() {
            return Err("already finished".to_string());
        }
        let status = match self.inner.advance() {
            Ok(x) => x,
            Err(x) => return Err(x.to_string()),
        };
        match status {
            SolverStatus::Intermediate(step) => return Ok(step),
            SolverStatus::Mate(solutions) => {
                self.solutions = solutions;
            }
            SolverStatus::NoSolution => self.no_solution = true,
        }
        Ok(0)
    }

    pub fn no_solution(&self) -> bool {
        self.no_solution
    }

    pub fn solutions_found(&self) -> bool {
        !self.solutions.is_empty()
    }

    /// Newline-delimited sfen moves
    pub fn solutions_sfen(&self) -> String {
        let solutions_sfen = convert_solutions_to_sfen(&self.solutions);
        solutions_sfen.join("\n")
    }

    pub fn solutions_kif(&mut self) -> String {
        if self.initial_position.turn() == Color::WHITE {
            let mut ini = self.initial_position.flipped();
            let mut sol = self.solutions.clone();
            sol.iter_mut()
                .for_each(|x| x.iter_mut().for_each(|m| *m = m.flipped()));
            return converter::convert_to_kif(&mut ini, &sol);
        }

        converter::convert_to_kif(&mut self.initial_position, &self.solutions)
    }

    pub fn solutions_count(&self) -> u32 {
        self.solutions.len() as u32
    }

    pub fn redundant(&self) -> bool {
        if self.solutions.is_empty() {
            return false;
        }
        let mut np = self.initial_position.clone();
        for m in self.solutions[0].iter() {
            np.do_move(m);
        }
        !np.hands().is_empty(Color::BLACK)
    }

    pub fn is_from_white(&self) -> bool {
        self.initial_position.turn() == Color::WHITE
    }
}

fn convert_solutions_to_sfen(solutions: &[Solution]) -> Vec<String> {
    let mut res = vec![];
    for solution in solutions {
        let mut moves = vec![];
        for movement in solution.iter() {
            moves.push(sfen::encode_move(movement))
        }
        res.push(moves.join(" "));
    }
    res
}

#[cfg(test)]
mod tests {
    use super::Solver;

    #[test]
    fn test_solutions_to_kif() {
        for sfen in ["2k6/9/1R1l5/9/9/3+l5/9/9/2L1K4 b 4g3s3n 1"] {
            let mut solver = Solver::new(sfen.into(), 1, super::Algorithm::Standard).unwrap();
            while !solver.solutions_found() && !solver.no_solution() {
                solver.advance().unwrap();
            }
            solver.solutions_kif();
        }
    }
}
