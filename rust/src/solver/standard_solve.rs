use fmrs_core::{
    position::position::PositionAux,
    solve::{Solution, SolverStatus, StandardSolver},
};

pub fn standard_solve(
    position: PositionAux,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Vec<Solution>> {
    let mut solver = StandardSolver::new(position, solutions_upto, silent);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(solutions) => return Ok(solutions),
            SolverStatus::NoSolution => return Ok(vec![]),
        }
    }
}
