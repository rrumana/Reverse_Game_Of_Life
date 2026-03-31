//! File I/O operations for Game of Life grids

use super::Grid;
use crate::config::BoundaryCondition;
use anyhow::{Context, Result};
use std::path::Path;

/// Load a grid from a text file
/// Format: Each line represents a row, with '1' for alive cells and '0' for dead cells
pub fn load_grid_from_file<P: AsRef<Path>>(
    path: P,
    boundary_condition: BoundaryCondition,
) -> Result<Grid> {
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read grid file: {}", path.as_ref().display()))?;

    parse_grid_from_string(&content, boundary_condition).with_context(|| {
        format!(
            "Failed to parse grid from file: {}",
            path.as_ref().display()
        )
    })
}

/// Parse a grid from a string representation
pub fn parse_grid_from_string(
    content: &str,
    boundary_condition: BoundaryCondition,
) -> Result<Grid> {
    let lines: Vec<&str> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        anyhow::bail!("Grid file is empty or contains no valid rows");
    }

    let height = lines.len();
    let width = lines[0].len();

    if width == 0 {
        anyhow::bail!("Grid rows cannot be empty");
    }

    let mut cells = Vec::with_capacity(height);

    for (row_idx, line) in lines.iter().enumerate() {
        if line.len() != width {
            anyhow::bail!(
                "Row {} has length {}, expected {} (all rows must have the same length)",
                row_idx,
                line.len(),
                width
            );
        }

        let mut row = Vec::with_capacity(width);
        for (col_idx, ch) in line.chars().enumerate() {
            match ch {
                '0' => row.push(false),
                '1' => row.push(true),
                _ => anyhow::bail!(
                    "Invalid character '{}' at position ({}, {}). Only '0' and '1' are allowed",
                    ch,
                    row_idx,
                    col_idx
                ),
            }
        }
        cells.push(row);
    }

    Grid::from_cells(cells, boundary_condition)
}

/// Save a grid to a text file
pub fn save_grid_to_file<P: AsRef<Path>>(grid: &Grid, path: P) -> Result<()> {
    let content = grid_to_string(grid);

    // Create parent directories if they don't exist
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write grid to file: {}", path.as_ref().display()))?;

    Ok(())
}

/// Convert a grid to string representation
pub fn grid_to_string(grid: &Grid) -> String {
    let mut result = String::with_capacity(grid.height * (grid.width + 1));

    for row in 0..grid.height {
        for col in 0..grid.width {
            let cell = grid.get(row, col);
            result.push(if cell { '1' } else { '0' });
        }
        result.push('\n');
    }

    result
}

/// Load multiple grids from a directory
pub fn load_grids_from_directory<P: AsRef<Path>>(
    dir_path: P,
    boundary_condition: BoundaryCondition,
) -> Result<Vec<(String, Grid)>> {
    let dir = std::fs::read_dir(&dir_path)
        .with_context(|| format!("Failed to read directory: {}", dir_path.as_ref().display()))?;

    let mut grids = Vec::new();

    for entry in dir {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "txt" {
                    let filename = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    match load_grid_from_file(&path, boundary_condition.clone()) {
                        Ok(grid) => grids.push((filename, grid)),
                        Err(e) => eprintln!("Warning: Failed to load {}: {}", path.display(), e),
                    }
                }
            }
        }
    }

    grids.sort_by(|a, b| a.0.cmp(&b.0)); // Sort by filename
    Ok(grids)
}

/// Create example grid files for testing
pub fn create_example_grids<P: AsRef<Path>>(output_dir: P) -> Result<()> {
    let dir = output_dir.as_ref();
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create directory: {}", dir.display()))?;

    // Glider pattern
    let glider_content = "00100\n10100\n01100\n00000\n00000\n";
    std::fs::write(dir.join("glider.txt"), glider_content).context("Failed to write glider.txt")?;

    // Blinker pattern
    let blinker_content = "000\n111\n000\n";
    std::fs::write(dir.join("blinker.txt"), blinker_content)
        .context("Failed to write blinker.txt")?;

    // Block pattern (still life)
    let block_content = "0000\n0110\n0110\n0000\n";
    std::fs::write(dir.join("block.txt"), block_content).context("Failed to write block.txt")?;

    // Beacon pattern (oscillator)
    let beacon_content = "110000\n110000\n001100\n001100\n";
    std::fs::write(dir.join("beacon.txt"), beacon_content).context("Failed to write beacon.txt")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_grid_from_string() {
        let content = "010\n101\n010\n";
        let grid = parse_grid_from_string(content, BoundaryCondition::Dead).unwrap();

        assert_eq!(grid.width, 3);
        assert_eq!(grid.height, 3);

        assert_eq!(grid.living_count(), 4);
        assert!(grid.get(0, 1));
        assert!(grid.get(1, 0));
        assert!(grid.get(1, 2));
        assert!(grid.get(2, 1));
    }

    #[test]
    fn test_grid_to_string() {
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let string_repr = grid_to_string(&grid);

        assert_eq!(string_repr, "010\n101\n010\n");
    }

    #[test]
    fn test_round_trip() {
        let original_content = "010\n101\n010\n";
        let grid = parse_grid_from_string(original_content, BoundaryCondition::Dead).unwrap();
        let regenerated_content = grid_to_string(&grid);

        assert_eq!(original_content, regenerated_content);
    }

    #[test]
    fn test_file_operations() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test_grid.txt");

        let cells = vec![vec![true, false, true], vec![false, true, false]];
        let original_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        // Save grid
        save_grid_to_file(&original_grid, &file_path).unwrap();

        // Load grid
        let loaded_grid = load_grid_from_file(&file_path, BoundaryCondition::Dead).unwrap();

        assert_eq!(original_grid.width, loaded_grid.width);
        assert_eq!(original_grid.height, loaded_grid.height);
        assert_eq!(original_grid.cells, loaded_grid.cells);
    }

    #[test]
    fn test_invalid_input() {
        // Test invalid character
        let invalid_content = "010\n1X1\n010\n";
        assert!(parse_grid_from_string(invalid_content, BoundaryCondition::Dead).is_err());

        // Test inconsistent row lengths
        let inconsistent_content = "010\n11\n010\n";
        assert!(parse_grid_from_string(inconsistent_content, BoundaryCondition::Dead).is_err());

        // Test empty content
        let empty_content = "";
        assert!(parse_grid_from_string(empty_content, BoundaryCondition::Dead).is_err());
    }

    #[test]
    fn test_create_example_grids() {
        let temp_dir = tempdir().unwrap();
        create_example_grids(temp_dir.path()).unwrap();

        // Check that files were created
        assert!(temp_dir.path().join("glider.txt").exists());
        assert!(temp_dir.path().join("blinker.txt").exists());
        assert!(temp_dir.path().join("block.txt").exists());
        assert!(temp_dir.path().join("beacon.txt").exists());

        // Test loading one of the created files
        let glider =
            load_grid_from_file(temp_dir.path().join("glider.txt"), BoundaryCondition::Dead)
                .unwrap();
        assert_eq!(glider.width, 5);
        assert_eq!(glider.height, 5);
        assert_eq!(glider.living_count(), 5); // Glider has 5 living cells
    }
}
