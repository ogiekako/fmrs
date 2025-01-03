use fmrs_core::{
    converter,
    position::Position,
    sfen,
    solve::{Solution, SolverStatus, StandardSolver},
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{response::JsonResponse, utils::set_panic_hook};

pub trait SolverTrait {
    fn advance(&mut self) -> anyhow::Result<SolverStatus>;
}

impl SolverTrait for StandardSolver {
    fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        StandardSolver::advance(self)
    }
}

#[wasm_bindgen]
pub struct Solver {
    initial_position: Position,
    inner: Box<dyn SolverTrait>,
    no_solution: bool,
    solutions: Vec<Solution>,
}

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
        Self {
            initial_position: position.clone(),
            inner: Box::new(StandardSolver::new(position, solutions_upto as usize)),
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
            SolverStatus::Intermediate(delta) => return Ok(delta),
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

    // jkf format
    pub fn solutions_json(&self) -> JsonResponse {
        let kif = converter::convert_to_kif(&self.initial_position, &self.solutions);
        JsonResponse {
            solutions: self.solutions.len() as u16,
            kif,
        }
    }
}

fn convert_solutions_to_sfen(solutions: &[Solution]) -> Vec<String> {
    let mut res = vec![];
    for solution in solutions {
        let mut moves = vec![];
        for movement in solution {
            moves.push(sfen::encode_move(movement))
        }
        res.push(moves.join(" "));
    }
    res
}
