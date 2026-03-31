use anyhow::Result;
use rev_gol_proof::circuit::{Clause, CnfFormula, Literal};
use rev_gol_proof::reduction::exhaustive_reduction_report;

fn main() -> Result<()> {
    let formula = CnfFormula::new(vec![
        Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
        Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
    ]);

    let report = exhaustive_reduction_report(&formula)?;
    println!(
        "truth_table_preserved={} formula_sat={} construction_sat={}",
        report.preserves_truth_table(),
        report.formula_is_satisfiable(),
        report.construction_is_satisfiable()
    );
    for row in report.assignments {
        println!(
            "{:?} => formula={} construction={}",
            row.assignment, row.formula_value, row.construction_value
        );
    }
    Ok(())
}
