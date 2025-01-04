use std::time::Instant;

use fmrs_core::position::position::PositionAux;

use fmrs_core::solve::parallel_solve::ParallelSolver;
use fmrs_core::solve::{Solution, SolverStatus};

pub(super) fn parallel_solve(
    position: PositionAux,
    _progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
    start: Option<Instant>,
) -> anyhow::Result<Vec<Solution>> {
    let mut solver = ParallelSolver::new(position, solutions_upto);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(solutions) => {
                if let Some(start) = start {
                    eprintln!(
                        "found mate in {}: {:.1?}",
                        solutions[0].len(),
                        start.elapsed()
                    );
                }
                return Ok(solutions);
            }
            SolverStatus::NoSolution => return Ok(vec![]),
        }
    }
}
