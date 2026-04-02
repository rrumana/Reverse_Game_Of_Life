use anyhow::Result;
use rev_gol_proof::board::build_published_board;
use rev_gol_proof::circuit::{Clause, CnfFormula, Literal};
use rev_gol_proof::compiler::ConstructionCompiler;

fn main() -> Result<()> {
    let formula = CnfFormula::new(vec![
        Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
        Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
    ]);

    let construction = ConstructionCompiler::compile_cnf(&formula)?;
    let (width, height) = construction.bounds();

    println!(
        "Macro construction bounds: {} columns x {} rows",
        width, height
    );
    println!("{}", construction.render_blueprint());
    if let Ok(board) = build_published_board(&construction) {
        println!(
            "Published board: {} pieces, {}x{}, live cells={}",
            board.pieces.len(),
            board.target.width,
            board.target.height,
            board.target.living_count()
        );
    }
    Ok(())
}
