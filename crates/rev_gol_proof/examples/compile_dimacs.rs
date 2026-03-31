use anyhow::{bail, Context, Result};
use rev_gol_proof::board::{audit_published_board_motifs, build_published_board};
use rev_gol_proof::compiler::ConstructionCompiler;
use rev_gol_proof::dimacs::parse_dimacs_file;
use rev_gol_proof::reduction::exhaustive_reduction_report;
use std::path::PathBuf;

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    output_grid: Option<PathBuf>,
    exhaustive_check: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut out = Self::default();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--input" => {
                    out.input = Some(PathBuf::from(
                        args.next().context("--input requires a path")?,
                    ));
                }
                "--output-grid" => {
                    out.output_grid = Some(PathBuf::from(
                        args.next().context("--output-grid requires a path")?,
                    ));
                }
                "--check-exhaustive" => out.exhaustive_check = true,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => bail!("Unknown option '{arg}'"),
            }
        }
        Ok(out)
    }
}

fn print_usage() {
    println!(
        "Usage: cargo run -p rev_gol_proof --example compile_dimacs -- --input <FILE> [--check-exhaustive] [--output-grid <FILE>]"
    );
}

fn main() -> Result<()> {
    let options = Options::parse()?;
    let input = options.input.context("An input DIMACS file is required")?;
    let formula = parse_dimacs_file(&input)?;
    let construction = ConstructionCompiler::compile_cnf(&formula)?;
    let (width, height) = construction.bounds();

    println!(
        "compiled {} clauses over {} vars into {} macro instances ({}x{})",
        formula.clauses.len(),
        formula.variables().len(),
        construction.instances.len(),
        width,
        height
    );
    println!("{}", construction.render_blueprint());

    if options.exhaustive_check {
        let vars = formula.variables();
        if vars.len() > 16 {
            bail!(
                "Refusing exhaustive check on {} variables; cap is 16",
                vars.len()
            );
        }
        let report = exhaustive_reduction_report(&formula)?;
        println!(
            "truth_table_preserved={} formula_sat={} construction_sat={}",
            report.preserves_truth_table(),
            report.formula_is_satisfiable(),
            report.construction_is_satisfiable()
        );
    }

    match build_published_board(&construction) {
        Ok(board) => {
            println!(
                "published board candidate: {} pieces, {}x{}, live cells={}",
                board.pieces.len(),
                board.target.width,
                board.target.height,
                board.target.living_count()
            );
            if let Some(path) = options.output_grid.as_ref() {
                board.save_target_grid(path)?;
                println!("saved published board grid to {}", path.display());
            }
        }
        Err(err) => {
            println!("published board candidate unavailable: {err}");
            if let Ok(audit) = audit_published_board_motifs(&construction) {
                println!("{}", audit.render_summary());
            }
        }
    }

    Ok(())
}
