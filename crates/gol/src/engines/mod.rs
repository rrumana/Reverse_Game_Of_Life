pub mod naive;
pub mod ultimate;

pub use naive::NaiveEngine;
pub use ultimate::{
    auto_from_grid_ultimate_engine, auto_new_ultimate_engine, create_optimal_engine,
    safe_auto_new_ultimate_engine, UltimateEngine,
};

use crate::grid::Grid;
use std::time::Duration;

/// Information about a Game of Life engine's performance characteristics
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub name: String,
    pub description: String,
    pub memory_per_cell_bits: f64,
    pub supports_parallel: bool,
    pub supports_simd: bool,
    pub min_grid_size: Option<(usize, usize)>,
    pub max_grid_size: Option<(usize, usize)>,
}

/// Trait for Game of Life engine implementations
pub trait GameOfLifeEngine {
    /// Advance the simulation by one step
    fn step(&mut self);

    /// Get a reference to the current grid
    fn get_grid(&self) -> &dyn Grid;

    /// Set the grid to a new state
    fn set_grid(&mut self, grid: &dyn Grid);

    /// Get information about this engine
    fn benchmark_info(&self) -> EngineInfo;

    /// Get the value of a specific cell
    fn get_cell(&self, row: usize, col: usize) -> bool;

    /// Get the width of the grid
    fn width(&self) -> usize {
        self.get_grid().width()
    }

    /// Get the height of the grid
    fn height(&self) -> usize {
        self.get_grid().height()
    }

    /// Count the number of live cells
    fn count_live_cells(&self) -> usize {
        let grid = self.get_grid();
        let mut count = 0;
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                if grid.get_cell(y, x) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Run multiple steps efficiently
    fn run_steps(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step();
        }
    }

    /// Run steps with timing information
    fn run_steps_timed(&mut self, steps: usize) -> Duration {
        let start = std::time::Instant::now();
        self.run_steps(steps);
        start.elapsed()
    }
}
