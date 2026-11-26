//! Rust port of `shtsume` (https://github.com/hkijin/shtsume) with pluggable position backends.
//!
//! Original C implementation: Copyright (c) 2022-2025 棋人(Hkijin), MIT License.
//! See `src/shtsume/LICENSE` for the full license text.

pub mod backend;
pub mod cycle_skip;
pub mod default_weights;
pub mod fmrs_backend;
pub mod ml;
pub mod ni_cache;
pub mod parallel;
pub mod proof;
pub mod shared_tbase;
pub mod shtsume_backend;
pub mod solver;
pub mod tbase;
pub mod time;
pub mod types;

pub use backend::{HsOracle, PositionBackend};
pub use fmrs_backend::{move_to_movement, pv_to_movements, FmrsPos};
pub use shtsume_backend::ShtsumePos;
pub use solver::{DefaultHeuristic, SearchHeuristic, Solver};
pub use types::{Move, SearchConfig, SearchResult};
