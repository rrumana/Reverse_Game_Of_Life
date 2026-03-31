//! Solution validation for reverse Game of Life problems

use crate::config::Settings;
use crate::game_of_life::{GameOfLifeRules, Grid};
use anyhow::Result;

/// Validates solutions to reverse Game of Life problems
pub struct SolutionValidator {
    settings: Settings,
}

/// Result of solution validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub evolution_path: Vec<Grid>,
    pub error_message: Option<String>,
    pub validation_details: ValidationDetails,
}

/// Detailed validation information
#[derive(Debug, Clone)]
pub struct ValidationDetails {
    pub generations_checked: usize,
    pub intermediate_states_valid: bool,
    pub final_state_matches: bool,
    pub rule_violations: Vec<RuleViolation>,
    pub performance_metrics: ValidationMetrics,
}

/// Represents a rule violation found during validation
#[derive(Debug, Clone)]
pub struct RuleViolation {
    pub generation: usize,
    pub cell_position: (usize, usize),
    pub expected_state: bool,
    pub actual_state: bool,
    pub neighbor_count: u8,
    pub description: String,
}

/// Performance metrics for validation
#[derive(Debug, Clone)]
pub struct ValidationMetrics {
    pub validation_time_ms: u64,
    pub states_validated: usize,
    pub cells_checked: usize,
}

impl SolutionValidator {
    /// Create a new solution validator
    pub fn new(settings: Settings) -> Self {
        Self { settings }
    }

    /// Validate that a predecessor correctly evolves to the target
    pub fn validate(&self, predecessor: &Grid, target: &Grid) -> Result<ValidationResult> {
        let start_time = std::time::Instant::now();

        // Check grid dimensions
        if predecessor.width != target.width || predecessor.height != target.height {
            return Ok(ValidationResult {
                is_valid: false,
                evolution_path: vec![],
                error_message: Some(format!(
                    "Grid dimension mismatch: predecessor {}x{}, target {}x{}",
                    predecessor.width, predecessor.height, target.width, target.height
                )),
                validation_details: ValidationDetails::default(),
            });
        }

        // Check boundary conditions match
        if std::mem::discriminant(&predecessor.boundary_condition)
            != std::mem::discriminant(&target.boundary_condition)
        {
            return Ok(ValidationResult {
                is_valid: false,
                evolution_path: vec![],
                error_message: Some(
                    "Boundary condition mismatch between predecessor and target".to_string(),
                ),
                validation_details: ValidationDetails::default(),
            });
        }

        // Evolve the predecessor and track the path
        let mut evolution_path = vec![predecessor.clone()];
        let mut current_grid = predecessor.clone();
        let mut rule_violations = Vec::new();

        for generation in 0..self.settings.simulation.generations {
            let next_grid = GameOfLifeRules::evolve(&current_grid);
            evolution_path.push(next_grid.clone());

            // Validate each transition follows Game of Life rules
            let violations = self.validate_transition(&current_grid, &next_grid, generation);
            rule_violations.extend(violations);

            current_grid = next_grid;
        }

        let final_state_matches = GameOfLifeRules::grids_equal(&current_grid, target);
        let intermediate_states_valid = rule_violations.is_empty();
        let is_valid = final_state_matches && intermediate_states_valid;

        let validation_time = start_time.elapsed();
        let cells_checked = evolution_path.len() * predecessor.width * predecessor.height;

        let validation_details = ValidationDetails {
            generations_checked: self.settings.simulation.generations,
            intermediate_states_valid,
            final_state_matches,
            rule_violations,
            performance_metrics: ValidationMetrics {
                validation_time_ms: validation_time.as_millis() as u64,
                states_validated: evolution_path.len(),
                cells_checked,
            },
        };

        let error_message = if !is_valid {
            Some(self.generate_error_message(&validation_details))
        } else {
            None
        };

        Ok(ValidationResult {
            is_valid,
            evolution_path,
            error_message,
            validation_details,
        })
    }

    /// Validate a single transition between two grid states
    fn validate_transition(
        &self,
        current: &Grid,
        next: &Grid,
        generation: usize,
    ) -> Vec<RuleViolation> {
        let mut violations = Vec::new();

        for y in 0..current.height {
            for x in 0..current.width {
                let current_cell = current.get(y, x);
                let next_cell = next.get(y, x);
                let neighbor_count = current.count_neighbors(y, x);

                let expected_next = GameOfLifeRules::should_be_alive(current_cell, neighbor_count);

                if next_cell != expected_next {
                    violations.push(RuleViolation {
                        generation,
                        cell_position: (y, x),
                        expected_state: expected_next,
                        actual_state: next_cell,
                        neighbor_count,
                        description: format!(
                            "Cell ({}, {}) at generation {} should be {} but is {} (current: {}, neighbors: {})",
                            y, x, generation + 1,
                            if expected_next { "alive" } else { "dead" },
                            if next_cell { "alive" } else { "dead" },
                            if current_cell { "alive" } else { "dead" },
                            neighbor_count
                        ),
                    });
                }
            }
        }

        violations
    }

    /// Generate a descriptive error message from validation details
    fn generate_error_message(&self, details: &ValidationDetails) -> String {
        let mut message = String::new();

        if !details.final_state_matches {
            message.push_str("Final state does not match target. ");
        }

        if !details.intermediate_states_valid {
            message.push_str(&format!(
                "Found {} rule violations during evolution. ",
                details.rule_violations.len()
            ));

            // Include details of first few violations
            for (i, violation) in details.rule_violations.iter().take(3).enumerate() {
                if i == 0 {
                    message.push_str("Examples: ");
                }
                message.push_str(&format!("{}; ", violation.description));
            }

            if details.rule_violations.len() > 3 {
                message.push_str(&format!(
                    "... and {} more",
                    details.rule_violations.len() - 3
                ));
            }
        }

        message
    }

    /// Validate multiple solutions and return statistics
    pub fn validate_multiple(&self, solutions: &[(Grid, Grid)]) -> Result<MultiValidationResult> {
        let mut results = Vec::new();
        let mut valid_count = 0;
        let mut total_violations = 0;

        for (i, (predecessor, target)) in solutions.iter().enumerate() {
            match self.validate(predecessor, target) {
                Ok(result) => {
                    if result.is_valid {
                        valid_count += 1;
                    }
                    total_violations += result.validation_details.rule_violations.len();
                    results.push((i, result));
                }
                Err(e) => {
                    eprintln!("Error validating solution {}: {}", i, e);
                }
            }
        }

        Ok(MultiValidationResult {
            total_solutions: solutions.len(),
            valid_solutions: valid_count,
            invalid_solutions: solutions.len() - valid_count,
            total_rule_violations: total_violations,
            individual_results: results,
        })
    }

    /// Quick validation that only checks the final state
    pub fn quick_validate(&self, predecessor: &Grid, target: &Grid) -> Result<bool> {
        let evolved = GameOfLifeRules::evolve_generations(
            predecessor.clone(),
            self.settings.simulation.generations,
        );
        Ok(GameOfLifeRules::grids_equal(&evolved, target))
    }

    /// Validate that a grid is a valid Game of Life state
    pub fn validate_grid_state(&self, grid: &Grid) -> GridValidationResult {
        let mut issues = Vec::new();

        // Check for basic consistency
        if grid.cells.len() != grid.width * grid.height {
            issues.push("Grid cell count doesn't match dimensions".to_string());
        }

        // Check for reasonable density (heuristic)
        let density = grid.living_count() as f64 / grid.cells.len() as f64;
        if density > 0.9 {
            issues.push("Grid density is very high (>90%), which is unusual".to_string());
        }

        // Check for isolated patterns that might indicate errors
        let isolated_cells = self.count_isolated_cells(grid);
        if isolated_cells > grid.living_count() / 2 {
            issues.push("Many isolated cells detected, which is unusual".to_string());
        }

        GridValidationResult {
            is_valid: issues.is_empty(),
            issues,
            living_cells: grid.living_count(),
            density,
            isolated_cells,
        }
    }

    /// Count cells that have no living neighbors
    fn count_isolated_cells(&self, grid: &Grid) -> usize {
        let mut isolated = 0;
        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(y, x) && grid.count_neighbors(y, x) == 0 {
                    isolated += 1;
                }
            }
        }
        isolated
    }
}

/// Result of validating multiple solutions
#[derive(Debug, Clone)]
pub struct MultiValidationResult {
    pub total_solutions: usize,
    pub valid_solutions: usize,
    pub invalid_solutions: usize,
    pub total_rule_violations: usize,
    pub individual_results: Vec<(usize, ValidationResult)>,
}

/// Result of validating a grid state
#[derive(Debug, Clone)]
pub struct GridValidationResult {
    pub is_valid: bool,
    pub issues: Vec<String>,
    pub living_cells: usize,
    pub density: f64,
    pub isolated_cells: usize,
}

impl Default for ValidationDetails {
    fn default() -> Self {
        Self {
            generations_checked: 0,
            intermediate_states_valid: false,
            final_state_matches: false,
            rule_violations: Vec::new(),
            performance_metrics: ValidationMetrics {
                validation_time_ms: 0,
                states_validated: 0,
                cells_checked: 0,
            },
        }
    }
}

impl std::fmt::Display for ValidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Validation Result: {}",
            if self.is_valid { "VALID" } else { "INVALID" }
        )?;

        if let Some(ref error) = self.error_message {
            writeln!(f, "Error: {}", error)?;
        }

        let details = &self.validation_details;
        writeln!(f, "Generations checked: {}", details.generations_checked)?;
        writeln!(f, "Final state matches: {}", details.final_state_matches)?;
        writeln!(
            f,
            "Intermediate states valid: {}",
            details.intermediate_states_valid
        )?;
        writeln!(f, "Rule violations: {}", details.rule_violations.len())?;
        writeln!(
            f,
            "Validation time: {}ms",
            details.performance_metrics.validation_time_ms
        )?;

        Ok(())
    }
}

impl std::fmt::Display for MultiValidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Multi-Solution Validation Results:")?;
        writeln!(f, "  Total solutions: {}", self.total_solutions)?;
        writeln!(f, "  Valid solutions: {}", self.valid_solutions)?;
        writeln!(f, "  Invalid solutions: {}", self.invalid_solutions)?;
        writeln!(
            f,
            "  Success rate: {:.1}%",
            (self.valid_solutions as f64 / self.total_solutions as f64) * 100.0
        )?;
        writeln!(f, "  Total rule violations: {}", self.total_rule_violations)?;
        Ok(())
    }
}

impl std::fmt::Display for GridValidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Grid Validation: {}",
            if self.is_valid {
                "VALID"
            } else {
                "ISSUES FOUND"
            }
        )?;
        writeln!(f, "  Living cells: {}", self.living_cells)?;
        writeln!(f, "  Density: {:.1}%", self.density * 100.0)?;
        writeln!(f, "  Isolated cells: {}", self.isolated_cells)?;

        if !self.issues.is_empty() {
            writeln!(f, "  Issues:")?;
            for issue in &self.issues {
                writeln!(f, "    - {}", issue)?;
            }
        }

        Ok(())
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
    fn test_valid_blinker_evolution() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        // Vertical blinker -> horizontal blinker
        let predecessor_cells = vec![
            vec![false, true, false],
            vec![false, true, false],
            vec![false, true, false],
        ];
        let target_cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];

        let predecessor = Grid::from_cells(predecessor_cells, BoundaryCondition::Dead).unwrap();
        let target = Grid::from_cells(target_cells, BoundaryCondition::Dead).unwrap();

        let result = validator.validate(&predecessor, &target).unwrap();
        assert!(result.is_valid);
        assert!(result.validation_details.final_state_matches);
        assert!(result.validation_details.intermediate_states_valid);
        assert_eq!(result.validation_details.rule_violations.len(), 0);
    }

    #[test]
    fn test_invalid_evolution() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        // Empty grid cannot evolve to non-empty grid
        let predecessor = Grid::new(3, 3, BoundaryCondition::Dead);
        let target_cells = vec![
            vec![false, false, false],
            vec![false, true, false],
            vec![false, false, false],
        ];
        let target = Grid::from_cells(target_cells, BoundaryCondition::Dead).unwrap();

        let result = validator.validate(&predecessor, &target).unwrap();
        assert!(!result.is_valid);
        assert!(!result.validation_details.final_state_matches);
    }

    #[test]
    fn test_dimension_mismatch() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        let predecessor = Grid::new(3, 3, BoundaryCondition::Dead);
        let target = Grid::new(4, 4, BoundaryCondition::Dead);

        let result = validator.validate(&predecessor, &target).unwrap();
        assert!(!result.is_valid);
        assert!(result.error_message.is_some());
        assert!(result.error_message.unwrap().contains("dimension mismatch"));
    }

    #[test]
    fn test_quick_validation() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        let grid = Grid::new(3, 3, BoundaryCondition::Dead);
        let is_valid = validator.quick_validate(&grid, &grid).unwrap();
        assert!(is_valid); // Empty grid evolves to empty grid
    }

    #[test]
    fn test_grid_state_validation() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        // Normal grid
        let normal_cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let normal_grid = Grid::from_cells(normal_cells, BoundaryCondition::Dead).unwrap();
        let result = validator.validate_grid_state(&normal_grid);
        assert!(result.is_valid);

        // Grid with many isolated cells
        let isolated_cells = vec![
            vec![true, false, true],
            vec![false, false, false],
            vec![true, false, true],
        ];
        let isolated_grid = Grid::from_cells(isolated_cells, BoundaryCondition::Dead).unwrap();
        let result = validator.validate_grid_state(&isolated_grid);
        // Should still be valid but might have warnings
        assert_eq!(result.isolated_cells, 4);
    }

    #[test]
    fn test_rule_violation_detection() {
        let settings = create_test_settings();
        let validator = SolutionValidator::new(settings);

        // Create grids that violate Game of Life rules
        let current_cells = vec![
            vec![false, false, false],
            vec![false, true, false],
            vec![false, false, false],
        ];
        let next_cells = vec![
            vec![false, false, false],
            vec![false, true, false], // Should die (0 neighbors) but stays alive
            vec![false, false, false],
        ];

        let current = Grid::from_cells(current_cells, BoundaryCondition::Dead).unwrap();
        let next = Grid::from_cells(next_cells, BoundaryCondition::Dead).unwrap();

        let violations = validator.validate_transition(&current, &next, 0);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].cell_position, (1, 1));
        assert_eq!(violations[0].neighbor_count, 0);
        assert_eq!(violations[0].expected_state, false);
        assert_eq!(violations[0].actual_state, true);
    }
}
