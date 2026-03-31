//! Simplified benchmark tool for comparing CaDiCaL vs ParKissat-RS solvers
//!
//! This tool demonstrates the threading capabilities of ParKissat-RS
//! compared to single-threaded CaDiCaL.

use anyhow::Result;
use rev_gol::{
    config::{BoundaryCondition, Settings, SolverBackend},
    game_of_life::io::parse_grid_from_string,
    reverse::ReverseProblem,
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct BenchmarkResult {
    solver_backend: SolverBackend,
    thread_count: usize,
    run_time: Duration,
    success: bool,
    solutions_found: usize,
}

impl BenchmarkResult {
    fn new(
        solver_backend: SolverBackend,
        thread_count: usize,
        run_time: Duration,
        success: bool,
        solutions_found: usize,
    ) -> Self {
        Self {
            solver_backend,
            thread_count,
            run_time,
            success,
            solutions_found,
        }
    }
}

fn main() -> Result<()> {
    println!("=== SAT Solver Threading Benchmark ===\n");

    // Test configurations
    let configs = vec![
        (SolverBackend::Cadical, 1),   // CaDiCaL is single-threaded
        (SolverBackend::Parkissat, 1), // ParKissat with 1 thread
        (SolverBackend::Parkissat, 2), // ParKissat with 2 threads
        (SolverBackend::Parkissat, 4), // ParKissat with 4 threads
        (SolverBackend::Parkissat, 8), // ParKissat with 8 threads
    ];

    let mut results = Vec::new();

    for (backend, thread_count) in configs {
        println!(
            "Testing {:?} with {} thread{}:",
            backend,
            thread_count,
            if thread_count == 1 { "" } else { "s" }
        );

        match run_benchmark(backend, thread_count) {
            Ok(result) => {
                println!(
                    "  ✅ Completed in {:.2}s (found {} solution{})",
                    result.run_time.as_secs_f64(),
                    result.solutions_found,
                    if result.solutions_found == 1 { "" } else { "s" }
                );
                results.push(result);
            }
            Err(e) => {
                println!("  ❌ Failed: {}", e);
                results.push(BenchmarkResult::new(
                    backend,
                    thread_count,
                    Duration::ZERO,
                    false,
                    0,
                ));
            }
        }
        println!();
    }

    // Print summary
    println!("=== Benchmark Summary ===");
    for result in &results {
        if result.success {
            println!(
                "{:?} ({} thread{}): {:.2}s - {} solution{}",
                result.solver_backend,
                result.thread_count,
                if result.thread_count == 1 { "" } else { "s" },
                result.run_time.as_secs_f64(),
                result.solutions_found,
                if result.solutions_found == 1 { "" } else { "s" }
            );
        } else {
            println!(
                "{:?} ({} thread{}): FAILED",
                result.solver_backend,
                result.thread_count,
                if result.thread_count == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

fn run_benchmark(backend: SolverBackend, thread_count: usize) -> Result<BenchmarkResult> {
    // Create a simple test settings
    let mut settings = Settings::default();
    settings.solver.backend = backend;
    settings.solver.num_threads = Some(thread_count);
    settings.solver.enable_preprocessing = true;
    settings.solver.verbosity = 0;
    settings.solver.timeout_seconds = 30; // 30 second timeout
    settings.simulation.generations = 3; // Simple problem

    // Create a simple target state (3x3 blinker pattern)
    let target_content = "010\n010\n010";

    let start = Instant::now();

    // Parse the target grid and create the problem
    let target_grid = parse_grid_from_string(target_content, BoundaryCondition::Dead)?;
    let mut problem = ReverseProblem::with_target_grid(settings, target_grid)?;
    let solutions = problem.solve()?;

    let duration = start.elapsed();

    Ok(BenchmarkResult::new(
        backend,
        thread_count,
        duration,
        !solutions.is_empty(),
        solutions.len(),
    ))
}
