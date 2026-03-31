//! Game of Life rules implementation (adapted from existing implementation)

use super::Grid;
use rayon::prelude::*;

/// Game of Life rules engine
pub struct GameOfLifeRules;

impl GameOfLifeRules {
    /// Apply Game of Life rules to evolve the grid one generation forward
    /// (Adapted from the existing implementation's update function)
    pub fn evolve(current: &Grid) -> Grid {
        let mut next = Grid::new(
            current.width,
            current.height,
            current.boundary_condition.clone(),
        );

        // Use parallel processing for better performance on large grids
        let next_cells: Vec<bool> = (0..current.height)
            .into_par_iter()
            .flat_map(|row| {
                (0..current.width).into_par_iter().map(move |col| {
                    let neighbors = current.count_neighbors(row, col);
                    let current_cell = current.get(row, col);

                    // Apply Conway's Game of Life rules
                    match (current_cell, neighbors) {
                        (true, 2) | (true, 3) | (false, 3) => true, // Survive or birth
                        _ => false,                                 // Death
                    }
                })
            })
            .collect();

        next.cells = next_cells;
        next
    }

    /// Evolve the grid for multiple generations
    pub fn evolve_generations(mut grid: Grid, generations: usize) -> Grid {
        for _ in 0..generations {
            grid = Self::evolve(&grid);
        }
        grid
    }

    /// Check if a cell should be alive in the next generation given its current state and neighbor count
    pub fn should_be_alive(current_state: bool, neighbor_count: u8) -> bool {
        match (current_state, neighbor_count) {
            (true, 2) | (true, 3) | (false, 3) => true,
            _ => false,
        }
    }

    /// Get all possible neighbor counts that would result in a live cell
    pub fn live_neighbor_counts() -> Vec<u8> {
        vec![2, 3] // For live cells: 2 or 3 neighbors to survive
    }

    /// Get neighbor counts that would result in birth (dead -> alive)
    pub fn birth_neighbor_counts() -> Vec<u8> {
        vec![3] // For dead cells: exactly 3 neighbors for birth
    }

    /// Get neighbor counts that would result in survival (alive -> alive)
    pub fn survival_neighbor_counts() -> Vec<u8> {
        vec![2, 3] // For live cells: 2 or 3 neighbors to survive
    }

    /// Validate that a predecessor state correctly evolves to the target state
    pub fn validate_evolution(predecessor: &Grid, target: &Grid, generations: usize) -> bool {
        if predecessor.width != target.width || predecessor.height != target.height {
            return false;
        }

        let evolved = Self::evolve_generations(predecessor.clone(), generations);
        evolved == *target
    }

    /// Check if two grids are equivalent (same living cells)
    pub fn grids_equal(grid1: &Grid, grid2: &Grid) -> bool {
        grid1.width == grid2.width && grid1.height == grid2.height && grid1.cells == grid2.cells
    }

    /// Get the maximum possible neighbor count for any cell
    pub fn max_neighbor_count() -> u8 {
        8 // Maximum 8 neighbors in Moore neighborhood
    }

    /// Check if a neighbor count is valid (0-8)
    pub fn is_valid_neighbor_count(count: u8) -> bool {
        count <= Self::max_neighbor_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BoundaryCondition;

    #[test]
    fn test_still_life_block() {
        // 2x2 block should remain stable
        let cells = vec![
            vec![false, false, false, false],
            vec![false, true, true, false],
            vec![false, true, true, false],
            vec![false, false, false, false],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let evolved = GameOfLifeRules::evolve(&grid);

        assert!(GameOfLifeRules::grids_equal(&grid, &evolved));
    }

    #[test]
    fn test_oscillator_blinker() {
        // Vertical blinker
        let cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let evolved = GameOfLifeRules::evolve(&grid);

        // Should become horizontal blinker
        let expected_cells = vec![
            vec![false, true, false],
            vec![false, true, false],
            vec![false, true, false],
        ];
        let expected = Grid::from_cells(expected_cells, BoundaryCondition::Dead).unwrap();

        assert!(GameOfLifeRules::grids_equal(&evolved, &expected));

        // Evolve again should return to original
        let evolved_twice = GameOfLifeRules::evolve(&evolved);
        assert!(GameOfLifeRules::grids_equal(&grid, &evolved_twice));
    }

    #[test]
    fn test_rule_logic() {
        // Test individual rule cases
        assert!(GameOfLifeRules::should_be_alive(true, 2)); // Survival with 2 neighbors
        assert!(GameOfLifeRules::should_be_alive(true, 3)); // Survival with 3 neighbors
        assert!(GameOfLifeRules::should_be_alive(false, 3)); // Birth with 3 neighbors
        assert!(!GameOfLifeRules::should_be_alive(true, 1)); // Death with 1 neighbor
        assert!(!GameOfLifeRules::should_be_alive(true, 4)); // Death with 4 neighbors
        assert!(!GameOfLifeRules::should_be_alive(false, 2)); // No birth with 2 neighbors
    }

    #[test]
    fn test_validation() {
        let cells = vec![
            vec![false, true, false],
            vec![false, true, false],
            vec![false, true, false],
        ];
        let predecessor = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        let target_cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];
        let target = Grid::from_cells(target_cells, BoundaryCondition::Dead).unwrap();

        assert!(GameOfLifeRules::validate_evolution(
            &predecessor,
            &target,
            1
        ));
        assert!(!GameOfLifeRules::validate_evolution(
            &predecessor,
            &target,
            2
        )); // Should be back to original after 2 steps
    }

    #[test]
    fn test_neighbor_count_constants() {
        assert_eq!(GameOfLifeRules::max_neighbor_count(), 8);
        assert!(GameOfLifeRules::is_valid_neighbor_count(0));
        assert!(GameOfLifeRules::is_valid_neighbor_count(8));
        assert!(!GameOfLifeRules::is_valid_neighbor_count(9));

        assert_eq!(GameOfLifeRules::birth_neighbor_counts(), vec![3]);
        assert_eq!(GameOfLifeRules::survival_neighbor_counts(), vec![2, 3]);
    }
}
