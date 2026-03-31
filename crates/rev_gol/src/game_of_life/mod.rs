//! Game of Life core functionality

pub mod grid;
pub mod io;
pub mod rules;

pub use grid::Grid;
pub use io::{create_example_grids, load_grid_from_file, save_grid_to_file};
pub use rules::GameOfLifeRules;
