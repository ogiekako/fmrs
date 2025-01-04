pub mod parallel_solve;
pub mod reconstruct;
mod standard_solve;
use crate::position::position::PositionAux;
use crate::position::Movement;

pub type Solution = (Vec<Movement>, /* last position */ PositionAux);
pub use reconstruct::reconstruct_solutions;
pub use standard_solve::SolverStatus;
pub use standard_solve::StandardSolver;
