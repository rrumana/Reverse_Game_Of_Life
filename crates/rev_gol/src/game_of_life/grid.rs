//! Grid representation and utilities for Game of Life

use crate::config::BoundaryCondition;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a Game of Life grid
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<bool>,
    pub boundary_condition: BoundaryCondition,
}

impl Grid {
    /// Create a new empty grid
    pub fn new(width: usize, height: usize, boundary_condition: BoundaryCondition) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
            boundary_condition,
        }
    }

    /// Create a grid from a 2D boolean array
    pub fn from_cells(
        cells: Vec<Vec<bool>>,
        boundary_condition: BoundaryCondition,
    ) -> Result<Self> {
        if cells.is_empty() {
            anyhow::bail!("Grid cannot be empty");
        }

        let height = cells.len();
        let width = cells[0].len();

        if width == 0 {
            anyhow::bail!("Grid width cannot be zero");
        }

        // Verify all rows have the same length
        for (i, row) in cells.iter().enumerate() {
            if row.len() != width {
                anyhow::bail!("Row {} has length {}, expected {}", i, row.len(), width);
            }
        }

        let flat_cells: Vec<bool> = cells.into_iter().flatten().collect();

        Ok(Self {
            width,
            height,
            cells: flat_cells,
            boundary_condition,
        })
    }

    /// Convert 2D coordinates to 1D index (reused from existing implementation)
    #[inline]
    pub fn index(&self, row: usize, col: usize) -> usize {
        row * self.width + col
    }

    /// Get cell value at coordinates
    pub fn get(&self, row: usize, col: usize) -> bool {
        if row < self.height && col < self.width {
            self.cells[self.index(row, col)]
        } else {
            false // Out of bounds cells are considered dead
        }
    }

    /// Set cell value at coordinates
    pub fn set(&mut self, row: usize, col: usize, value: bool) -> Result<()> {
        if row >= self.height || col >= self.width {
            anyhow::bail!(
                "Coordinates ({}, {}) out of bounds for {}x{} grid",
                row,
                col,
                self.height,
                self.width
            );
        }
        let idx = self.index(row, col);
        self.cells[idx] = value;
        Ok(())
    }

    /// Count living neighbors for a cell (adapted from existing implementation)
    pub fn count_neighbors(&self, row: usize, col: usize) -> u8 {
        let mut count = 0;

        for dr in [-1, 0, 1].iter() {
            for dc in [-1, 0, 1].iter() {
                if *dr == 0 && *dc == 0 {
                    continue; // Skip the cell itself
                }

                let r = row as isize + dr;
                let c = col as isize + dc;

                if self.is_neighbor_alive(r, c) {
                    count += 1;
                }
            }
        }

        count
    }

    /// Check if a neighbor at given coordinates is alive, handling boundary conditions
    fn is_neighbor_alive(&self, row: isize, col: isize) -> bool {
        match self.boundary_condition {
            BoundaryCondition::Dead => {
                if row >= 0 && row < self.height as isize && col >= 0 && col < self.width as isize {
                    self.cells[self.index(row as usize, col as usize)]
                } else {
                    false // Out of bounds cells are dead
                }
            }
            BoundaryCondition::Wrap => {
                let wrapped_row = ((row % self.height as isize + self.height as isize)
                    % self.height as isize) as usize;
                let wrapped_col = ((col % self.width as isize + self.width as isize)
                    % self.width as isize) as usize;
                self.cells[self.index(wrapped_row, wrapped_col)]
            }
            BoundaryCondition::Mirror => {
                let mirrored_row = if row < 0 {
                    (-row - 1) as usize
                } else if row >= self.height as isize {
                    self.height - 1 - (row - self.height as isize) as usize
                } else {
                    row as usize
                };

                let mirrored_col = if col < 0 {
                    (-col - 1) as usize
                } else if col >= self.width as isize {
                    self.width - 1 - (col - self.width as isize) as usize
                } else {
                    col as usize
                };

                if mirrored_row < self.height && mirrored_col < self.width {
                    self.cells[self.index(mirrored_row, mirrored_col)]
                } else {
                    false
                }
            }
        }
    }

    /// Get all living cell coordinates
    pub fn living_cells(&self) -> Vec<(usize, usize)> {
        let mut living = Vec::new();
        for row in 0..self.height {
            for col in 0..self.width {
                if self.get(row, col) {
                    living.push((row, col));
                }
            }
        }
        living
    }

    /// Count total living cells
    pub fn living_count(&self) -> usize {
        self.cells.iter().filter(|&&cell| cell).count()
    }

    /// Check if the grid is empty (no living cells)
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|&cell| !cell)
    }

    /// Create a copy of the grid with different boundary conditions
    pub fn with_boundary_condition(&self, boundary_condition: BoundaryCondition) -> Self {
        Self {
            width: self.width,
            height: self.height,
            cells: self.cells.clone(),
            boundary_condition,
        }
    }
}

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..self.height {
            for col in 0..self.width {
                let cell = self.get(row, col);
                let symbol = if cell { "⬛" } else { "⬜" };
                write!(f, "{}", symbol)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_creation() {
        let grid = Grid::new(3, 3, BoundaryCondition::Dead);
        assert_eq!(grid.width, 3);
        assert_eq!(grid.height, 3);
        assert_eq!(grid.cells.len(), 9);
        assert!(grid.is_empty());
    }

    #[test]
    fn test_grid_from_cells() {
        let cells = vec![
            vec![true, false, true],
            vec![false, true, false],
            vec![true, false, true],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        assert_eq!(grid.width, 3);
        assert_eq!(grid.height, 3);
        assert_eq!(grid.living_count(), 5);
    }

    #[test]
    fn test_neighbor_counting() {
        let cells = vec![
            vec![true, true, true],
            vec![true, false, true],
            vec![true, true, true],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        // Center cell should have 8 neighbors
        assert_eq!(grid.count_neighbors(1, 1), 8);

        // Corner cell should have 3 neighbors
        assert_eq!(grid.count_neighbors(0, 0), 2); // Only 2 because center is dead
    }

    #[test]
    fn test_boundary_conditions() {
        let cells = vec![vec![true, false], vec![false, true]];

        // Test dead boundary
        let grid_dead = Grid::from_cells(cells.clone(), BoundaryCondition::Dead).unwrap();
        assert_eq!(grid_dead.count_neighbors(0, 0), 1); // Only (1,1) is alive

        // Test wrap boundary
        let grid_wrap = Grid::from_cells(cells, BoundaryCondition::Wrap).unwrap();
        assert_eq!(grid_wrap.count_neighbors(0, 0), 4); // Multiple wrapping positions point to (1,1)
    }
}
