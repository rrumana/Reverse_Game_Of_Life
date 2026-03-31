use crate::engines::{EngineInfo, GameOfLifeEngine};
use crate::grid::{Grid, StandardGrid};
use rayon::prelude::*;

/// Naive Game of Life engine using basic cell-by-cell simulation
pub struct NaiveEngine {
    grid: StandardGrid,
    next_grid: StandardGrid,
}

impl NaiveEngine {
    /// Create a new naive engine with the specified grid dimensions
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            grid: StandardGrid::new(width, height),
            next_grid: StandardGrid::new(width, height),
        }
    }

    /// Create a new naive engine from an existing grid
    pub fn from_grid(grid: &dyn Grid) -> Self {
        let width = grid.width();
        let height = grid.height();
        let mut new_grid = StandardGrid::new(width, height);

        for row in 0..height {
            for col in 0..width {
                new_grid.set_cell(row, col, grid.get_cell(row, col));
            }
        }

        Self {
            grid: new_grid,
            next_grid: StandardGrid::new(width, height),
        }
    }

    /// Update using a safer approach that collects results first
    fn update_safe(&mut self) {
        let width = self.grid.width();
        let height = self.grid.height();

        let new_cells: Vec<bool> = (0..height * width)
            .into_par_iter()
            .map(|idx| {
                let row = idx / width;
                let col = idx % width;
                let neighbors = self.grid.count_neighbors(row, col);
                let current_cell = self.grid.get_cell(row, col);

                match (current_cell, neighbors) {
                    (true, 2) | (true, 3) | (false, 3) => true,
                    _ => false,
                }
            })
            .collect();

        for (idx, &alive) in new_cells.iter().enumerate() {
            let row = idx / width;
            let col = idx % width;
            self.next_grid.set_cell(row, col, alive);
        }

        std::mem::swap(&mut self.grid, &mut self.next_grid);
    }

    /// Get the width of the grid
    pub fn width(&self) -> usize {
        self.grid.width()
    }

    /// Get the height of the grid
    pub fn height(&self) -> usize {
        self.grid.height()
    }

    /// Get a cell value
    pub fn get_cell(&self, row: usize, col: usize) -> bool {
        self.grid.get_cell(row, col)
    }

    /// Count live cells in the grid
    pub fn count_live_cells(&self) -> usize {
        self.grid.count_live_cells()
    }
}

impl GameOfLifeEngine for NaiveEngine {
    fn step(&mut self) {
        self.update_safe();
    }

    fn get_grid(&self) -> &dyn Grid {
        &self.grid
    }

    fn set_grid(&mut self, grid: &dyn Grid) {
        if self.grid.width() != grid.width() || self.grid.height() != grid.height() {
            self.grid = StandardGrid::new(grid.width(), grid.height());
            self.next_grid = StandardGrid::new(grid.width(), grid.height());
        } else {
            self.grid.clear();
        }

        for row in 0..grid.height() {
            for col in 0..grid.width() {
                self.grid.set_cell(row, col, grid.get_cell(row, col));
            }
        }
    }

    fn get_cell(&self, row: usize, col: usize) -> bool {
        self.grid.get_cell(row, col)
    }

    fn benchmark_info(&self) -> EngineInfo {
        EngineInfo {
            name: "Naive".to_string(),
            description: "Basic cell-by-cell simulation with parallel row processing".to_string(),
            memory_per_cell_bits: 8.0,
            supports_parallel: true,
            supports_simd: false,
            min_grid_size: Some((1, 1)),
            max_grid_size: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::StandardGrid;

    #[test]
    fn test_naive_engine_creation() {
        let engine = NaiveEngine::new(10, 10);
        assert_eq!(engine.get_grid().width(), 10);
        assert_eq!(engine.get_grid().height(), 10);
        assert_eq!(engine.get_grid().count_live_cells(), 0);
    }

    #[test]
    fn test_blinker_pattern() {
        let pattern = ["...", "###", "..."];

        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();
        let mut engine = NaiveEngine::from_grid(&grid as &dyn Grid);

        assert_eq!(engine.get_grid().count_live_cells(), 3);
        assert!(engine.get_grid().get_cell(1, 0));
        assert!(engine.get_grid().get_cell(1, 1));
        assert!(engine.get_grid().get_cell(1, 2));

        engine.step();
        assert_eq!(engine.get_grid().count_live_cells(), 3);
        assert!(engine.get_grid().get_cell(0, 1));
        assert!(engine.get_grid().get_cell(1, 1));
        assert!(engine.get_grid().get_cell(2, 1));

        engine.step();
        assert_eq!(engine.get_grid().count_live_cells(), 3);
        assert!(engine.get_grid().get_cell(1, 0));
        assert!(engine.get_grid().get_cell(1, 1));
        assert!(engine.get_grid().get_cell(1, 2));
    }

    #[test]
    fn test_block_pattern() {
        let pattern = ["....", ".##.", ".##.", "...."];

        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();
        let mut engine = NaiveEngine::from_grid(&grid as &dyn Grid);

        let initial_count = engine.get_grid().count_live_cells();
        assert_eq!(initial_count, 4);

        engine.step();
        assert_eq!(engine.get_grid().count_live_cells(), initial_count);

        engine.step();
        assert_eq!(engine.get_grid().count_live_cells(), initial_count);
    }
}
