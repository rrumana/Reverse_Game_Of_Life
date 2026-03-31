//! DIMACS CNF parsing for the proof-side SAT-to-Rev-GOL compiler.

use crate::circuit::{Clause, CnfFormula, Literal};
use anyhow::{Context, Result};
use std::path::Path;

pub fn parse_dimacs_file(path: &Path) -> Result<CnfFormula> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read DIMACS file {}", path.display()))?;
    parse_dimacs_str(&text)
}

pub fn parse_dimacs_str(text: &str) -> Result<CnfFormula> {
    let mut clauses = Vec::new();
    let mut current = Vec::<i32>::new();
    let mut declared_vars = None;
    let mut declared_clauses = None;
    let mut seen_header = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('c') {
            continue;
        }

        if line.starts_with('p') {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() != 4 || parts[1] != "cnf" {
                anyhow::bail!("Unsupported DIMACS header '{}'", line);
            }
            declared_vars = Some(
                parts[2]
                    .parse::<usize>()
                    .with_context(|| format!("Invalid variable count in '{}'", line))?,
            );
            declared_clauses = Some(
                parts[3]
                    .parse::<usize>()
                    .with_context(|| format!("Invalid clause count in '{}'", line))?,
            );
            seen_header = true;
            continue;
        }

        for token in line.split_whitespace() {
            let lit = token
                .parse::<i32>()
                .with_context(|| format!("Invalid DIMACS literal '{}'", token))?;
            if lit == 0 {
                clauses.push(Clause::new(
                    current
                        .drain(..)
                        .map(dimacs_literal_to_literal)
                        .collect::<Vec<_>>(),
                ));
            } else {
                current.push(lit);
            }
        }
    }

    if !seen_header {
        anyhow::bail!("DIMACS input is missing a 'p cnf' header");
    }
    if !current.is_empty() {
        anyhow::bail!("DIMACS input ended before a clause-terminating 0");
    }

    if let Some(expected) = declared_clauses {
        if clauses.len() != expected {
            anyhow::bail!(
                "DIMACS declared {} clauses but parsed {}",
                expected,
                clauses.len()
            );
        }
    }
    if let Some(expected) = declared_vars {
        let actual_max = clauses
            .iter()
            .flat_map(|clause| clause.literals.iter())
            .map(|lit| lit.variable[1..].parse::<usize>().unwrap_or(0))
            .max()
            .unwrap_or(0);
        if actual_max > expected {
            anyhow::bail!(
                "DIMACS declared {} variables but used variable {}",
                expected,
                actual_max
            );
        }
    }

    Ok(CnfFormula::new(clauses))
}

fn dimacs_literal_to_literal(value: i32) -> Literal {
    let variable = format!("x{}", value.unsigned_abs());
    if value < 0 {
        Literal::negative(variable)
    } else {
        Literal::positive(variable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dimacs_str_basic_formula() {
        let formula = parse_dimacs_str(
            "c example\np cnf 3 2\n1 -2 0\n2 3 0\n",
        )
        .unwrap();

        assert_eq!(formula.clauses.len(), 2);
        assert_eq!(formula.variables(), vec!["x1", "x2", "x3"]);
    }

    #[test]
    fn test_parse_dimacs_requires_header() {
        let err = parse_dimacs_str("1 -2 0\n").unwrap_err();
        assert!(err.to_string().contains("missing a 'p cnf' header"));
    }

    #[test]
    fn test_parse_dimacs_rejects_unterminated_clause() {
        let err = parse_dimacs_str("p cnf 2 1\n1 -2\n").unwrap_err();
        assert!(err.to_string().contains("ended before a clause-terminating 0"));
    }
}
