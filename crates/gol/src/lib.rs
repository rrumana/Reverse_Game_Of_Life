#![feature(portable_simd)]

pub mod benchmark;
pub mod engines;
pub mod grid;

pub use engines::{EngineInfo, GameOfLifeEngine};
pub use grid::Grid;

pub mod prelude {
    pub use crate::engines::naive::NaiveEngine;
    pub use crate::engines::ultimate::{
        auto_from_grid_ultimate_engine, auto_new_ultimate_engine, create_optimal_engine,
        safe_auto_new_ultimate_engine, UltimateEngine,
    };
    pub use crate::engines::{EngineInfo, GameOfLifeEngine};
    pub use crate::grid::{Grid, StandardGrid};
}
