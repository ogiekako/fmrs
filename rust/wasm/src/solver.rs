use fmrs_core::{
    converter,
    piece::{Color, Kind},
    position::position::PositionAux,
    sfen,
    solve::{
        low_mem_standard::LowMemStandardSolver, parallel_solve::ParallelSolver, Solution,
        SolverStatus,
    },
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

fn decode_and_validate_position(problem_sfen: &str) -> Result<PositionAux, String> {
    let mut position = sfen::decode_position(problem_sfen)
        .map_err(|_| "局面の読み込みに失敗しました。".to_string())?;

    let black_checked = position.checked_slow(Color::BLACK);
    let white_checked = position.checked_slow(Color::WHITE);
    if black_checked && white_checked {
        return Err("両方の玉に王手がかかっています。".to_string());
    }
    if white_checked {
        position.set_turn(Color::WHITE);
    }

    let mut reasons = vec![];
    if has_double_pawns(&position) {
        reasons.push("二歩があります");
    }
    if has_unmovable_pieces(&position) {
        reasons.push("行きどころのない駒があります");
    }
    if !reasons.is_empty() {
        return Err(format!("初形が不正です: {}。", reasons.join("、")));
    }

    Ok(position)
}

fn has_double_pawns(position: &PositionAux) -> bool {
    for color in [Color::BLACK, Color::WHITE] {
        let pawns = position.bitboard(color, Kind::Pawn).u128();
        for col in 0..9 {
            if (pawns >> (col * 9) & 0x1FF).count_ones() > 1 {
                return true;
            }
        }
    }
    false
}

fn has_unmovable_pieces(position: &PositionAux) -> bool {
    for color in [Color::BLACK, Color::WHITE] {
        for kind in [Kind::Pawn, Kind::Lance, Kind::Knight] {
            for pos in position.bitboard(color, kind) {
                if is_unmovable_square(pos, color, kind) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_unmovable_square(pos: fmrs_core::position::Square, color: Color, kind: Kind) -> bool {
    match (color, kind) {
        (Color::BLACK, Kind::Pawn | Kind::Lance) => pos.row() == 0,
        (Color::WHITE, Kind::Pawn | Kind::Lance) => pos.row() == 8,
        (Color::BLACK, Kind::Knight) => pos.row() <= 1,
        (Color::WHITE, Kind::Knight) => pos.row() >= 7,
        _ => false,
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
                solver.solutions_kif();
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
