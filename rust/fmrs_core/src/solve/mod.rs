pub mod reconstruct;
mod standard_solve;
pub mod parallel_solve;
use crate::position::Movement;

pub type Solution = Vec<Movement>;
pub use reconstruct::reconstruct_solutions;
pub use standard_solve::SolverStatus;
pub use standard_solve::StandardSolver;
