use fmrs_core::{
    converter,
    position::position::PositionAux,
    sfen,
    solve::{parallel_solve::ParallelSolver, Solution, SolverStatus, StandardSolver},
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{log, utils::set_panic_hook};

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
    pub fn new(problem_sfen: String, solutions_upto: u16, algo: Algorithm) -> Self {
        set_panic_hook();

        let position = sfen::decode_position(&problem_sfen).unwrap();

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

        Self {
            initial_position: position,
            inner,
            no_solution: false,
            solutions: vec![],
        }
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
        converter::convert_to_kif(&mut self.initial_position, &self.solutions)
    }

    pub fn solutions_count(&self) -> u32 {
        self.solutions.len() as u32
    }
}

fn convert_solutions_to_sfen(solutions: &[Solution]) -> Vec<String> {
    let mut res = vec![];
    for solution in solutions {
        let mut moves = vec![];
        for movement in solution.0.iter() {
            moves.push(sfen::encode_move(movement))
        }
        res.push(moves.join(" "));
    }
    res
}

impl Drop for Solver {
    fn drop(&mut self) {
        log("Solver dropped");
    }
}
