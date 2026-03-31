use super::Grid;

/// Standard grid implementation that stores each cell as a boolean
#[derive(Debug, Clone, PartialEq)]
pub struct StandardGrid {
    width: usize,
    height: usize,
    cells: Vec<bool>,
}

impl StandardGrid {
    /// Create a new empty grid with the specified dimensions
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    /// Create a grid from a 2D boolean array
    pub fn from_cells(cells: Vec<Vec<bool>>) -> Result<Self, String> {
        if cells.is_empty() {
            return Err("Grid cannot be empty".to_string());
        }

        let height = cells.len();
        let width = cells[0].len();

        if width == 0 {
            return Err("Grid width cannot be zero".to_string());
        }

        for (i, row) in cells.iter().enumerate() {
            if row.len() != width {
                return Err(format!(
                    "Row {} has length {}, expected {}",
                    i,
                    row.len(),
                    width
                ));
            }
        }

        let flat_cells: Vec<bool> = cells.into_iter().flatten().collect();

        Ok(Self {
            width,
            height,
            cells: flat_cells,
        })
    }

    /// Create a grid from a string representation
    pub fn from_string_pattern(
        pattern: &[&str],
        alive_char: char,
        dead_char: char,
    ) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("Pattern cannot be empty".to_string());
        }

        let height = pattern.len();
        let width = pattern[0].chars().count();

        if width == 0 {
            return Err("Pattern width cannot be zero".to_string());
        }

        let mut cells = Vec::with_capacity(width * height);

        for (row_idx, row) in pattern.iter().enumerate() {
            let row_chars: Vec<char> = row.chars().collect();
            if row_chars.len() != width {
                return Err(format!(
                    "Row {} has length {}, expected {}",
                    row_idx,
                    row_chars.len(),
                    width
                ));
            }

            for ch in row_chars {
                let cell = match ch {
                    c if c == alive_char => true,
                    c if c == dead_char => false,
                    _ => return Err(format!("Invalid character '{}' in pattern", ch)),
                };
                cells.push(cell);
            }
        }

        Ok(Self {
            width,
            height,
            cells,
        })
    }

    /// Create a grid from a file containing █ and ·
    pub fn from_file(file_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(file_path)?;
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Err("File is empty".into());
        }

        let height = lines.len();
        let width = lines[0].len();

        if width == 0 {
            return Err("Grid width cannot be zero".into());
        }

        let mut cells = Vec::with_capacity(width * height);

        for (row_idx, line) in lines.iter().enumerate() {
            if line.len() != width {
                return Err(format!(
                    "Row {} has length {}, expected {}",
                    row_idx,
                    line.len(),
                    width
                )
                .into());
            }

            for ch in line.chars() {
                let cell = match ch {
                    '1' => true,
                    '0' => false,
                    _ => {
                        return Err(format!(
                            "Invalid character '{}' in file. Only '0' and '1' are allowed.",
                            ch
                        )
                        .into())
                    }
                };
                cells.push(cell);
            }
        }

        Ok(Self {
            width,
            height,
            cells,
        })
    }

    /// Get the internal cell index for row, col coordinates
    fn index(&self, row: usize, col: usize) -> usize {
        row * self.width + col
    }

    /// Get a reference to the internal cells vector
    pub fn cells(&self) -> &[bool] {
        &self.cells
    }

    /// Get a mutable reference to the internal cells vector
    pub fn cells_mut(&mut self) -> &mut [bool] {
        &mut self.cells
    }
}

impl Grid for StandardGrid {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn get_cell(&self, row: usize, col: usize) -> bool {
        assert!(
            row < self.height && col < self.width,
            "Cell coordinates out of bounds"
        );
        self.cells[self.index(row, col)]
    }

    fn set_cell(&mut self, row: usize, col: usize, alive: bool) {
        assert!(
            row < self.height && col < self.width,
            "Cell coordinates out of bounds"
        );
        let idx = self.index(row, col);
        self.cells[idx] = alive;
    }

    fn clear(&mut self) {
        self.cells.fill(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_grid() {
        let grid = StandardGrid::new(10, 5);
        assert_eq!(grid.width(), 10);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid.total_cells(), 50);
        assert_eq!(grid.count_live_cells(), 0);
    }

    #[test]
    fn test_set_get_cell() {
        let mut grid = StandardGrid::new(3, 3);
        assert!(!grid.get_cell(1, 1));

        grid.set_cell(1, 1, true);
        assert!(grid.get_cell(1, 1));
        assert_eq!(grid.count_live_cells(), 1);
    }

    #[test]
    fn test_from_string_pattern() {
        let pattern = ["...", ".#.", "..."];

        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();
        assert_eq!(grid.width(), 3);
        assert_eq!(grid.height(), 3);
        assert!(grid.get_cell(1, 1));
        assert!(!grid.get_cell(0, 0));
        assert_eq!(grid.count_live_cells(), 1);
    }

    #[test]
    fn test_count_neighbors() {
        let pattern = ["#.#", ".#.", "#.#"];

        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();
        assert_eq!(grid.count_neighbors(1, 1), 4);
        assert_eq!(grid.count_neighbors(0, 0), 1);
        assert_eq!(grid.count_neighbors(0, 1), 3);
    }
}
