use fmrs_core::{
    position::Position,
    solve::{Solution, SolverStatus, StandardSolver},
};

pub fn standard_solve(position: Position, solutions_upto: usize) -> anyhow::Result<Vec<Solution>> {
    let mut solver = StandardSolver::new(position, solutions_upto);
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(solutions) => return Ok(solutions),
            SolverStatus::NoSolution => return Ok(vec![]),
        }
    }
}
