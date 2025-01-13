use std::time::Instant;

use fmrs_core::position::position::PositionAux;

use fmrs_core::solve::parallel_solve::ParallelSolver;
use fmrs_core::solve::{Solution, SolverStatus};
use log::info;

pub(crate) fn parallel_solve(
    position: PositionAux,
    solutions_upto: usize,
    start: Option<Instant>,
) -> anyhow::Result<Vec<Solution>> {
    if position.is_illegal_initial_position() {
        anyhow::bail!("Illegal initial position");
    }
    let start = start.unwrap_or_else(|| Instant::now());
    let mut solver = ParallelSolver::new(position, solutions_upto);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(solutions) => {
                info!(
                    "Found mate in {} caching {} positions: {:.1?}",
                    solutions[0].len(),
                    solver.cached_positions(),
                    start.elapsed()
                );
                return Ok(solutions);
            }
            SolverStatus::NoSolution => return Ok(vec![]),
        }
    }
}
