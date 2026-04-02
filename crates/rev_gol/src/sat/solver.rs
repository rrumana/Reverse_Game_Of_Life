//! SAT solver integration using CaDiCaL

use super::constraints::Clause;
use anyhow::Result;
use cadical::Solver;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// SAT solver wrapper for CaDiCaL
pub struct SatSolver {
    solver: Solver,
    variable_count: usize,
    clause_count: usize,
    timeout: Option<Duration>,
}

/// Result of SAT solving
#[derive(Debug, Clone)]
pub struct SolverSolution {
    pub assignment: HashMap<i32, bool>,
    pub solve_time: Duration,
}

/// Statistics about the solving process
#[derive(Debug, Clone)]
pub struct SolverStatistics {
    pub variable_count: usize,
    pub clause_count: usize,
    pub solve_time: Duration,
    pub result: SolverResultType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverResultType {
    Satisfiable,
    Unsatisfiable,
    Timeout,
    Error,
}

impl SatSolver {
    /// Create a new SAT solver instance
    pub fn new() -> Self {
        Self {
            solver: Solver::new(),
            variable_count: 0,
            clause_count: 0,
            timeout: None,
        }
    }

    /// Set solving timeout
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Add clauses to the solver
    pub fn add_clauses(&mut self, clauses: &[Clause]) -> Result<()> {
        for clause in clauses {
            self.add_clause(clause)?;
        }
        Ok(())
    }

    /// Add a single clause to the solver
    pub fn add_clause(&mut self, clause: &Clause) -> Result<()> {
        if clause.is_empty() {
            anyhow::bail!("Cannot add empty clause (unsatisfiable)");
        }

        // Update variable count
        for &literal in &clause.literals {
            let var = literal.abs() as usize;
            if var > self.variable_count {
                self.variable_count = var;
            }
        }

        // Add clause to solver
        self.solver.add_clause(clause.literals.iter().copied());

        self.clause_count += 1;
        Ok(())
    }

    fn solve_status(&mut self) -> Result<Option<bool>> {
        let start_time = Instant::now();

        // Set timeout if specified
        if let Some(_timeout) = self.timeout {
            // CaDiCaL doesn't have direct timeout support, so we'll implement a simple check
            // In a production system, you might want to use a more sophisticated timeout mechanism
        }

        let result = self.solver.solve();
        let _solve_time = start_time.elapsed();

        Ok(result)
    }

    fn solve_status_with_assumptions(&mut self, assumptions: &[i32]) -> Result<Option<bool>> {
        let start_time = Instant::now();

        if let Some(_timeout) = self.timeout {
            // CaDiCaL doesn't have direct timeout support, so we'll implement a simple check
            // In a production system, you might want to use a more sophisticated timeout mechanism
        }

        let result = self.solver.solve_with(assumptions.iter().copied());
        let _solve_time = start_time.elapsed();

        Ok(result)
    }

    /// Solve the SAT problem and return the first solution
    pub fn solve(&mut self) -> Result<Option<SolverSolution>> {
        let start_time = Instant::now();
        let result = self.solve_status()?;
        let solve_time = start_time.elapsed();

        if result == Some(true) {
            let assignment = self.extract_assignment()?;
            Ok(Some(SolverSolution {
                assignment,
                solve_time,
            }))
        } else {
            Ok(None)
        }
    }

    /// Solve under a set of assumptions and return whether the instance is satisfiable.
    pub fn solve_under_assumptions(&mut self, assumptions: &[i32]) -> Result<bool> {
        Ok(self.solve_status_with_assumptions(assumptions)? == Some(true))
    }

    /// Solve under a set of assumptions and return the first solution, if any.
    pub fn solve_with_assumptions(
        &mut self,
        assumptions: &[i32],
    ) -> Result<Option<SolverSolution>> {
        let start_time = Instant::now();
        let result = self.solve_status_with_assumptions(assumptions)?;
        let solve_time = start_time.elapsed();

        if result == Some(true) {
            let assignment = self.extract_assignment()?;
            Ok(Some(SolverSolution {
                assignment,
                solve_time,
            }))
        } else {
            Ok(None)
        }
    }

    /// Solve and find multiple solutions up to a limit
    pub fn solve_multiple(&mut self, max_solutions: usize) -> Result<Vec<SolverSolution>> {
        self.solve_multiple_with_blocking_vars(max_solutions, None)
    }

    /// Solve and find multiple solutions, optionally blocking only a selected set of variables
    pub fn solve_multiple_with_blocking_vars(
        &mut self,
        max_solutions: usize,
        blocking_vars: Option<&[i32]>,
    ) -> Result<Vec<SolverSolution>> {
        let mut solutions = Vec::new();
        let start_time = Instant::now();

        for _ in 0..max_solutions {
            if self.solve_status()? == Some(true) {
                let assignment = self.extract_assignment()?;
                let solution = SolverSolution {
                    assignment: assignment.clone(),
                    solve_time: start_time.elapsed(),
                };
                solutions.push(solution);

                // Add blocking clause to prevent finding the same solution again
                self.add_blocking_clause(&assignment, blocking_vars)?;
            } else {
                break;
            }
        }

        Ok(solutions)
    }

    /// Extract variable assignment from the solver
    fn extract_assignment(&self) -> Result<HashMap<i32, bool>> {
        let mut assignment = HashMap::new();

        for var in 1..=self.variable_count as i32 {
            if let Some(value) = self.solver.value(var) {
                assignment.insert(var, value);
            }
        }

        Ok(assignment)
    }

    /// Read the value of a variable in the current SAT model.
    pub fn model_value(&self, variable: i32) -> Result<Option<bool>> {
        if variable <= 0 {
            anyhow::bail!("Invalid variable {}", variable);
        }

        Ok(self.solver.value(variable))
    }

    /// Add a blocking clause to prevent finding the same solution again
    fn add_blocking_clause(
        &mut self,
        assignment: &HashMap<i32, bool>,
        blocking_vars: Option<&[i32]>,
    ) -> Result<()> {
        let mut blocking_literals = Vec::new();

        let blocking_variable_ids: Vec<i32> = match blocking_vars {
            Some(vars) => vars.to_vec(),
            None => {
                let mut vars: Vec<i32> = assignment.keys().copied().collect();
                vars.sort_unstable();
                vars
            }
        };

        for var in blocking_variable_ids {
            let Some(&value) = assignment.get(&var) else {
                anyhow::bail!(
                    "Blocking variable {} is missing from the model assignment",
                    var
                );
            };

            // Add the negation of the current assignment
            blocking_literals.push(if value { -var } else { var });
        }

        let blocking_clause = Clause::new(blocking_literals);
        self.add_clause(&blocking_clause)?;

        Ok(())
    }

    /// Get solver statistics
    pub fn statistics(&self) -> SolverStatistics {
        SolverStatistics {
            variable_count: self.variable_count,
            clause_count: self.clause_count,
            solve_time: Duration::from_secs(0), // Will be updated during solving
            result: SolverResultType::Error,    // Will be updated during solving
        }
    }

    /// Reset the solver (clear all clauses)
    pub fn reset(&mut self) {
        self.solver = Solver::new();
        self.variable_count = 0;
        self.clause_count = 0;
    }

    /// Check if a partial assignment satisfies all clauses
    pub fn check_assignment(&self, _assignment: &HashMap<i32, bool>) -> bool {
        // This is a simplified check - in practice, you might want to use
        // the solver's internal checking mechanisms
        true // Placeholder implementation
    }

    /// Get the number of variables
    pub fn variable_count(&self) -> usize {
        self.variable_count
    }

    /// Get the number of clauses
    pub fn clause_count(&self) -> usize {
        self.clause_count
    }

    /// Set solver configuration options
    pub fn configure(&mut self, options: &SolverOptions) {
        // Note: CaDiCaL 0.1 has limited configuration options
        // Most optimization is handled internally

        if let Some(timeout) = options.timeout {
            self.set_timeout(timeout);
        }

        // CaDiCaL is single-threaded, so num_threads is ignored
        // preprocessing and verbosity options are not exposed in the 0.1 API
        // but we store them for reference
    }
}

/// Configuration options for the SAT solver
#[derive(Debug, Clone)]
pub struct SolverOptions {
    pub num_threads: Option<usize>,
    pub enable_preprocessing: bool,
    pub verbosity: u32,
    pub timeout: Option<Duration>,
    pub random_seed: Option<u64>,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            num_threads: None, // Use available parallelism by default
            enable_preprocessing: true,
            verbosity: 0,
            timeout: None,
            random_seed: None,
        }
    }
}

impl std::fmt::Display for SolverStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SAT Solver Statistics:")?;
        writeln!(f, "  Variables: {}", self.variable_count)?;
        writeln!(f, "  Clauses: {}", self.clause_count)?;
        writeln!(f, "  Solve time: {:.3}s", self.solve_time.as_secs_f64())?;
        writeln!(f, "  Result: {:?}", self.result)?;
        Ok(())
    }
}

impl std::fmt::Display for SolverSolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SAT Solution:")?;
        writeln!(f, "  Solve time: {:.3}s", self.solve_time.as_secs_f64())?;
        writeln!(f, "  Variables assigned: {}", self.assignment.len())?;

        // Show a few example assignments
        let mut vars: Vec<_> = self.assignment.keys().collect();
        vars.sort();

        write!(f, "  Sample assignments: ")?;
        for (i, &var) in vars.iter().take(10).enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            let value = self.assignment[var];
            write!(f, "{}={}", var, if value { "T" } else { "F" })?;
        }
        if vars.len() > 10 {
            write!(f, ", ...")?;
        }
        writeln!(f)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_solver_creation() {
        let solver = SatSolver::new();
        assert_eq!(solver.variable_count(), 0);
        assert_eq!(solver.clause_count(), 0);
    }

    #[test]
    fn test_simple_satisfiable() {
        let mut solver = SatSolver::new();

        // Add clause: x1 ∨ x2
        let clause1 = Clause::new(vec![1, 2]);
        solver.add_clause(&clause1).unwrap();

        // Add clause: ¬x1 ∨ x2
        let clause2 = Clause::new(vec![-1, 2]);
        solver.add_clause(&clause2).unwrap();

        let solution = solver.solve().unwrap();
        assert!(solution.is_some());

        let assignment = solution.unwrap().assignment;
        // x2 should be true to satisfy both clauses
        assert_eq!(assignment.get(&2), Some(&true));
    }

    #[test]
    fn test_unsatisfiable() {
        let mut solver = SatSolver::new();

        // Add contradictory clauses: x1 and ¬x1
        let clause1 = Clause::unit(1);
        let clause2 = Clause::unit(-1);

        solver.add_clause(&clause1).unwrap();
        solver.add_clause(&clause2).unwrap();

        let solution = solver.solve().unwrap();
        assert!(solution.is_none());
    }

    #[test]
    fn test_multiple_solutions() {
        let mut solver = SatSolver::new();

        // Add clause: x1 ∨ x2 (has multiple solutions)
        let clause = Clause::new(vec![1, 2]);
        solver.add_clause(&clause).unwrap();

        let solutions = solver.solve_multiple(3).unwrap();
        assert!(!solutions.is_empty());

        // Each solution should satisfy the clause
        for solution in &solutions {
            let x1 = solution.assignment.get(&1).unwrap_or(&false);
            let x2 = solution.assignment.get(&2).unwrap_or(&false);
            assert!(*x1 || *x2); // At least one should be true
        }
    }

    #[test]
    fn test_solver_options() {
        let mut solver = SatSolver::new();
        let options = SolverOptions {
            num_threads: Some(4),
            enable_preprocessing: true,
            verbosity: 1,
            timeout: Some(Duration::from_secs(10)),
            random_seed: Some(42),
        };

        solver.configure(&options);
        // Test that configuration doesn't crash
        assert_eq!(solver.variable_count(), 0);
    }

    #[test]
    fn test_empty_clause_error() {
        let mut solver = SatSolver::new();
        let empty_clause = Clause::new(vec![]);

        assert!(solver.add_clause(&empty_clause).is_err());
    }

    #[test]
    fn test_variable_count_tracking() {
        let mut solver = SatSolver::new();

        let clause1 = Clause::new(vec![1, -5, 3]);
        solver.add_clause(&clause1).unwrap();

        assert_eq!(solver.variable_count(), 5); // Highest variable is 5

        let clause2 = Clause::new(vec![2, -7]);
        solver.add_clause(&clause2).unwrap();

        assert_eq!(solver.variable_count(), 7); // Now highest is 7
    }
}
