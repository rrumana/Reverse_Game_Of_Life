//! SAT encoder for the reverse Game of Life problem

use super::{ConstraintGenerator, SolverOptions, SolverSolution, UnifiedSatSolver};
use crate::config::Settings;
use crate::game_of_life::{GameOfLifeRules, Grid};
use anyhow::{Context, Result};
use std::time::Duration;

/// Main SAT encoder for reverse Game of Life problems
pub struct SatEncoder {
    settings: Settings,
    constraint_generator: ConstraintGenerator,
    solver: UnifiedSatSolver,
    grid_width: usize,
    grid_height: usize,
}

impl SatEncoder {
    /// Create a new SAT encoder with the given settings and target grid
    pub fn new(settings: Settings, target_grid: &Grid) -> Result<Self> {
        let constraint_generator = ConstraintGenerator::new(
            target_grid.width,
            target_grid.height,
            settings.simulation.generations + 1, // +1 because we need initial state + generations
            settings.simulation.boundary_condition.clone(),
            settings.encoding.symmetry_breaking,
        );

        let mut solver = UnifiedSatSolver::new(settings.solver.backend)
            .context("Failed to create SAT solver backend")?;

        // Configure solver based on settings
        let solver_options = SolverOptions {
            num_threads: settings.solver.num_threads,
            enable_preprocessing: settings.solver.enable_preprocessing,
            verbosity: settings.solver.verbosity,
            timeout: Some(Duration::from_secs(settings.solver.timeout_seconds)),
            random_seed: None,
        };
        solver
            .configure(&solver_options)
            .context("Failed to configure SAT solver")?;

        Ok(Self {
            settings,
            constraint_generator,
            solver,
            grid_width: target_grid.width,
            grid_height: target_grid.height,
        })
    }

    /// Encode and solve the reverse Game of Life problem
    pub fn solve(&mut self, target_grid: &Grid) -> Result<Vec<Grid>> {
        // Generate all SAT constraints
        let clauses = self
            .constraint_generator
            .generate_all_constraints(target_grid)
            .context("Failed to generate SAT constraints")?;

        println!(
            "Generated {} clauses with {} variables",
            clauses.len(),
            self.constraint_generator
                .variable_manager()
                .variable_count()
        );

        // Add constraints to solver
        self.solver
            .add_clauses(&clauses)
            .context("Failed to add clauses to SAT solver")?;

        let predecessor_vars = self
            .constraint_generator
            .variable_manager()
            .all_cell_variables_at_time(0)
            .context("Failed to collect predecessor variables for blocking clauses")?;

        // Solve for multiple solutions
        let solutions = self
            .solver
            .solve_multiple_with_blocking_vars(
                self.settings.solver.max_solutions,
                Some(&predecessor_vars),
            )
            .context("SAT solving failed")?;

        println!("Found {} solutions", solutions.len());

        // Convert SAT solutions to Game of Life grids
        let mut result_grids = Vec::new();
        for (i, solution) in solutions.iter().enumerate() {
            match self.extract_grid_from_solution(solution, 0) {
                Ok(grid) => {
                    // Validate the solution
                    if self.validate_solution(&grid, target_grid)? {
                        result_grids.push(grid);
                    } else {
                        eprintln!("Warning: Solution {} failed validation", i);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to extract grid from solution {}: {}", i, e);
                }
            }
        }

        Ok(result_grids)
    }

    /// Extract a Game of Life grid from a SAT solution at a specific time step
    fn extract_grid_from_solution(
        &mut self,
        solution: &SolverSolution,
        time_step: usize,
    ) -> Result<Grid> {
        let mut grid = Grid::new(
            self.grid_width,
            self.grid_height,
            self.settings.simulation.boundary_condition.clone(),
        );

        for y in 0..self.grid_height {
            for x in 0..self.grid_width {
                let cell_var = self
                    .constraint_generator
                    .variable_manager()
                    .cell_variable(x, y, time_step)?;

                let is_alive = solution.assignment.get(&cell_var).copied().unwrap_or(false);

                grid.set(y, x, is_alive)?;
            }
        }

        Ok(grid)
    }

    /// Validate that a predecessor grid correctly evolves to the target
    fn validate_solution(&self, predecessor: &Grid, target: &Grid) -> Result<bool> {
        let evolved = GameOfLifeRules::evolve_generations(
            predecessor.clone(),
            self.settings.simulation.generations,
        );

        Ok(GameOfLifeRules::grids_equal(&evolved, target))
    }

    /// Get all intermediate states from a solution
    pub fn extract_all_states(&mut self, solution: &SolverSolution) -> Result<Vec<Grid>> {
        let mut states = Vec::new();

        for t in 0..=self.settings.simulation.generations {
            let grid = self.extract_grid_from_solution(solution, t)?;
            states.push(grid);
        }

        Ok(states)
    }

    /// Get encoding statistics
    pub fn statistics(&self) -> EncodingStatistics {
        let constraint_stats = self.constraint_generator.statistics();

        EncodingStatistics {
            grid_width: self.grid_width,
            grid_height: self.grid_height,
            generations: self.settings.simulation.generations,
            total_variables: constraint_stats.total_variables,
            total_clauses: constraint_stats.total_clauses,
            boundary_condition: self.settings.simulation.boundary_condition.clone(),
        }
    }

    /// Reset the encoder for a new problem
    pub fn reset(&mut self) {
        self.solver.reset().expect("Failed to reset SAT solver");
        self.constraint_generator = ConstraintGenerator::new(
            self.grid_width,
            self.grid_height,
            self.settings.simulation.generations + 1,
            self.settings.simulation.boundary_condition.clone(),
            self.settings.encoding.symmetry_breaking,
        );
    }

    /// Check if the problem is likely to be solvable (heuristic check)
    pub fn estimate_complexity(&self, target_grid: &Grid) -> ComplexityEstimate {
        let total_cells = self.grid_width * self.grid_height;
        let time_steps = self.settings.simulation.generations + 1;
        let living_cells = target_grid.living_count();

        let total_variables = total_cells * time_steps;
        let estimated_clauses = self.constraint_generator.total_clause_count();
        let complexity_score = total_variables + estimated_clauses / 16;

        let complexity = if complexity_score < 10_000 {
            ComplexityLevel::Low
        } else if complexity_score < 100_000 {
            ComplexityLevel::Medium
        } else if complexity_score < 1_000_000 {
            ComplexityLevel::High
        } else {
            ComplexityLevel::VeryHigh
        };

        ComplexityEstimate {
            complexity_level: complexity,
            estimated_variables: total_variables,
            estimated_clauses,
            living_cells_ratio: living_cells as f64 / total_cells as f64,
            grid_size: total_cells,
            time_steps,
        }
    }
}

/// Statistics about the SAT encoding
#[derive(Debug, Clone)]
pub struct EncodingStatistics {
    pub grid_width: usize,
    pub grid_height: usize,
    pub generations: usize,
    pub total_variables: usize,
    pub total_clauses: usize,
    pub boundary_condition: crate::config::BoundaryCondition,
}

/// Complexity estimate for the problem
#[derive(Debug, Clone)]
pub struct ComplexityEstimate {
    pub complexity_level: ComplexityLevel,
    pub estimated_variables: usize,
    pub estimated_clauses: usize,
    pub living_cells_ratio: f64,
    pub grid_size: usize,
    pub time_steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
    VeryHigh,
}

impl std::fmt::Display for EncodingStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SAT Encoding Statistics:")?;
        writeln!(f, "  Grid: {}x{}", self.grid_width, self.grid_height)?;
        writeln!(f, "  Generations: {}", self.generations)?;
        writeln!(f, "  Total variables: {}", self.total_variables)?;
        writeln!(f, "  Total clauses: {}", self.total_clauses)?;
        writeln!(f, "  Boundary condition: {:?}", self.boundary_condition)?;
        Ok(())
    }
}

impl std::fmt::Display for ComplexityEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Problem Complexity Estimate:")?;
        writeln!(f, "  Complexity level: {:?}", self.complexity_level)?;
        writeln!(f, "  Estimated variables: {}", self.estimated_variables)?;
        writeln!(f, "  Estimated clauses: {}", self.estimated_clauses)?;
        writeln!(f, "  Grid size: {} cells", self.grid_size)?;
        writeln!(f, "  Time steps: {}", self.time_steps)?;
        writeln!(
            f,
            "  Living cells ratio: {:.2}%",
            self.living_cells_ratio * 100.0
        )?;

        let recommendation = match self.complexity_level {
            ComplexityLevel::Low => "Should solve quickly",
            ComplexityLevel::Medium => "May take some time to solve",
            ComplexityLevel::High => "Likely to be challenging, consider reducing problem size",
            ComplexityLevel::VeryHigh => {
                "Very challenging, strongly consider reducing problem size or using approximations"
            }
        };
        writeln!(f, "  Recommendation: {}", recommendation)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::game_of_life::Grid;
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
    fn test_encoder_creation() {
        let settings = create_test_settings();
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let encoder = SatEncoder::new(settings, &target_grid).unwrap();

        let stats = encoder.statistics();
        assert_eq!(stats.grid_width, 3);
        assert_eq!(stats.grid_height, 3);
        assert_eq!(stats.generations, 1);
    }

    #[test]
    fn test_complexity_estimation() {
        let settings = create_test_settings();
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let encoder = SatEncoder::new(settings, &target_grid).unwrap();

        let estimate = encoder.estimate_complexity(&target_grid);
        assert_eq!(estimate.grid_size, 9);
        assert_eq!(estimate.time_steps, 2);
        assert!(estimate.living_cells_ratio > 0.0);
    }

    #[test]
    fn test_grid_extraction() {
        let settings = create_test_settings();
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let mut encoder = SatEncoder::new(settings, &target_grid).unwrap();

        // Create a mock solution
        let mut assignment = std::collections::HashMap::new();

        // Set up a simple pattern: only cell (1,1) at time 0 is alive
        for y in 0..3 {
            for x in 0..3 {
                let var = encoder
                    .constraint_generator
                    .variable_manager()
                    .cell_variable(x, y, 0)
                    .unwrap();
                assignment.insert(var, x == 1 && y == 1);
            }
        }

        let solution = SolverSolution {
            assignment,
            solve_time: Duration::from_millis(100),
        };

        let grid = encoder.extract_grid_from_solution(&solution, 0).unwrap();
        assert_eq!(grid.living_count(), 1);
        assert!(grid.get(1, 1)); // Center cell should be alive
        assert!(!grid.get(0, 0)); // Corner should be dead
    }
}
