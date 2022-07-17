use fmrs_core::{
    converter, jkf,
    position::Position,
    sfen,
    solve::{Solution, SolverStatus, StandardSolver},
};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub struct Solver {
    initial_position: Position,
    inner: StandardSolver,
    no_solution: bool,
    solutions: Vec<Solution>,
}

#[wasm_bindgen]
impl Solver {
    pub fn new(problem_sfen: String, solutions_upto: u32) -> Self {
        let position = sfen::decode_position(&problem_sfen).unwrap();
        Self {
            initial_position: position.clone(),
            inner: StandardSolver::new(position, solutions_upto as usize),
            no_solution: false,
            solutions: vec![],
        }
    }

    /// Returns non-empty string in case of an error.
    pub fn advance(&mut self) -> String {
        if self.no_solution || !self.solutions.is_empty() {
            return "already finished".to_string();
        }
        let status = match self.inner.advance() {
            Ok(x) => x,
            Err(x) => return x.to_string(),
        };
        match status {
            SolverStatus::Intermediate => (),
            SolverStatus::Mate(solutions) => {
                self.solutions = solutions;
            }
            SolverStatus::NoSolution => self.no_solution = true,
        }
        "".to_string()
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
    pub fn solutions_json(&self) -> String {
        let jkf = converter::convert(&self.initial_position, &self.solutions);
        serde_json::to_string(&jkf).unwrap()
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
