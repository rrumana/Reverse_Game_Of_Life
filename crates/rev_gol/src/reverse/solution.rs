//! Solution representation for reverse Game of Life problems

use crate::game_of_life::Grid;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Represents a solution to a reverse Game of Life problem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Solution {
    /// The predecessor state (initial state)
    pub predecessor: Grid,
    /// The target state (final state)
    pub target: Grid,
    /// Number of generations between predecessor and target
    pub generations: usize,
    /// Complete evolution path from predecessor to target
    pub evolution_path: Vec<Grid>,
    /// Time taken to find this solution
    #[serde(skip)]
    pub solve_time: Duration,
    /// Metadata about the solution
    pub metadata: SolutionMetadata,
}

/// Metadata about a solution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolutionMetadata {
    /// Unique identifier for this solution
    pub id: String,
    /// Number of living cells in the predecessor
    pub predecessor_living_cells: usize,
    /// Number of living cells in the target
    pub target_living_cells: usize,
    /// Density of living cells in predecessor (0.0 to 1.0)
    pub predecessor_density: f64,
    /// Whether this solution contains known patterns
    pub contains_known_patterns: bool,
    /// Stability analysis of the predecessor
    pub stability: StabilityAnalysis,
    /// Quality score of the solution (0.0 to 1.0, higher is better)
    pub quality_score: f64,
}

/// Analysis of solution stability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityAnalysis {
    /// Whether the predecessor is a still life
    pub is_still_life: bool,
    /// Whether the predecessor is an oscillator
    pub is_oscillator: bool,
    /// Period of oscillation (if oscillator)
    pub oscillation_period: Option<usize>,
    /// Whether the predecessor contains moving patterns
    pub has_moving_patterns: bool,
    /// Estimated stability score (0.0 to 1.0)
    pub stability_score: f64,
}

impl Solution {
    /// Create a new solution
    pub fn new(
        predecessor: Grid,
        target: Grid,
        generations: usize,
        evolution_path: Vec<Grid>,
        solve_time: Duration,
    ) -> Self {
        let metadata = SolutionMetadata::analyze(&predecessor, &target, &evolution_path);

        Self {
            predecessor,
            target,
            generations,
            evolution_path,
            solve_time,
            metadata,
        }
    }

    /// Get the initial state (predecessor)
    pub fn initial_state(&self) -> &Grid {
        &self.predecessor
    }

    /// Get the final state (target)
    pub fn final_state(&self) -> &Grid {
        &self.target
    }

    /// Get a specific state in the evolution path
    pub fn state_at_generation(&self, generation: usize) -> Option<&Grid> {
        self.evolution_path.get(generation)
    }

    /// Get the complete evolution path
    pub fn evolution_path(&self) -> &[Grid] {
        &self.evolution_path
    }

    /// Check if this solution is equivalent to another (same predecessor)
    pub fn is_equivalent_to(&self, other: &Solution) -> bool {
        self.predecessor == other.predecessor
    }

    /// Get a summary of the solution
    pub fn summary(&self) -> SolutionSummary {
        SolutionSummary {
            id: self.metadata.id.clone(),
            predecessor_living_cells: self.metadata.predecessor_living_cells,
            target_living_cells: self.metadata.target_living_cells,
            generations: self.generations,
            quality_score: self.metadata.quality_score,
            solve_time_ms: self.solve_time.as_millis() as u64,
            is_still_life: self.metadata.stability.is_still_life,
            is_oscillator: self.metadata.stability.is_oscillator,
        }
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Create from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Save to file
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> anyhow::Result<()> {
        let json = self.to_json()?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load from file
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::from_json(&content)?)
    }

    /// Get visual representation of the evolution
    pub fn format_evolution(&self) -> String {
        let mut result = String::new();

        result.push_str(&format!(
            "Solution {} - {} generations\n",
            self.metadata.id, self.generations
        ));
        result.push_str(&format!(
            "Quality: {:.2}, Solve time: {:.3}s\n\n",
            self.metadata.quality_score,
            self.solve_time.as_secs_f64()
        ));

        for (i, grid) in self.evolution_path.iter().enumerate() {
            result.push_str(&format!("Generation {}:\n", i));
            result.push_str(&grid.to_string());
            result.push('\n');
        }

        result
    }

    /// Compare quality with another solution
    pub fn is_better_than(&self, other: &Solution) -> bool {
        self.metadata.quality_score > other.metadata.quality_score
    }
}

impl SolutionMetadata {
    /// Analyze a solution and create metadata
    pub fn analyze(predecessor: &Grid, target: &Grid, evolution_path: &[Grid]) -> Self {
        let id = Self::generate_id(predecessor);
        let predecessor_living_cells = predecessor.living_count();
        let target_living_cells = target.living_count();
        let total_cells = predecessor.width * predecessor.height;
        let predecessor_density = predecessor_living_cells as f64 / total_cells as f64;

        let contains_known_patterns = Self::detect_known_patterns(predecessor);
        let stability = StabilityAnalysis::analyze(evolution_path);
        let quality_score =
            Self::calculate_quality_score(predecessor, target, &stability, contains_known_patterns);

        Self {
            id,
            predecessor_living_cells,
            target_living_cells,
            predecessor_density,
            contains_known_patterns,
            stability,
            quality_score,
        }
    }

    /// Generate a unique ID for the solution based on predecessor state
    fn generate_id(predecessor: &Grid) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        predecessor.cells.hash(&mut hasher);
        predecessor.width.hash(&mut hasher);
        predecessor.height.hash(&mut hasher);

        format!("sol_{:x}", hasher.finish())
    }

    /// Detect known Game of Life patterns
    fn detect_known_patterns(grid: &Grid) -> bool {
        let living_count = grid.living_count();

        // Common still lifes
        if living_count == 4 {
            return Self::is_block_pattern(grid) || Self::is_beehive_pattern(grid);
        }

        // Common oscillators
        if living_count == 3 {
            return Self::is_blinker_pattern(grid);
        }

        // Glider
        if living_count == 5 {
            return Self::is_glider_pattern(grid);
        }

        false
    }

    /// Check if grid contains a block pattern (2x2 square)
    fn is_block_pattern(grid: &Grid) -> bool {
        for y in 0..grid.height.saturating_sub(1) {
            for x in 0..grid.width.saturating_sub(1) {
                if grid.get(y, x)
                    && grid.get(y, x + 1)
                    && grid.get(y + 1, x)
                    && grid.get(y + 1, x + 1)
                {
                    // Check if only these 4 cells are alive
                    if grid.living_count() == 4 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if grid contains a blinker pattern
    fn is_blinker_pattern(grid: &Grid) -> bool {
        let living_cells = grid.living_cells();
        if living_cells.len() != 3 {
            return false;
        }

        // Check for horizontal or vertical line
        let rows: Vec<_> = living_cells.iter().map(|(r, _)| *r).collect();
        let cols: Vec<_> = living_cells.iter().map(|(_, c)| *c).collect();

        // All same row (horizontal)
        if rows.iter().all(|&r| r == rows[0]) {
            let mut sorted_cols = cols;
            sorted_cols.sort();
            return sorted_cols[1] == sorted_cols[0] + 1 && sorted_cols[2] == sorted_cols[1] + 1;
        }

        // All same column (vertical)
        if cols.iter().all(|&c| c == cols[0]) {
            let mut sorted_rows = rows;
            sorted_rows.sort();
            return sorted_rows[1] == sorted_rows[0] + 1 && sorted_rows[2] == sorted_rows[1] + 1;
        }

        false
    }

    /// Check if grid contains a beehive pattern
    fn is_beehive_pattern(_grid: &Grid) -> bool {
        // Simplified - would need more complex pattern matching
        false
    }

    /// Check if grid contains a glider pattern
    fn is_glider_pattern(_grid: &Grid) -> bool {
        // Simplified - would need to check all orientations
        false
    }

    /// Calculate quality score for a solution
    fn calculate_quality_score(
        predecessor: &Grid,
        _target: &Grid,
        stability: &StabilityAnalysis,
        contains_known_patterns: bool,
    ) -> f64 {
        let mut score = 0.5; // Base score

        // Prefer solutions with known patterns
        if contains_known_patterns {
            score += 0.2;
        }

        // Prefer stable patterns
        score += stability.stability_score * 0.3;

        // Prefer simpler patterns (fewer living cells)
        let density =
            predecessor.living_count() as f64 / (predecessor.width * predecessor.height) as f64;
        if density < 0.3 {
            score += 0.1;
        }

        // Prefer still lifes and oscillators
        if stability.is_still_life {
            score += 0.2;
        } else if stability.is_oscillator {
            score += 0.1;
        }

        score.min(1.0)
    }
}

impl StabilityAnalysis {
    /// Analyze the stability of an evolution path
    pub fn analyze(evolution_path: &[Grid]) -> Self {
        if evolution_path.len() < 2 {
            return Self::default();
        }

        let is_still_life = Self::check_still_life(evolution_path);
        let (is_oscillator, oscillation_period) = Self::check_oscillator(evolution_path);
        let has_moving_patterns = Self::check_moving_patterns(evolution_path);

        let stability_score =
            Self::calculate_stability_score(is_still_life, is_oscillator, has_moving_patterns);

        Self {
            is_still_life,
            is_oscillator,
            oscillation_period,
            has_moving_patterns,
            stability_score,
        }
    }

    /// Check if the pattern is a still life
    fn check_still_life(evolution_path: &[Grid]) -> bool {
        if evolution_path.len() < 2 {
            return false;
        }

        // Check if first and second states are identical
        evolution_path[0] == evolution_path[1]
    }

    /// Check if the pattern is an oscillator and find its period
    fn check_oscillator(evolution_path: &[Grid]) -> (bool, Option<usize>) {
        if evolution_path.len() < 3 {
            return (false, None);
        }

        // Check for periods 2-8
        for period in 2..=8.min(evolution_path.len() - 1) {
            if evolution_path[0] == evolution_path[period] {
                // Verify the period by checking more cycles if possible
                let mut is_periodic = true;
                for i in 1..period {
                    if i + period < evolution_path.len() {
                        if evolution_path[i] != evolution_path[i + period] {
                            is_periodic = false;
                            break;
                        }
                    }
                }
                if is_periodic {
                    return (true, Some(period));
                }
            }
        }

        (false, None)
    }

    /// Check if the pattern has moving components
    fn check_moving_patterns(evolution_path: &[Grid]) -> bool {
        if evolution_path.len() < 2 {
            return false;
        }

        // Simple heuristic: if living cells change position significantly
        for i in 1..evolution_path.len() {
            let prev_cells = evolution_path[i - 1].living_cells();
            let curr_cells = evolution_path[i].living_cells();

            if prev_cells.len() == curr_cells.len() && !prev_cells.is_empty() {
                // Check if the pattern has shifted
                let prev_center = Self::calculate_center_of_mass(&prev_cells);
                let curr_center = Self::calculate_center_of_mass(&curr_cells);

                let distance = ((prev_center.0 - curr_center.0).powi(2)
                    + (prev_center.1 - curr_center.1).powi(2))
                .sqrt();

                if distance > 0.5 {
                    return true;
                }
            }
        }

        false
    }

    /// Calculate center of mass of living cells
    fn calculate_center_of_mass(cells: &[(usize, usize)]) -> (f64, f64) {
        if cells.is_empty() {
            return (0.0, 0.0);
        }

        let sum_x: usize = cells.iter().map(|(_, x)| x).sum();
        let sum_y: usize = cells.iter().map(|(y, _)| y).sum();

        (
            sum_x as f64 / cells.len() as f64,
            sum_y as f64 / cells.len() as f64,
        )
    }

    /// Calculate stability score
    fn calculate_stability_score(
        is_still_life: bool,
        is_oscillator: bool,
        has_moving_patterns: bool,
    ) -> f64 {
        if is_still_life {
            1.0
        } else if is_oscillator {
            0.8
        } else if has_moving_patterns {
            0.3
        } else {
            0.5
        }
    }
}

impl Default for StabilityAnalysis {
    fn default() -> Self {
        Self {
            is_still_life: false,
            is_oscillator: false,
            oscillation_period: None,
            has_moving_patterns: false,
            stability_score: 0.0,
        }
    }
}

/// Summary of a solution for display purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolutionSummary {
    pub id: String,
    pub predecessor_living_cells: usize,
    pub target_living_cells: usize,
    pub generations: usize,
    pub quality_score: f64,
    pub solve_time_ms: u64,
    pub is_still_life: bool,
    pub is_oscillator: bool,
}

impl std::fmt::Display for SolutionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Solution {}: {} → {} cells, {} gen, quality {:.2}, {}ms",
            self.id,
            self.predecessor_living_cells,
            self.target_living_cells,
            self.generations,
            self.quality_score,
            self.solve_time_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BoundaryCondition;

    #[test]
    fn test_solution_creation() {
        let cells = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let predecessor = Grid::from_cells(cells.clone(), BoundaryCondition::Dead).unwrap();
        let target = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let evolution_path = vec![predecessor.clone(), target.clone()];

        let solution = Solution::new(
            predecessor,
            target,
            1,
            evolution_path,
            Duration::from_millis(100),
        );

        assert_eq!(solution.generations, 1);
        // Pattern has 4 living cells: (0,1), (1,0), (1,2), (2,1)
        assert_eq!(solution.metadata.predecessor_living_cells, 4);
        assert!(!solution.metadata.id.is_empty());
    }

    #[test]
    fn test_blinker_detection() {
        let cells = vec![
            vec![false, false, false],
            vec![true, true, true],
            vec![false, false, false],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        assert!(SolutionMetadata::is_blinker_pattern(&grid));
    }

    #[test]
    fn test_block_detection() {
        let cells = vec![
            vec![false, false, false, false],
            vec![false, true, true, false],
            vec![false, true, true, false],
            vec![false, false, false, false],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        assert!(SolutionMetadata::is_block_pattern(&grid));
    }

    #[test]
    fn test_still_life_detection() {
        let cells = vec![vec![true, false], vec![false, true]];
        let grid1 = Grid::from_cells(cells.clone(), BoundaryCondition::Dead).unwrap();
        let grid2 = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let evolution_path = vec![grid1, grid2];

        assert!(StabilityAnalysis::check_still_life(&evolution_path));
    }

    #[test]
    fn test_solution_comparison() {
        let cells = vec![vec![true]];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();
        let evolution_path = vec![grid.clone()];

        let solution1 = Solution::new(
            grid.clone(),
            grid.clone(),
            1,
            evolution_path.clone(),
            Duration::from_millis(100),
        );

        let solution2 = Solution::new(
            grid.clone(),
            grid,
            1,
            evolution_path,
            Duration::from_millis(200),
        );

        assert!(solution1.is_equivalent_to(&solution2));
    }
}
