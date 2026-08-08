use fmrs_core::{
    converter,
    piece::Color,
    position::position::PositionAux,
    sfen,
    solve::{
        low_mem_standard::LowMemStandardSolver, parallel_solve::ParallelSolver, Solution,
        SolverStatus,
    },
    validate::decode_and_validate_position,
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::utils::set_panic_hook;

pub trait SolverTrait {
    fn advance(&mut self) -> anyhow::Result<SolverStatus>;
}

impl SolverTrait for LowMemStandardSolver {
    fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        LowMemStandardSolver::advance(self)
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

        let position = decode_and_validate_position(&problem_sfen)?;

        let inner: Box<dyn SolverTrait> = match algo {
            Algorithm::Standard => {
                match LowMemStandardSolver::new(position.clone(), solutions_upto as usize, false) {
                    Ok(x) => Box::new(x),
                    Err(x) => return Err(x.to_string()),
                }
            }
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
            SolverStatus::Mate(reconstructor) => {
                self.solutions = reconstructor.solutions();
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
            for algorithm in [super::Algorithm::Standard, super::Algorithm::Parallel] {
                let mut solver = Solver::new(sfen.into(), 1, algorithm).unwrap();
                while !solver.solutions_found() && !solver.no_solution() {
                    solver.advance().unwrap();
                }
                assert!(solver.solutions_found(), "Expected solution for: {}", sfen);
                let kif = solver.solutions_kif();
                assert!(
                    kif.contains("後手番"),
                    "Expected 後手番 in KIF for white-first sfen {}: {}",
                    sfen,
                    kif
                );
            }
        }
    }

    #[test]
    fn test_invalid_initial_position_messages() {
        let got = match Solver::new(
            "4k4/4R4/9/9/9/9/9/4r4/4K4 b 2b4g4s4n4l18p 1".into(),
            1,
            super::Algorithm::Standard,
        ) {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert_eq!(got, "両方の玉に王手がかかっています。");

        let got = match Solver::new(
            "4k4/4P4/9/9/9/4P4/9/9/4K4 b 2r2b4g4s4n4l16p 1".into(),
            1,
            super::Algorithm::Standard,
        ) {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert_eq!(got, "初形が不正です: 二歩があります。");

        let got = match Solver::new(
            "P3k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l17p 1".into(),
            1,
            super::Algorithm::Standard,
        ) {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert_eq!(got, "初形が不正です: 行きどころのない駒があります。");
    }
}
