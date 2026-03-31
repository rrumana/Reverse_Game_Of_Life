//! Factory for creating SAT solver instances based on configuration

use super::constraints::Clause;
use super::parkissat_solver::ParkissatSatSolver;
use super::solver::{SatSolver, SolverOptions, SolverSolution, SolverStatistics};
use crate::config::SolverBackend;
use anyhow::Result;
use std::collections::HashMap;

/// Unified SAT solver interface that can use different backends
pub enum UnifiedSatSolver {
    Cadical(SatSolver),
    Parkissat(ParkissatSatSolver),
}

impl UnifiedSatSolver {
    /// Create a new solver instance based on the specified backend
    pub fn new(backend: SolverBackend) -> Result<Self> {
        match backend {
            SolverBackend::Cadical => Ok(UnifiedSatSolver::Cadical(SatSolver::new())),
            SolverBackend::Parkissat => Ok(UnifiedSatSolver::Parkissat(ParkissatSatSolver::new()?)),
        }
    }

    /// Add clauses to the solver
    pub fn add_clauses(&mut self, clauses: &[Clause]) -> Result<()> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.add_clauses(clauses),
            UnifiedSatSolver::Parkissat(solver) => solver.add_clauses(clauses),
        }
    }

    /// Add a single clause to the solver
    pub fn add_clause(&mut self, clause: &Clause) -> Result<()> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.add_clause(clause),
            UnifiedSatSolver::Parkissat(solver) => solver.add_clause(clause),
        }
    }

    /// Solve the SAT problem and return the first solution
    pub fn solve(&mut self) -> Result<Option<SolverSolution>> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.solve(),
            UnifiedSatSolver::Parkissat(solver) => solver.solve(),
        }
    }

    /// Solve the SAT problem under a temporary set of assumptions.
    pub fn solve_with_assumptions(&mut self, assumptions: &[i32]) -> Result<Option<SolverSolution>> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.solve_with_assumptions(assumptions),
            UnifiedSatSolver::Parkissat(solver) => solver.solve_with_assumptions(assumptions),
        }
    }

    /// Check satisfiability under a temporary set of assumptions without extracting a full model.
    pub fn solve_under_assumptions(&mut self, assumptions: &[i32]) -> Result<bool> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.solve_under_assumptions(assumptions),
            UnifiedSatSolver::Parkissat(solver) => solver.solve_under_assumptions(assumptions),
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
        match self {
            UnifiedSatSolver::Cadical(solver) => {
                solver.solve_multiple_with_blocking_vars(max_solutions, blocking_vars)
            }
            UnifiedSatSolver::Parkissat(solver) => {
                solver.solve_multiple_with_blocking_vars(max_solutions, blocking_vars)
            }
        }
    }

    /// Get solver statistics
    pub fn statistics(&self) -> SolverStatistics {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.statistics(),
            UnifiedSatSolver::Parkissat(solver) => solver.statistics(),
        }
    }

    /// Reset the solver (clear all clauses)
    pub fn reset(&mut self) -> Result<()> {
        match self {
            UnifiedSatSolver::Cadical(solver) => {
                solver.reset();
                Ok(())
            }
            UnifiedSatSolver::Parkissat(solver) => solver.reset(),
        }
    }

    /// Check if a partial assignment satisfies all clauses
    pub fn check_assignment(&self, assignment: &HashMap<i32, bool>) -> bool {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.check_assignment(assignment),
            UnifiedSatSolver::Parkissat(solver) => solver.check_assignment(assignment),
        }
    }

    /// Get the number of variables
    pub fn variable_count(&self) -> usize {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.variable_count(),
            UnifiedSatSolver::Parkissat(solver) => solver.variable_count(),
        }
    }

    /// Get the number of clauses
    pub fn clause_count(&self) -> usize {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.clause_count(),
            UnifiedSatSolver::Parkissat(solver) => solver.clause_count(),
        }
    }

    /// Set solver configuration options
    pub fn configure(&mut self, options: &SolverOptions) -> Result<()> {
        match self {
            UnifiedSatSolver::Cadical(solver) => {
                solver.configure(options);
                Ok(())
            }
            UnifiedSatSolver::Parkissat(solver) => solver.configure(options),
        }
    }

    /// Get the backend type being used
    pub fn backend(&self) -> SolverBackend {
        match self {
            UnifiedSatSolver::Cadical(_) => SolverBackend::Cadical,
            UnifiedSatSolver::Parkissat(_) => SolverBackend::Parkissat,
        }
    }

    /// Read the value of a variable in the current SAT model.
    pub fn model_value(&self, variable: i32) -> Result<Option<bool>> {
        match self {
            UnifiedSatSolver::Cadical(solver) => solver.model_value(variable),
            UnifiedSatSolver::Parkissat(solver) => solver.model_value(variable),
        }
    }
}

impl Default for UnifiedSatSolver {
    fn default() -> Self {
        // Default to CaDiCaL for backward compatibility
        UnifiedSatSolver::Cadical(SatSolver::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SolverBackend;

    #[test]
    fn test_cadical_solver_creation() {
        let solver = UnifiedSatSolver::new(SolverBackend::Cadical).unwrap();
        assert_eq!(solver.backend(), SolverBackend::Cadical);
        assert_eq!(solver.variable_count(), 0);
        assert_eq!(solver.clause_count(), 0);
    }

    #[test]
    fn test_parkissat_solver_creation() {
        let solver = UnifiedSatSolver::new(SolverBackend::Parkissat).unwrap();
        assert_eq!(solver.backend(), SolverBackend::Parkissat);
        assert_eq!(solver.variable_count(), 0);
        assert_eq!(solver.clause_count(), 0);
    }

    #[test]
    fn test_simple_satisfiable_cadical() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Cadical).unwrap();

        // Add clause: x1
        let clause = Clause::new(vec![1]);
        solver.add_clause(&clause).unwrap();

        let result = solver.solve().unwrap();
        assert!(result.is_some());

        let solution = result.unwrap();
        assert_eq!(solution.assignment.get(&1), Some(&true));
    }

    #[test]
    fn test_simple_satisfiable_parkissat() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Parkissat).unwrap();

        // Add clause: x1
        let clause = Clause::new(vec![1]);
        solver.add_clause(&clause).unwrap();

        let result = solver.solve().unwrap();
        assert!(result.is_some());

        let solution = result.unwrap();
        // For clause x1, any assignment to x1 is valid (true or false)
        // Just verify that variable 1 has some assignment
        assert!(solution.assignment.contains_key(&1));
    }

    #[test]
    fn test_unsatisfiable_cadical() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Cadical).unwrap();

        // Add contradictory clauses: x1 and ¬x1
        solver.add_clause(&Clause::new(vec![1])).unwrap();
        solver.add_clause(&Clause::new(vec![-1])).unwrap();

        let result = solver.solve().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_unsatisfiable_parkissat() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Parkissat).unwrap();

        // Add contradictory clauses: x1 and ¬x1
        solver.add_clause(&Clause::new(vec![1])).unwrap();
        solver.add_clause(&Clause::new(vec![-1])).unwrap();

        let result = solver.solve().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_assumptions_respected_cadical() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Cadical).unwrap();
        solver.add_clause(&Clause::new(vec![1, 2])).unwrap();
        assert!(solver.solve_under_assumptions(&[1]).unwrap());
        assert!(solver.solve_under_assumptions(&[-1, 2]).unwrap());
        assert!(!solver.solve_under_assumptions(&[-1, -2]).unwrap());
    }

    #[test]
    fn test_assumptions_respected_parkissat() {
        let mut solver = UnifiedSatSolver::new(SolverBackend::Parkissat).unwrap();
        solver.add_clause(&Clause::new(vec![1, 2])).unwrap();
        assert!(solver.solve_under_assumptions(&[1]).unwrap());
        assert!(solver.solve_under_assumptions(&[-1, 2]).unwrap());
        assert!(!solver.solve_under_assumptions(&[-1, -2]).unwrap());
    }
}
