//! Performance metrics and result types for benchmarking

use std::time::Duration;

/// Result of a benchmark run
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub engine_name: String,
    pub steps: usize,
    pub duration: Duration,
    pub total_cells: usize,
    pub live_cells: usize,
    pub cells_per_second: f64,
}

impl BenchmarkResult {
    /// Get the average time per step
    pub fn time_per_step(&self) -> Duration {
        self.duration / self.steps as u32
    }

    /// Get the throughput in millions of cells per second
    pub fn mcells_per_second(&self) -> f64 {
        self.cells_per_second / 1_000_000.0
    }

    /// Get the speedup relative to another result
    pub fn speedup_vs(&self, baseline: &BenchmarkResult) -> f64 {
        baseline.duration.as_secs_f64() / self.duration.as_secs_f64()
    }
}

/// Detailed performance metrics
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub memory_usage_bytes: usize,
    pub memory_per_cell_bytes: f64,
    pub cache_misses: Option<u64>,
    pub instructions_per_cell: Option<f64>,
}

impl PerformanceMetrics {
    /// Create basic metrics from memory usage
    pub fn from_memory(total_memory: usize, total_cells: usize) -> Self {
        Self {
            memory_usage_bytes: total_memory,
            memory_per_cell_bytes: total_memory as f64 / total_cells as f64,
            cache_misses: None,
            instructions_per_cell: None,
        }
    }
}

/// Comparison between two benchmark results
#[derive(Debug)]
pub struct BenchmarkComparison {
    pub baseline: BenchmarkResult,
    pub optimized: BenchmarkResult,
    pub speedup: f64,
    pub memory_improvement: f64,
}

impl BenchmarkComparison {
    pub fn new(baseline: BenchmarkResult, optimized: BenchmarkResult) -> Self {
        let speedup = optimized.speedup_vs(&baseline);
        let memory_improvement = baseline.total_cells as f64 / optimized.total_cells as f64;

        Self {
            baseline,
            optimized,
            speedup,
            memory_improvement,
        }
    }
}
