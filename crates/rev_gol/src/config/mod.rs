//! Configuration management for the reverse Game of Life solver

pub mod settings;

pub use settings::{
    BoundaryCondition, CliOverrides, EncodingConfig, InputConfig, OutputConfig, OutputFormat,
    Settings, SimulationConfig, SolverBackend, SolverConfig,
};
