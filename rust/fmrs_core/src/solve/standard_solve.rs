use crate::position::position::PositionAux;

use super::low_mem_standard::{low_mem_standard_solve, low_mem_standard_solve_mult};
use super::reconstruct::Reconstructor;

pub type StandardSolver = super::low_mem_standard::LowMemStandardSolver;

pub fn standard_solve(
    position: PositionAux,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    low_mem_standard_solve(position, solutions_upto, silent)
}

pub fn standard_solve_mult(
    positions: Vec<PositionAux>,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    low_mem_standard_solve_mult(positions, solutions_upto, silent)
}

#[derive(PartialEq, Eq)]
pub enum SolverStatus {
    Intermediate(u32),
    Mate(Reconstructor),
    NoSolution,
}
