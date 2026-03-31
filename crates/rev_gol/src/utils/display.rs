//! Display and output formatting utilities

use crate::config::OutputFormat;
use crate::game_of_life::Grid;
use crate::reverse::Solution;
use anyhow::Result;
use std::path::Path;

/// Format solutions for display
pub struct SolutionFormatter;

impl SolutionFormatter {
    /// Format a single solution for console output
    pub fn format_solution(solution: &Solution, show_evolution: bool) -> String {
        let mut output = String::new();

        output.push_str(&format!("=== Solution {} ===\n", solution.metadata.id));
        output.push_str(&format!(
            "Quality Score: {:.2}\n",
            solution.metadata.quality_score
        ));
        output.push_str(&format!(
            "Solve Time: {:.3}s\n",
            solution.solve_time.as_secs_f64()
        ));
        output.push_str(&format!("Generations: {}\n", solution.generations));
        output.push_str(&format!(
            "Living Cells: {} → {}\n",
            solution.metadata.predecessor_living_cells, solution.metadata.target_living_cells
        ));

        if solution.metadata.stability.is_still_life {
            output.push_str("Type: Still Life\n");
        } else if solution.metadata.stability.is_oscillator {
            output.push_str(&format!(
                "Type: Oscillator (period {})\n",
                solution.metadata.stability.oscillation_period.unwrap_or(0)
            ));
        } else if solution.metadata.stability.has_moving_patterns {
            output.push_str("Type: Moving Pattern\n");
        } else {
            output.push_str("Type: Other\n");
        }

        output.push('\n');

        if show_evolution {
            output.push_str("Evolution:\n");
            for (i, grid) in solution.evolution_path.iter().enumerate() {
                output.push_str(&format!("Generation {}:\n", i));
                output.push_str(&Self::format_grid_compact(grid));
                output.push('\n');
            }
        } else {
            output.push_str("Initial State:\n");
            output.push_str(&Self::format_grid_compact(&solution.predecessor));
            output.push('\n');
            output.push_str(&format!(
                "Final State (after {} generations):\n",
                solution.generations
            ));
            output.push_str(&Self::format_grid_compact(&solution.target));
        }

        output
    }

    /// Format a single solution for file output (using 1s and 0s)
    pub fn format_solution_for_file(solution: &Solution, show_evolution: bool) -> String {
        let mut output = String::new();

        output.push_str(&format!("=== Solution {} ===\n", solution.metadata.id));
        output.push_str(&format!(
            "Quality Score: {:.2}\n",
            solution.metadata.quality_score
        ));
        output.push_str(&format!(
            "Solve Time: {:.3}s\n",
            solution.solve_time.as_secs_f64()
        ));
        output.push_str(&format!("Generations: {}\n", solution.generations));
        output.push_str(&format!(
            "Living Cells: {} → {}\n",
            solution.metadata.predecessor_living_cells, solution.metadata.target_living_cells
        ));

        if solution.metadata.stability.is_still_life {
            output.push_str("Type: Still Life\n");
        } else if solution.metadata.stability.is_oscillator {
            output.push_str(&format!(
                "Type: Oscillator (period {})\n",
                solution.metadata.stability.oscillation_period.unwrap_or(0)
            ));
        } else if solution.metadata.stability.has_moving_patterns {
            output.push_str("Type: Moving Pattern\n");
        } else {
            output.push_str("Type: Other\n");
        }

        output.push('\n');

        if show_evolution {
            output.push_str("Evolution:\n");
            for (i, grid) in solution.evolution_path.iter().enumerate() {
                output.push_str(&format!("Generation {}:\n", i));
                output.push_str(&Self::format_grid_binary(grid));
                output.push('\n');
            }
        } else {
            output.push_str("Initial State:\n");
            output.push_str(&Self::format_grid_binary(&solution.predecessor));
            output.push('\n');
            output.push_str(&format!(
                "Final State (after {} generations):\n",
                solution.generations
            ));
            output.push_str(&Self::format_grid_binary(&solution.target));
        }

        output
    }

    /// Format multiple solutions as a summary table
    pub fn format_solution_summary(solutions: &[Solution]) -> String {
        let mut output = String::new();

        output.push_str("Solutions Summary:\n");
        output.push_str("ID       | Quality | Time(ms) | Living | Type\n");
        output.push_str("---------|---------|----------|--------|----------\n");

        for solution in solutions {
            let solution_type = if solution.metadata.stability.is_still_life {
                "Still"
            } else if solution.metadata.stability.is_oscillator {
                "Osc"
            } else if solution.metadata.stability.has_moving_patterns {
                "Moving"
            } else {
                "Other"
            };

            output.push_str(&format!(
                "{:8} | {:7.2} | {:8} | {:6} | {}\n",
                &solution.metadata.id[..8.min(solution.metadata.id.len())],
                solution.metadata.quality_score,
                solution.solve_time.as_millis(),
                solution.metadata.predecessor_living_cells,
                solution_type
            ));
        }

        output
    }

    /// Format a grid in compact form (for console display)
    pub fn format_grid_compact(grid: &Grid) -> String {
        let mut output = String::new();
        for y in 0..grid.height {
            for x in 0..grid.width {
                output.push(if grid.get(y, x) { '█' } else { '·' });
            }
            output.push('\n');
        }
        output
    }

    /// Format a grid with 1s and 0s (for file output)
    pub fn format_grid_binary(grid: &Grid) -> String {
        let mut output = String::new();
        for y in 0..grid.height {
            for x in 0..grid.width {
                output.push(if grid.get(y, x) { '1' } else { '0' });
            }
            output.push('\n');
        }
        output
    }

    /// Format a grid with coordinates
    pub fn format_grid_with_coords(grid: &Grid) -> String {
        let mut output = String::new();

        // Header with column numbers
        output.push_str("   ");
        for x in 0..grid.width {
            if x == 0 {
                output.push_str(&format!("{}", x % 10));
            } else {
                output.push_str(&format!("  {}", x % 10));
            }
        }
        output.push('\n');

        // Rows with row numbers
        for y in 0..grid.height {
            output.push_str(&format!("{:2} ", y));
            for x in 0..grid.width {
                output.push_str(if grid.get(y, x) { "██" } else { "··" });
            }
            output.push('\n');
        }

        output
    }

    /// Save solutions to files based on output format
    pub fn save_solutions<P: AsRef<Path>>(
        solutions: &[Solution],
        output_dir: P,
        format: &OutputFormat,
    ) -> Result<()> {
        let output_dir = output_dir.as_ref();
        std::fs::create_dir_all(output_dir)?;

        match format {
            OutputFormat::Text => {
                for (i, solution) in solutions.iter().enumerate() {
                    let filename = format!("solution_{:03}.txt", i + 1);
                    let filepath = output_dir.join(filename);
                    let content = Self::format_solution_for_file(solution, true);
                    std::fs::write(filepath, content)?;
                }
            }
            OutputFormat::Json => {
                for (i, solution) in solutions.iter().enumerate() {
                    let filename = format!("solution_{:03}.json", i + 1);
                    let filepath = output_dir.join(filename);
                    solution.save_to_file(filepath)?;
                }

                // Also save a summary file
                let summary_path = output_dir.join("solutions_summary.json");
                let summaries: Vec<_> = solutions.iter().map(|s| s.summary()).collect();
                let summary_json = serde_json::to_string_pretty(&summaries)?;
                std::fs::write(summary_path, summary_json)?;
            }
            OutputFormat::Visual => {
                // Create visual representations
                for (i, solution) in solutions.iter().enumerate() {
                    let filename = format!("solution_{:03}_visual.txt", i + 1);
                    let filepath = output_dir.join(filename);
                    let content = Self::create_visual_evolution(solution);
                    std::fs::write(filepath, content)?;
                }
            }
        }

        Ok(())
    }

    /// Create a visual representation of the evolution (for file output with 1s and 0s)
    fn create_visual_evolution(solution: &Solution) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "Visual Evolution - Solution {}\n",
            solution.metadata.id
        ));
        output.push_str(&"=".repeat(50));
        output.push('\n');

        for (i, grid) in solution.evolution_path.iter().enumerate() {
            output.push_str(&format!(
                "\nGeneration {} (Living: {}):\n",
                i,
                grid.living_count()
            ));
            output.push_str(&Self::format_grid_binary(grid));
        }

        output.push_str(&format!("\nSolution Statistics:\n"));
        output.push_str(&format!(
            "Quality Score: {:.2}\n",
            solution.metadata.quality_score
        ));
        output.push_str(&format!(
            "Solve Time: {:.3}s\n",
            solution.solve_time.as_secs_f64()
        ));
        output.push_str(&format!(
            "Stability Score: {:.2}\n",
            solution.metadata.stability.stability_score
        ));

        if solution.metadata.contains_known_patterns {
            output.push_str("Contains known patterns: Yes\n");
        }

        output
    }

    /// Create a side-by-side comparison of solutions
    pub fn compare_solutions(solutions: &[Solution]) -> String {
        if solutions.is_empty() {
            return "No solutions to compare".to_string();
        }

        let mut output = String::new();
        output.push_str("Solution Comparison:\n");
        output.push_str(&"=".repeat(80));
        output.push('\n');

        // Show initial states side by side
        output.push_str("Initial States:\n");
        let max_height = solutions
            .iter()
            .map(|s| s.predecessor.height)
            .max()
            .unwrap_or(0);

        for row in 0..max_height {
            for (i, solution) in solutions.iter().enumerate() {
                if i > 0 {
                    output.push_str("  |  ");
                }

                if row < solution.predecessor.height {
                    for x in 0..solution.predecessor.width {
                        output.push(if solution.predecessor.get(row, x) {
                            '█'
                        } else {
                            '·'
                        });
                    }
                } else {
                    output.push_str(&" ".repeat(solution.predecessor.width));
                }
            }
            output.push('\n');
        }

        // Show solution IDs
        output.push('\n');
        for (i, solution) in solutions.iter().enumerate() {
            if i > 0 {
                output.push_str("     ");
            }
            output.push_str(&format!(
                "{:8}",
                &solution.metadata.id[..8.min(solution.metadata.id.len())]
            ));
        }
        output.push('\n');

        // Show quality scores
        for (i, solution) in solutions.iter().enumerate() {
            if i > 0 {
                output.push_str("     ");
            }
            output.push_str(&format!("Q:{:5.2}", solution.metadata.quality_score));
        }
        output.push('\n');

        output
    }
}

/// Progress indicator for long-running operations
pub struct ProgressIndicator {
    total: usize,
    current: usize,
    last_update: std::time::Instant,
    start_time: std::time::Instant,
}

impl ProgressIndicator {
    /// Create a new progress indicator
    pub fn new(total: usize) -> Self {
        let now = std::time::Instant::now();
        Self {
            total,
            current: 0,
            last_update: now,
            start_time: now,
        }
    }

    /// Update progress and optionally display
    pub fn update(&mut self, current: usize) {
        self.current = current;
        let now = std::time::Instant::now();

        // Update display every 100ms
        if now.duration_since(self.last_update).as_millis() > 100 {
            self.display();
            self.last_update = now;
        }
    }

    /// Display current progress
    pub fn display(&self) {
        let percentage = if self.total > 0 {
            (self.current as f64 / self.total as f64) * 100.0
        } else {
            0.0
        };

        let elapsed = self.start_time.elapsed();
        let eta = if self.current > 0 {
            let rate = self.current as f64 / elapsed.as_secs_f64();
            let remaining = (self.total - self.current) as f64 / rate;
            format!("ETA: {:.1}s", remaining)
        } else {
            "ETA: --".to_string()
        };

        print!(
            "\rProgress: {}/{} ({:.1}%) - {}",
            self.current, self.total, percentage, eta
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    /// Finish and clear the progress line
    pub fn finish(&self) {
        println!(
            "\rCompleted: {}/{} (100.0%) - Total time: {:.1}s",
            self.total,
            self.total,
            self.start_time.elapsed().as_secs_f64()
        );
    }
}

/// Color output utilities
pub struct ColorOutput;

impl ColorOutput {
    /// Format text with color (if terminal supports it)
    pub fn colored(text: &str, color: Color) -> String {
        if Self::supports_color() {
            format!("\x1b[{}m{}\x1b[0m", color.code(), text)
        } else {
            text.to_string()
        }
    }

    /// Check if terminal supports color
    fn supports_color() -> bool {
        std::env::var("NO_COLOR").is_err() && (std::env::var("TERM").unwrap_or_default() != "dumb")
    }

    /// Format success message
    pub fn success(text: &str) -> String {
        Self::colored(text, Color::Green)
    }

    /// Format error message
    pub fn error(text: &str) -> String {
        Self::colored(text, Color::Red)
    }

    /// Format warning message
    pub fn warning(text: &str) -> String {
        Self::colored(text, Color::Yellow)
    }

    /// Format info message
    pub fn info(text: &str) -> String {
        Self::colored(text, Color::Blue)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

impl Color {
    fn code(self) -> u8 {
        match self {
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BoundaryCondition;

    #[test]
    fn test_grid_formatting() {
        let cells = vec![
            vec![true, false, true],
            vec![false, true, false],
            vec![true, false, true],
        ];
        let grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        let compact = SolutionFormatter::format_grid_compact(&grid);
        assert!(compact.contains('█'));
        assert!(compact.contains('·'));

        let with_coords = SolutionFormatter::format_grid_with_coords(&grid);
        assert!(with_coords.contains("0  1  2"));
    }

    #[test]
    fn test_progress_indicator() {
        let mut progress = ProgressIndicator::new(100);
        progress.update(50);
        assert_eq!(progress.current, 50);
        assert_eq!(progress.total, 100);
    }

    #[test]
    fn test_color_output() {
        let colored = ColorOutput::colored("test", Color::Red);
        // Should either be colored or plain text
        assert!(colored.contains("test"));

        let success = ColorOutput::success("OK");
        assert!(success.contains("OK"));
    }
}
