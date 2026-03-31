pub mod standard;

pub use standard::StandardGrid;

/// Trait for Game of Life grid representations
pub trait Grid {
    /// Get the width of the grid
    fn width(&self) -> usize;

    /// Get the height of the grid
    fn height(&self) -> usize;

    /// Get the state of a cell (true = alive, false = dead)
    fn get_cell(&self, row: usize, col: usize) -> bool;

    /// Set the state of a cell
    fn set_cell(&mut self, row: usize, col: usize, alive: bool);

    /// Clear all cells (set to dead)
    fn clear(&mut self);

    /// Get the total number of cells
    fn total_cells(&self) -> usize {
        self.width() * self.height()
    }

    /// Count live neighbors for a cell at the given position
    fn count_neighbors(&self, row: usize, col: usize) -> u8 {
        let mut count = 0;
        let height = self.height();
        let width = self.width();

        for dr in [-1, 0, 1].iter() {
            for dc in [-1, 0, 1].iter() {
                if *dr == 0 && *dc == 0 {
                    continue;
                }
                let r = row as isize + dr;
                let c = col as isize + dc;
                if r >= 0 && r < height as isize && c >= 0 && c < width as isize {
                    if self.get_cell(r as usize, c as usize) {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    /// Count total live cells in the grid
    fn count_live_cells(&self) -> usize {
        let mut count = 0;
        for row in 0..self.height() {
            for col in 0..self.width() {
                if self.get_cell(row, col) {
                    count += 1;
                }
            }
        }
        count
    }
}
