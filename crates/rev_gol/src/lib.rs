//! Reverse Game of Life SAT Solver
//!
//! This library provides functionality to find predecessor states for Conway's Game of Life
//! using SAT solving techniques.

pub mod config;
pub mod game_of_life;
pub mod reverse;
pub mod sat;
pub mod utils;

pub use config::Settings;
pub use reverse::{ReverseProblem, Solution};

use anyhow::Result;

/// Main entry point for solving reverse Game of Life problems
pub fn solve_reverse(settings: Settings) -> Result<Vec<Solution>> {
    let mut problem = ReverseProblem::new(settings)?;
    problem.solve()
}
