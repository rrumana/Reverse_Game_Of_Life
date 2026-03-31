//! Configuration settings for the reverse Game of Life solver

use crate::utils::resolve_workspace_path;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub simulation: SimulationConfig,
    pub solver: SolverConfig,
    pub input: InputConfig,
    pub output: OutputConfig,
    pub encoding: EncodingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub generations: usize,
    pub boundary_condition: BoundaryCondition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryCondition {
    Dead,
    Wrap,
    Mirror,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
    pub max_solutions: usize,
    pub timeout_seconds: u64,
    pub num_threads: Option<usize>,
    pub enable_preprocessing: bool,
    pub verbosity: u32,
    pub backend: SolverBackend,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SolverBackend {
    Cadical,
    Parkissat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub target_state_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub save_intermediate: bool,
    pub output_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Text,
    Json,
    Visual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingConfig {
    pub symmetry_breaking: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            simulation: SimulationConfig {
                generations: 5,
                boundary_condition: BoundaryCondition::Dead,
            },
            solver: SolverConfig {
                max_solutions: 10,
                timeout_seconds: 300,
                num_threads: None, // Use available parallelism by default
                enable_preprocessing: true,
                verbosity: 0,
                backend: SolverBackend::Parkissat,
            },
            input: InputConfig {
                target_state_file: PathBuf::from("input/target_states/example.txt"),
            },
            output: OutputConfig {
                format: OutputFormat::Text,
                save_intermediate: false,
                output_directory: PathBuf::from("output/solutions"),
            },
            encoding: EncodingConfig {
                symmetry_breaking: false,
            },
        }
    }
}

impl Settings {
    /// Load settings from a YAML file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let resolved_path = resolve_workspace_path(path);
        let content = std::fs::read_to_string(&resolved_path)
            .with_context(|| format!("Failed to read config file: {}", resolved_path.display()))?;

        let settings: Settings = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", resolved_path.display()))?;

        let mut settings = settings;
        settings.normalize_paths();
        settings.validate()?;
        Ok(settings)
    }

    /// Save settings to a YAML file
    pub fn to_file(&self, path: &PathBuf) -> Result<()> {
        let content = serde_yaml::to_string(self).context("Failed to serialize settings")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Validate the settings
    pub fn validate(&self) -> Result<()> {
        if self.simulation.generations == 0 {
            anyhow::bail!("Number of generations must be positive");
        }

        if self.solver.max_solutions == 0 {
            anyhow::bail!("Maximum solutions must be positive");
        }

        let target_state_file = resolve_workspace_path(&self.input.target_state_file);
        if !target_state_file.exists() {
            anyhow::bail!(
                "Target state file does not exist: {}",
                target_state_file.display()
            );
        }

        Ok(())
    }

    /// Normalize any relative paths against the workspace root.
    pub fn normalize_paths(&mut self) {
        self.input.target_state_file = resolve_workspace_path(&self.input.target_state_file);
        self.output.output_directory = resolve_workspace_path(&self.output.output_directory);
    }

    /// Merge settings with command line overrides
    pub fn merge_with_cli(&mut self, cli_overrides: &CliOverrides) {
        if let Some(generations) = cli_overrides.generations {
            self.simulation.generations = generations;
        }
        if let Some(max_solutions) = cli_overrides.max_solutions {
            self.solver.max_solutions = max_solutions;
        }
        if let Some(ref target_file) = cli_overrides.target_file {
            self.input.target_state_file = target_file.clone();
        }
        if let Some(ref output_dir) = cli_overrides.output_dir {
            self.output.output_directory = output_dir.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{resolve_workspace_path, workspace_root};

    #[test]
    fn test_from_file_resolves_workspace_relative_paths() {
        let settings = Settings::from_file(&PathBuf::from("config/default.yaml")).unwrap();

        assert!(settings.input.target_state_file.is_absolute());
        assert!(settings
            .input
            .target_state_file
            .starts_with(workspace_root()));
        assert_eq!(
            settings.output.output_directory,
            resolve_workspace_path("output/solutions")
        );
        assert!(settings.input.target_state_file.exists());
        assert!(settings
            .output
            .output_directory
            .starts_with(workspace_root()));
    }
}

/// Command line overrides for settings
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub generations: Option<usize>,
    pub max_solutions: Option<usize>,
    pub target_file: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
}
