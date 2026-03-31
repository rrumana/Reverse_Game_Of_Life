//! Benchmarking framework for Game of Life engines

pub mod metrics;
pub mod suite;

pub use metrics::{BenchmarkResult, PerformanceMetrics};
pub use suite::BenchmarkSuite;

use crate::engines::GameOfLifeEngine;
use std::time::Instant;

/// Run a simple benchmark on an engine
pub fn benchmark_engine(engine: &mut dyn GameOfLifeEngine, steps: usize) -> BenchmarkResult {
    let start = Instant::now();
    engine.run_steps(steps);
    let duration = start.elapsed();

    let grid = engine.get_grid();
    let total_cells = grid.total_cells();
    let live_cells = grid.count_live_cells();

    BenchmarkResult {
        engine_name: engine.benchmark_info().name,
        steps,
        duration,
        total_cells,
        live_cells,
        cells_per_second: (total_cells as f64 * steps as f64) / duration.as_secs_f64(),
    }
}
