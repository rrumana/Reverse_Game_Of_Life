//! Demonstration of ParKissat-RS integration
//!
//! This example shows how to use both CaDiCaL and ParKissat-RS solvers
//! through the unified interface.

use rev_gol::config::SolverBackend;
use rev_gol::sat::constraints::Clause;
use rev_gol::sat::{SolverOptions, SolverSolution, UnifiedSatSolver};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SAT Solver Backend Demonstration ===\n");

    // Test both solver backends
    test_solver_backend(SolverBackend::Cadical)?;
    test_solver_backend(SolverBackend::Parkissat)?;

    println!("✅ All solver backends working correctly!");
    Ok(())
}

fn test_solver_backend(backend: SolverBackend) -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing {:?} solver backend:", backend);

    // Configure the solver options
    let options = SolverOptions {
        num_threads: Some(4),
        enable_preprocessing: true,
        verbosity: 0,
        timeout: Some(Duration::from_secs(10)),
        random_seed: Some(42),
    };

    // Test 1: Simple satisfiable problem
    println!("  Test 1: Simple satisfiable problem (x1)");
    let mut solver1 = UnifiedSatSolver::new(backend)?;
    solver1.configure(&options)?;
    solver1.add_clause(&Clause::new(vec![1]))?; // x1

    let start = std::time::Instant::now();
    let result = solver1.solve()?;
    let solve_time = start.elapsed();

    match result {
        Some(assignment) => {
            println!(
                "    ✅ SAT - Variable 1 = {:?}",
                assignment.assignment.get(&1)
            );
            println!(
                "    ⏱️  Solve time: {:.3}ms",
                solve_time.as_secs_f64() * 1000.0
            );
        }
        None => {
            println!("    ❌ Unexpected UNSAT result");
            return Err("Expected SAT but got UNSAT".into());
        }
    }

    // Test 2: Unsatisfiable problem (create new solver instance)
    println!("  Test 2: Unsatisfiable problem (x1 ∧ ¬x1)");
    let mut solver2 = UnifiedSatSolver::new(backend)?;
    solver2.configure(&options)?;
    solver2.add_clause(&Clause::new(vec![1]))?; // x1
    solver2.add_clause(&Clause::new(vec![-1]))?; // ¬x1

    let result = solver2.solve()?;
    match result {
        Some(_) => {
            println!("    ❌ Unexpected SAT result");
            return Err("Expected UNSAT but got SAT".into());
        }
        None => {
            println!("    ✅ UNSAT - Correctly detected contradiction");
        }
    }

    // Test 3: Multiple solutions (create new solver instance)
    println!("  Test 3: Multiple solutions (x1 ∨ x2)");
    let mut solver3 = UnifiedSatSolver::new(backend)?;
    solver3.configure(&options)?;
    solver3.add_clause(&Clause::new(vec![1, 2]))?; // x1 ∨ x2

    let mut solutions: Vec<SolverSolution> = Vec::new();
    let mut iteration = 0;
    const MAX_SOLUTIONS: usize = 3;

    // For each solution found, create a new solver to find the next one
    while solutions.len() < MAX_SOLUTIONS && iteration < 10 {
        iteration += 1;

        let mut solver = UnifiedSatSolver::new(backend)?;
        solver.configure(&options)?;
        solver.add_clause(&Clause::new(vec![1, 2]))?; // x1 ∨ x2

        // Add blocking clauses for previously found solutions
        for prev_solution in &solutions {
            let mut blocking_clause = Vec::new();
            if let Some(&val1) = prev_solution.assignment.get(&1) {
                blocking_clause.push(if val1 { -1 } else { 1 });
            }
            if let Some(&val2) = prev_solution.assignment.get(&2) {
                blocking_clause.push(if val2 { -2 } else { 2 });
            }
            if !blocking_clause.is_empty() {
                solver.add_clause(&Clause::new(blocking_clause))?;
            }
        }

        match solver.solve()? {
            Some(assignment) => {
                solutions.push(assignment);
            }
            None => break, // No more solutions
        }
    }

    if solutions.len() > 0 {
        println!("    ✅ Found {} solutions", solutions.len());
        for (i, solution) in solutions.iter().enumerate() {
            let x1 = solution.assignment.get(&1).copied().unwrap_or(false);
            let x2 = solution.assignment.get(&2).copied().unwrap_or(false);
            println!("      Solution {}: x1={}, x2={}", i + 1, x1, x2);
        }
    } else {
        println!("    ❌ No solutions found");
        return Err("Expected at least one solution".into());
    }

    // Test 4: Get statistics (create new solver instance)
    let mut solver4 = UnifiedSatSolver::new(backend)?;
    solver4.configure(&options)?;
    solver4.add_clause(&Clause::new(vec![1, 2]))?; // x1 ∨ x2
    solver4.add_clause(&Clause::new(vec![-1, 2]))?; // ¬x1 ∨ x2
    solver4.add_clause(&Clause::new(vec![1, -2]))?; // x1 ∨ ¬x2
    solver4.add_clause(&Clause::new(vec![-1, -2]))?; // ¬x1 ∨ ¬x2

    let _ = solver4.solve()?;
    let stats = solver4.statistics();

    println!("  📊 Statistics:");
    println!("    Variables: {}", stats.variable_count);
    println!("    Clauses: {}", stats.clause_count);

    println!("  ✅ {:?} backend tests completed successfully\n", backend);
    Ok(())
}
