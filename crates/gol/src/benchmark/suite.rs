//! Benchmark suite for comparing Game of Life engines

use super::metrics::{BenchmarkComparison, BenchmarkResult};
use crate::engines::GameOfLifeEngine;
use crate::grid::{Grid, StandardGrid};
use std::time::Instant;

/// A comprehensive benchmark suite for Game of Life engines
pub struct BenchmarkSuite {
    test_patterns: Vec<TestPattern>,
    grid_sizes: Vec<(usize, usize)>,
    step_counts: Vec<usize>,
}

/// A test pattern for benchmarking
#[derive(Debug, Clone)]
pub struct TestPattern {
    pub name: String,
    pub description: String,
    pub pattern: Vec<String>,
}

impl BenchmarkSuite {
    /// Create a new benchmark suite with default test cases
    pub fn new() -> Self {
        Self {
            test_patterns: Self::default_patterns(),
            grid_sizes: vec![(50, 50), (100, 100), (200, 200), (500, 500)],
            step_counts: vec![10, 50, 100, 500],
        }
    }

    /// Create a minimal benchmark suite for quick testing
    pub fn minimal() -> Self {
        Self {
            test_patterns: vec![Self::blinker_pattern()],
            grid_sizes: vec![(10, 10), (50, 50)],
            step_counts: vec![10, 100],
        }
    }

    /// Run a comprehensive benchmark on an engine
    pub fn benchmark_engine(&self, engine: &mut dyn GameOfLifeEngine) -> Vec<BenchmarkResult> {
        let mut results = Vec::new();

        for &(width, height) in &self.grid_sizes {
            for &steps in &self.step_counts {
                for pattern in &self.test_patterns {
                    if let Ok(grid) = self.create_test_grid(pattern, width, height) {
                        engine.set_grid(&grid);

                        let start = Instant::now();
                        engine.run_steps(steps);
                        let duration = start.elapsed();

                        let final_grid = engine.get_grid();
                        let result = BenchmarkResult {
                            engine_name: format!(
                                "{}-{}-{}x{}-{}",
                                engine.benchmark_info().name,
                                pattern.name,
                                width,
                                height,
                                steps
                            ),
                            steps,
                            duration,
                            total_cells: final_grid.total_cells(),
                            live_cells: final_grid.count_live_cells(),
                            cells_per_second: (final_grid.total_cells() as f64 * steps as f64)
                                / duration.as_secs_f64(),
                        };

                        results.push(result);
                    }
                }
            }
        }

        results
    }

    /// Compare two engines across all benchmarks
    pub fn compare_engines(
        &self,
        baseline: &mut dyn GameOfLifeEngine,
        optimized: &mut dyn GameOfLifeEngine,
    ) -> Vec<BenchmarkComparison> {
        let baseline_results = self.benchmark_engine(baseline);
        let optimized_results = self.benchmark_engine(optimized);

        baseline_results
            .into_iter()
            .zip(optimized_results.into_iter())
            .map(|(base, opt)| BenchmarkComparison::new(base, opt))
            .collect()
    }

    /// Create a test grid from a pattern, scaling it to fit the target size
    fn create_test_grid(
        &self,
        pattern: &TestPattern,
        width: usize,
        height: usize,
    ) -> Result<StandardGrid, String> {
        let mut grid = StandardGrid::new(width, height);

        let pattern_height = pattern.pattern.len();
        if pattern_height == 0 {
            return Ok(grid);
        }

        let pattern_width = pattern.pattern[0].len();
        if pattern_width == 0 {
            return Ok(grid);
        }

        let start_row = (height.saturating_sub(pattern_height)) / 2;
        let start_col = (width.saturating_sub(pattern_width)) / 2;

        for (row_offset, line) in pattern.pattern.iter().enumerate() {
            for (col_offset, ch) in line.chars().enumerate() {
                let grid_row = start_row + row_offset;
                let grid_col = start_col + col_offset;

                if grid_row < height && grid_col < width {
                    let alive = match ch {
                        '#' | '█' | '*' | 'O' => true,
                        _ => false,
                    };
                    grid.set_cell(grid_row, grid_col, alive);
                }
            }
        }

        Ok(grid)
    }

    /// Get default test patterns
    fn default_patterns() -> Vec<TestPattern> {
        vec![
            Self::blinker_pattern(),
            Self::block_pattern(),
            Self::glider_pattern(),
            Self::random_pattern(),
        ]
    }

    /// Blinker pattern (period-2 oscillator)
    fn blinker_pattern() -> TestPattern {
        TestPattern {
            name: "blinker".to_string(),
            description: "Simple period-2 oscillator".to_string(),
            pattern: vec!["...".to_string(), "###".to_string(), "...".to_string()],
        }
    }

    /// Block pattern (still life)
    fn block_pattern() -> TestPattern {
        TestPattern {
            name: "block".to_string(),
            description: "Simple still life".to_string(),
            pattern: vec![
                "....".to_string(),
                ".##.".to_string(),
                ".##.".to_string(),
                "....".to_string(),
            ],
        }
    }

    /// Glider pattern (moving spaceship)
    fn glider_pattern() -> TestPattern {
        TestPattern {
            name: "glider".to_string(),
            description: "Simple moving spaceship".to_string(),
            pattern: vec![
                ".....".to_string(),
                "..#..".to_string(),
                "...#.".to_string(),
                ".###.".to_string(),
                ".....".to_string(),
            ],
        }
    }

    /// Random pattern for stress testing
    fn random_pattern() -> TestPattern {
        TestPattern {
            name: "random".to_string(),
            description: "Random pattern for stress testing".to_string(),
            pattern: vec![
                "##.#.##.#".to_string(),
                ".#..#..#.".to_string(),
                "#.#.#.#.#".to_string(),
                "..#...#..".to_string(),
                "#.#.#.#.#".to_string(),
                ".#..#..#.".to_string(),
                "##.#.##.#".to_string(),
            ],
        }
    }
}

impl Default for BenchmarkSuite {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::naive::NaiveEngine;

    #[test]
    fn test_benchmark_suite_creation() {
        let suite = BenchmarkSuite::minimal();
        assert!(!suite.test_patterns.is_empty());
        assert!(!suite.grid_sizes.is_empty());
        assert!(!suite.step_counts.is_empty());
    }

    #[test]
    fn test_pattern_creation() {
        let suite = BenchmarkSuite::new();
        let pattern = &suite.test_patterns[0];
        let grid = suite.create_test_grid(pattern, 10, 10).unwrap();

        assert_eq!(grid.width(), 10);
        assert_eq!(grid.height(), 10);
    }

    #[test]
    fn test_engine_benchmark() {
        let suite = BenchmarkSuite::minimal();
        let mut engine = NaiveEngine::new(50, 50);

        let results = suite.benchmark_engine(&mut engine);
        assert!(!results.is_empty());

        for result in results {
            assert!(result.duration.as_nanos() > 0);
            assert!(result.cells_per_second > 0.0);
        }
    }
}
