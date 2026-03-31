//! Main CLI application for the reverse Game of Life solver

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rev_gol::{
    config::{CliOverrides, Settings},
    game_of_life::{create_example_grids, load_grid_from_file},
    reverse::ReverseProblem,
    utils::{resolve_workspace_path, ColorOutput, SolutionFormatter},
};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "rev_gol")]
#[command(about = "Reverse Game of Life SAT Solver")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Solve a reverse Game of Life problem
    Solve {
        /// Configuration file path
        #[arg(short, long, default_value = "config/default.yaml")]
        config: PathBuf,

        /// Target state file (overrides config)
        #[arg(short, long)]
        target: Option<PathBuf>,

        /// Number of generations (overrides config)
        #[arg(short, long)]
        generations: Option<usize>,

        /// Maximum solutions to find (overrides config)
        #[arg(short, long)]
        max_solutions: Option<usize>,

        /// Output directory (overrides config)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Show detailed evolution for each solution
        #[arg(long)]
        show_evolution: bool,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Create example configuration and input files
    Setup {
        /// Directory to create files in
        #[arg(short, long, default_value = ".")]
        directory: PathBuf,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
    },

    /// Validate a solution manually
    Validate {
        /// Configuration file path
        #[arg(short, long, default_value = "config/default.yaml")]
        config: PathBuf,

        /// Predecessor state file
        #[arg(short, long)]
        predecessor: PathBuf,

        /// Target state file
        #[arg(short, long)]
        target: PathBuf,

        /// Show evolution path
        #[arg(long)]
        show_evolution: bool,
    },

    /// Analyze a target state for solvability
    Analyze {
        /// Configuration file path
        #[arg(short, long, default_value = "config/default.yaml")]
        config: PathBuf,

        /// Target state file
        #[arg(short, long)]
        target: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Solve {
            config,
            target,
            generations,
            max_solutions,
            output,
            show_evolution,
            verbose,
        } => solve_command(
            config,
            target,
            generations,
            max_solutions,
            output,
            show_evolution,
            verbose,
        ),
        Commands::Setup { directory, force } => setup_command(directory, force),
        Commands::Validate {
            config,
            predecessor,
            target,
            show_evolution,
        } => validate_command(config, predecessor, target, show_evolution),
        Commands::Analyze { config, target } => analyze_command(config, target),
    }
}

fn solve_command(
    config_path: PathBuf,
    target_file: Option<PathBuf>,
    generations: Option<usize>,
    max_solutions: Option<usize>,
    output_dir: Option<PathBuf>,
    show_evolution: bool,
    verbose: bool,
) -> Result<()> {
    let config_path = resolve_workspace_path(config_path);
    println!(
        "{}",
        ColorOutput::info("🔄 Starting Reverse Game of Life Solver")
    );

    // Load configuration
    let mut settings = if config_path.exists() {
        Settings::from_file(&config_path)
            .with_context(|| format!("Failed to load config from {}", config_path.display()))?
    } else {
        println!(
            "{}",
            ColorOutput::warning(&format!(
                "Config file {} not found, using defaults",
                config_path.display()
            ))
        );
        let mut settings = Settings::default();
        settings.normalize_paths();
        settings
    };

    // Apply CLI overrides
    let cli_overrides = CliOverrides {
        generations,
        max_solutions,
        target_file: target_file.clone(),
        output_dir: output_dir.clone(),
    };
    settings.merge_with_cli(&cli_overrides);
    settings.normalize_paths();

    if verbose {
        println!("Configuration:");
        println!("  Generations: {}", settings.simulation.generations);
        println!("  Max solutions: {}", settings.solver.max_solutions);
        println!(
            "  Target file: {}",
            settings.input.target_state_file.display()
        );
        println!(
            "  Output dir: {}",
            settings.output.output_directory.display()
        );
        println!();
    }

    // Validate settings
    settings
        .validate()
        .context("Configuration validation failed")?;

    // Create and solve the problem
    let start_time = Instant::now();
    let mut problem =
        ReverseProblem::new(settings.clone()).context("Failed to create reverse problem")?;

    if verbose {
        let estimate = problem.estimate_solvability();
        println!("{}", estimate);
        println!();
    }

    println!(
        "{}",
        ColorOutput::info("🧮 Generating SAT constraints and solving...")
    );
    let solutions = problem.solve().context("Failed to solve reverse problem")?;

    let total_time = start_time.elapsed();

    if solutions.is_empty() {
        println!("{}", ColorOutput::warning("❌ No solutions found"));
        return Ok(());
    }

    println!(
        "{}",
        ColorOutput::success(&format!(
            "✅ Found {} solution(s) in {:.3}s",
            solutions.len(),
            total_time.as_secs_f64()
        ))
    );

    // Display solutions
    if show_evolution {
        for (i, solution) in solutions.iter().enumerate() {
            println!("\n{}", ColorOutput::info(&format!("Solution {}:", i + 1)));
            println!("{}", SolutionFormatter::format_solution(solution, true));
        }
    } else {
        println!(
            "\n{}",
            SolutionFormatter::format_solution_summary(&solutions)
        );

        if solutions.len() <= 3 {
            println!("\n{}", ColorOutput::info("Solution Details:"));
            for (i, solution) in solutions.iter().enumerate() {
                println!("\n{}", ColorOutput::info(&format!("Solution {}:", i + 1)));
                println!("{}", SolutionFormatter::format_solution(solution, false));
            }
        }
    }

    // Save solutions
    println!("\n{}", ColorOutput::info("💾 Saving solutions..."));
    SolutionFormatter::save_solutions(
        &solutions,
        &settings.output.output_directory,
        &settings.output.format,
    )
    .context("Failed to save solutions")?;

    println!(
        "{}",
        ColorOutput::success(&format!(
            "Solutions saved to {}",
            settings.output.output_directory.display()
        ))
    );

    // Show encoding statistics if verbose
    if verbose {
        println!("\n{}", problem.encoding_statistics());
    }

    Ok(())
}

fn setup_command(directory: PathBuf, force: bool) -> Result<()> {
    println!(
        "{}",
        ColorOutput::info("🛠️  Setting up project structure...")
    );

    // Create directories
    let config_dir = directory.join("config");
    let input_dir = directory.join("input/target_states");
    let output_dir = directory.join("output/solutions");

    for dir in [&config_dir, &input_dir, &output_dir] {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory {}", dir.display()))?;
    }

    // Create default configuration
    let config_path = config_dir.join("default.yaml");
    if !config_path.exists() || force {
        let default_settings = Settings::default();
        default_settings
            .to_file(&config_path)
            .context("Failed to create default configuration")?;
        println!("Created: {}", config_path.display());
    } else {
        println!("Skipped: {} (already exists)", config_path.display());
    }

    // Create example grids
    create_example_grids(&input_dir).context("Failed to create example grids")?;
    println!("Created example target states in: {}", input_dir.display());

    // Create example configuration variants
    let examples_dir = config_dir.join("examples");
    std::fs::create_dir_all(&examples_dir)?;

    // Simple configuration
    let mut simple_config = Settings::default();
    simple_config.simulation.generations = 1;
    simple_config.solver.max_solutions = 3;
    simple_config.input.target_state_file = PathBuf::from("input/target_states/blinker.txt");
    simple_config.to_file(&examples_dir.join("simple.yaml"))?;

    // Complex configuration
    let mut complex_config = Settings::default();
    complex_config.simulation.generations = 3;
    complex_config.solver.max_solutions = 10;
    complex_config.input.target_state_file = PathBuf::from("input/target_states/glider.txt");
    complex_config.to_file(&examples_dir.join("complex.yaml"))?;

    println!(
        "Created example configurations in: {}",
        examples_dir.display()
    );

    println!("\n{}", ColorOutput::success("✅ Setup complete!"));
    println!("\nNext steps:");
    println!("1. Edit configuration files in {}", config_dir.display());
    println!("2. Add your target states to {}", input_dir.display());
    println!("3. Run: cargo run -p rev_gol -- solve --config config/default.yaml");

    Ok(())
}

fn validate_command(
    config_path: PathBuf,
    predecessor_path: PathBuf,
    target_path: PathBuf,
    show_evolution: bool,
) -> Result<()> {
    println!("{}", ColorOutput::info("🔍 Validating solution..."));
    let config_path = resolve_workspace_path(config_path);
    let predecessor_path = resolve_workspace_path(predecessor_path);
    let target_path = resolve_workspace_path(target_path);

    // Load configuration
    let settings = if config_path.exists() {
        Settings::from_file(&config_path)?
    } else {
        let mut settings = Settings::default();
        settings.normalize_paths();
        settings
    };

    // Load grids
    let predecessor = load_grid_from_file(
        &predecessor_path,
        settings.simulation.boundary_condition.clone(),
    )
    .with_context(|| {
        format!(
            "Failed to load predecessor from {}",
            predecessor_path.display()
        )
    })?;

    let target = load_grid_from_file(&target_path, settings.simulation.boundary_condition.clone())
        .with_context(|| format!("Failed to load target from {}", target_path.display()))?;

    // Validate
    let validator = rev_gol::reverse::SolutionValidator::new(settings);
    let result = validator
        .validate(&predecessor, &target)
        .context("Validation failed")?;

    println!("{}", result);

    if show_evolution && !result.evolution_path.is_empty() {
        println!("\nEvolution Path:");
        for (i, grid) in result.evolution_path.iter().enumerate() {
            println!("Generation {}:", i);
            println!("{}", SolutionFormatter::format_grid_compact(grid));
        }
    }

    if result.is_valid {
        println!("{}", ColorOutput::success("✅ Solution is valid!"));
    } else {
        println!("{}", ColorOutput::error("❌ Solution is invalid"));
        if let Some(error) = result.error_message {
            println!("Error: {}", error);
        }
    }

    Ok(())
}

fn analyze_command(config_path: PathBuf, target_path: PathBuf) -> Result<()> {
    println!("{}", ColorOutput::info("🔬 Analyzing target state..."));
    let config_path = resolve_workspace_path(config_path);
    let target_path = resolve_workspace_path(target_path);

    // Load configuration
    let settings = if config_path.exists() {
        Settings::from_file(&config_path)?
    } else {
        let mut settings = Settings::default();
        settings.normalize_paths();
        settings
    };

    // Load target grid
    let target = load_grid_from_file(&target_path, settings.simulation.boundary_condition.clone())
        .with_context(|| format!("Failed to load target from {}", target_path.display()))?;

    println!("Target Grid ({}x{}):", target.width, target.height);
    println!("{}", SolutionFormatter::format_grid_with_coords(&target));

    println!("Grid Statistics:");
    println!("  Living cells: {}", target.living_count());
    println!(
        "  Density: {:.1}%",
        (target.living_count() as f64 / (target.width * target.height) as f64) * 100.0
    );

    // Create problem for analysis
    let problem = ReverseProblem::with_target_grid(settings, target)
        .context("Failed to create problem for analysis")?;

    let estimate = problem.estimate_solvability();
    println!("\n{}", estimate);

    let encoding_stats = problem.encoding_statistics();
    println!("{}", encoding_stats);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cli_parsing() {
        // Test that CLI parsing works
        let cli = Cli::try_parse_from(&[
            "rev_gol",
            "solve",
            "--config",
            "test.yaml",
            "--generations",
            "5",
        ]);

        assert!(cli.is_ok());
    }

    #[test]
    fn test_setup_command() {
        let temp_dir = tempdir().unwrap();
        let result = setup_command(temp_dir.path().to_path_buf(), false);

        assert!(result.is_ok());
        assert!(temp_dir.path().join("config/default.yaml").exists());
        assert!(temp_dir.path().join("input/target_states").exists());
    }
}
