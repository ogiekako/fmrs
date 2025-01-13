use std::time::Instant;

use fmrs_core::position::position::PositionAux;

use fmrs_core::solve::parallel_solve::ParallelSolver;
use fmrs_core::solve::reconstruct::Reconstructor;
use fmrs_core::solve::SolverStatus;
use log::info;

pub(crate) fn parallel_solve(
    position: PositionAux,
    solutions_upto: usize,
    start: Option<Instant>,
) -> anyhow::Result<Reconstructor> {
    if position.is_illegal_initial_position() {
        anyhow::bail!("Illegal initial position");
    }
    let start = start.unwrap_or_else(Instant::now);
    let mut solver = ParallelSolver::new(position, solutions_upto);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(reconstructor) => {
                info!(
                    "Found mate in {} caching {} positions: {:.1?}",
                    reconstructor.mate_in().unwrap(),
                    reconstructor.cached_positions(),
                    start.elapsed()
                );
                return Ok(reconstructor);
            }
            SolverStatus::NoSolution => return Ok(Reconstructor::no_solution()),
        }
    }
}
