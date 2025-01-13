pub mod parallel_solve;
pub mod reconstruct;
pub mod standard_solve;
use crate::position::Movement;

pub type Solution = Vec<Movement>;
pub use standard_solve::SolverStatus;
pub use standard_solve::StandardSolver;
