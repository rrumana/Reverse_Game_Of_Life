//! Reverse Game of Life problem definition

use super::{Solution, SolutionValidator};
use crate::config::Settings;
use crate::game_of_life::{load_grid_from_file, Grid};
use crate::sat::SatEncoder;
use anyhow::{Context, Result};
use std::time::Instant;

/// Represents a reverse Game of Life problem
pub struct ReverseProblem {
    settings: Settings,
    target_grid: Grid,
    encoder: SatEncoder,
    validator: SolutionValidator,
}

impl ReverseProblem {
    /// Create a new reverse problem from settings
    pub fn new(settings: Settings) -> Result<Self> {
        // Load the target grid from file
        let target_grid = load_grid_from_file(
            &settings.input.target_state_file,
            settings.simulation.boundary_condition.clone(),
        )
        .context("Failed to load target state file")?;

        let encoder = SatEncoder::new(settings.clone(), &target_grid)
            .context("Failed to create SAT encoder")?;
        let validator = SolutionValidator::new(settings.clone());

        Ok(Self {
            settings,
            target_grid,
            encoder,
            validator,
        })
    }

    /// Create a problem with an explicit target grid (useful for testing)
    pub fn with_target_grid(settings: Settings, target_grid: Grid) -> Result<Self> {
        let encoder = SatEncoder::new(settings.clone(), &target_grid)
            .context("Failed to create SAT encoder")?;
        let validator = SolutionValidator::new(settings.clone());

        Ok(Self {
            settings,
            target_grid,
            encoder,
            validator,
        })
    }

    /// Solve the reverse problem and return all valid solutions
    pub fn solve(&mut self) -> Result<Vec<Solution>> {
        let start_time = Instant::now();

        println!("Solving reverse Game of Life problem...");
        println!(
            "Target grid: {}x{}, {} generations back",
            self.target_grid.width, self.target_grid.height, self.settings.simulation.generations
        );
        println!(
            "Target has {} living cells",
            self.target_grid.living_count()
        );

        // Show complexity estimate
        let complexity = self.encoder.estimate_complexity(&self.target_grid);
        println!("{}", complexity);

        // Solve using SAT encoding
        let predecessor_grids = self
            .encoder
            .solve(&self.target_grid)
            .context("SAT solving failed")?;

        let solve_time = start_time.elapsed();

        if predecessor_grids.is_empty() {
            println!("No solutions found!");
            return Ok(Vec::new());
        }

        println!(
            "Found {} candidate solutions in {:.3}s",
            predecessor_grids.len(),
            solve_time.as_secs_f64()
        );

        // Convert grids to Solution objects and validate
        let mut solutions = Vec::new();
        for (i, predecessor_grid) in predecessor_grids.into_iter().enumerate() {
            println!("Validating solution {}...", i + 1);

            match self
                .validator
                .validate(&predecessor_grid, &self.target_grid)
            {
                Ok(validation_result) => {
                    if validation_result.is_valid {
                        let solution = Solution::new(
                            predecessor_grid,
                            self.target_grid.clone(),
                            self.settings.simulation.generations,
                            validation_result.evolution_path,
                            solve_time,
                        );
                        solutions.push(solution);
                        println!("Solution {} is valid", i + 1);
                    } else {
                        eprintln!(
                            "Solution {} failed validation: {}",
                            i + 1,
                            validation_result
                                .error_message
                                .unwrap_or_else(|| "Unknown error".to_string())
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error validating solution {}: {}", i + 1, e);
                }
            }
        }

        println!("Found {} valid solutions", solutions.len());
        Ok(solutions)
    }

    /// Get the target grid
    pub fn target_grid(&self) -> &Grid {
        &self.target_grid
    }

    /// Get the problem settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Get encoding statistics
    pub fn encoding_statistics(&self) -> crate::sat::encoder::EncodingStatistics {
        self.encoder.statistics()
    }

    /// Check if the problem is likely solvable
    pub fn estimate_solvability(&self) -> SolvabilityEstimate {
        let complexity = self.encoder.estimate_complexity(&self.target_grid);
        let living_cells = self.target_grid.living_count();
        let total_cells = self.target_grid.width * self.target_grid.height;

        // Heuristics for solvability
        let density = living_cells as f64 / total_cells as f64;
        let is_empty = living_cells == 0;
        let is_full = living_cells == total_cells;
        let has_known_patterns = self.detect_known_patterns();

        let likelihood = if is_empty {
            SolvabilityLikelihood::High // Empty grid has many predecessors
        } else if is_full {
            SolvabilityLikelihood::Low // Full grid is unlikely to have predecessors
        } else if has_known_patterns {
            SolvabilityLikelihood::High // Known patterns often have predecessors
        } else if density < 0.1 {
            SolvabilityLikelihood::Medium // Sparse grids often have solutions
        } else if density > 0.8 {
            SolvabilityLikelihood::Low // Dense grids are harder to reverse
        } else {
            SolvabilityLikelihood::Medium
        };

        let complexity_level = complexity.complexity_level.clone();
        let estimated_solve_time = self.estimate_solve_time(&complexity);
        let recommendations = self.generate_recommendations(&complexity, density);

        SolvabilityEstimate {
            likelihood,
            complexity_level,
            living_cell_density: density,
            estimated_solve_time,
            recommendations,
        }
    }

    /// Detect known Game of Life patterns in the target grid
    fn detect_known_patterns(&self) -> bool {
        // Simple pattern detection - could be expanded
        let living_cells = self.target_grid.living_count();

        // Check for common still lifes
        if living_cells == 4 {
            // Might be a block or beehive
            return true;
        }

        // Check for common oscillators
        if living_cells == 3 {
            // Might be a blinker
            return self.detect_blinker_pattern();
        }

        // Check for gliders (5 cells in specific pattern)
        if living_cells == 5 {
            return self.detect_glider_pattern();
        }

        false
    }

    /// Detect blinker pattern (3 cells in a row)
    fn detect_blinker_pattern(&self) -> bool {
        let living_cells = self.target_grid.living_cells();
        if living_cells.len() != 3 {
            return false;
        }

        // Check for horizontal blinker
        let mut rows: Vec<_> = living_cells.iter().map(|(r, _)| *r).collect();
        rows.sort();
        let mut cols: Vec<_> = living_cells.iter().map(|(_, c)| *c).collect();
        cols.sort();

        // Horizontal: same row, consecutive columns
        if rows[0] == rows[1]
            && rows[1] == rows[2]
            && cols[1] == cols[0] + 1
            && cols[2] == cols[1] + 1
        {
            return true;
        }

        // Vertical: same column, consecutive rows
        if cols[0] == cols[1]
            && cols[1] == cols[2]
            && rows[1] == rows[0] + 1
            && rows[2] == rows[1] + 1
        {
            return true;
        }

        false
    }

    /// Detect glider pattern
    fn detect_glider_pattern(&self) -> bool {
        // This is a simplified check - a full implementation would check
        // all rotations and reflections of the glider pattern
        self.target_grid.living_count() == 5
    }

    /// Estimate solve time based on complexity
    fn estimate_solve_time(
        &self,
        complexity: &crate::sat::encoder::ComplexityEstimate,
    ) -> EstimatedTime {
        match complexity.complexity_level {
            crate::sat::encoder::ComplexityLevel::Low => EstimatedTime::Seconds(1),
            crate::sat::encoder::ComplexityLevel::Medium => EstimatedTime::Seconds(30),
            crate::sat::encoder::ComplexityLevel::High => EstimatedTime::Minutes(5),
            crate::sat::encoder::ComplexityLevel::VeryHigh => EstimatedTime::Minutes(30),
        }
    }

    /// Generate recommendations for solving the problem
    fn generate_recommendations(
        &self,
        complexity: &crate::sat::encoder::ComplexityEstimate,
        density: f64,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        match complexity.complexity_level {
            crate::sat::encoder::ComplexityLevel::VeryHigh => {
                recommendations.push("Consider reducing the grid size".to_string());
                recommendations.push("Consider reducing the number of generations".to_string());
                recommendations.push(
                    "Use the ParKissat backend with multiple threads for large instances"
                        .to_string(),
                );
            }
            crate::sat::encoder::ComplexityLevel::High => {
                recommendations.push(
                    "Prefer the ParKissat backend when thread-level parallelism is available"
                        .to_string(),
                );
                recommendations.push("Monitor memory usage during solving".to_string());
            }
            _ => {}
        }

        if density > 0.7 {
            recommendations.push(
                "Dense grids are harder to reverse - consider if this is the correct target"
                    .to_string(),
            );
        }

        if self.settings.simulation.generations > 5 {
            recommendations.push(
                "Many generations make the problem harder - consider reducing if possible"
                    .to_string(),
            );
        }

        if recommendations.is_empty() {
            recommendations.push("Problem looks reasonable to solve".to_string());
        }

        recommendations
    }
}

/// Estimate of problem solvability
#[derive(Debug, Clone)]
pub struct SolvabilityEstimate {
    pub likelihood: SolvabilityLikelihood,
    pub complexity_level: crate::sat::encoder::ComplexityLevel,
    pub living_cell_density: f64,
    pub estimated_solve_time: EstimatedTime,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolvabilityLikelihood {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone)]
pub enum EstimatedTime {
    Seconds(u32),
    Minutes(u32),
    Hours(u32),
}

impl std::fmt::Display for SolvabilityEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Solvability Estimate:")?;
        writeln!(f, "  Likelihood: {:?}", self.likelihood)?;
        writeln!(f, "  Complexity: {:?}", self.complexity_level)?;
        writeln!(
            f,
            "  Living cell density: {:.1}%",
            self.living_cell_density * 100.0
        )?;
        writeln!(f, "  Estimated solve time: {}", self.estimated_solve_time)?;
        writeln!(f, "  Recommendations:")?;
        for rec in &self.recommendations {
            writeln!(f, "    - {}", rec)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for EstimatedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstimatedTime::Seconds(s) => write!(f, "~{} seconds", s),
            EstimatedTime::Minutes(m) => write!(f, "~{} minutes", m),
            EstimatedTime::Hours(h) => write!(f, "~{} hours", h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::path::PathBuf;

    fn create_test_settings() -> Settings {
        Settings {
            simulation: SimulationConfig {
                generations: 1,
                boundary_condition: BoundaryCondition::Dead,
            },
            solver: SolverConfig {
                max_solutions: 5,
                timeout_seconds: 10,
                num_threads: Some(1),
                enable_preprocessing: false,
                verbosity: 0,
                backend: SolverBackend::Cadical,
            },
            input: InputConfig {
                target_state_file: PathBuf::from("test.txt"),
            },
            output: OutputConfig {
                format: OutputFormat::Text,
                save_intermediate: false,
                output_directory: PathBuf::from("output"),
            },
            encoding: EncodingConfig {
                symmetry_breaking: false,
            },
        }
    }

    #[test]
    fn test_problem_creation_with_grid() {
        let settings = create_test_settings();
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        let problem = ReverseProblem::with_target_grid(settings, target_grid).unwrap();
        // Pattern has 4 living cells: (0,1), (1,0), (1,2), (2,1)
        assert_eq!(problem.target_grid().living_count(), 4);
    }

    #[test]
    fn test_solvability_estimation() {
        let settings = create_test_settings();
        let cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        let problem = ReverseProblem::with_target_grid(settings, target_grid).unwrap();
        let estimate = problem.estimate_solvability();

        assert_eq!(estimate.likelihood, SolvabilityLikelihood::High); // Should detect blinker
    }

    #[test]
    fn test_pattern_detection() {
        let settings = create_test_settings();

        // Test blinker detection
        let blinker_cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];
        let blinker_grid = Grid::from_cells(blinker_cells, BoundaryCondition::Dead).unwrap();
        let problem = ReverseProblem::with_target_grid(settings.clone(), blinker_grid).unwrap();
        assert!(problem.detect_blinker_pattern());
    }
}
