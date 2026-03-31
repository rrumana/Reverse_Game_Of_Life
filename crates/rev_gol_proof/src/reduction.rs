//! Exhaustive small-instance harnesses for the `SAT -> Rev_GOL` compiler.

use crate::circuit::CnfFormula;
use crate::compiler::ConstructionCompiler;
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct AssignmentReport {
    pub assignment: BTreeMap<String, bool>,
    pub formula_value: bool,
    pub construction_value: bool,
}

#[derive(Debug, Clone)]
pub struct ReductionReport {
    pub assignments: Vec<AssignmentReport>,
}

impl ReductionReport {
    pub fn preserves_truth_table(&self) -> bool {
        self.assignments
            .iter()
            .all(|row| row.formula_value == row.construction_value)
    }

    pub fn formula_is_satisfiable(&self) -> bool {
        self.assignments.iter().any(|row| row.formula_value)
    }

    pub fn construction_is_satisfiable(&self) -> bool {
        self.assignments.iter().any(|row| row.construction_value)
    }
}

pub fn exhaustive_reduction_report(formula: &CnfFormula) -> Result<ReductionReport> {
    let compiled = ConstructionCompiler::compile_cnf(formula)?;
    let variables = formula.variables();
    let mut assignments = Vec::new();

    for mask in 0..(1usize << variables.len()) {
        let assignment = variables
            .iter()
            .enumerate()
            .map(|(idx, variable)| (variable.clone(), (mask & (1usize << idx)) != 0))
            .collect::<BTreeMap<_, _>>();
        assignments.push(AssignmentReport {
            formula_value: formula.evaluate(&assignment),
            construction_value: compiled.evaluate(&assignment)?,
            assignment,
        });
    }

    Ok(ReductionReport { assignments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, Literal};

    #[test]
    fn test_exhaustive_report_preserves_truth_table_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);

        let report = exhaustive_reduction_report(&formula).unwrap();
        assert!(report.preserves_truth_table());
        assert_eq!(
            report.formula_is_satisfiable(),
            report.construction_is_satisfiable()
        );
    }
}
