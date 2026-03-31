use anyhow::{bail, Context, Result};
use rev_gol_proof::board::audit_published_board_motifs;
use rev_gol_proof::compiler::ConstructionCompiler;
use rev_gol_proof::dimacs::parse_dimacs_file;
use std::path::PathBuf;

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
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
    println!("Usage: cargo run -p rev_gol_proof --example audit_board -- --input <FILE>");
}

fn main() -> Result<()> {
    let options = Options::parse()?;
    let input = options.input.context("An input DIMACS file is required")?;
    let formula = parse_dimacs_file(&input)?;
    let construction = ConstructionCompiler::compile_cnf(&formula)?;
    let audit = audit_published_board_motifs(&construction)?;

    println!("{}", audit.render_summary());
    println!();
    println!("Suggested unresolved vertical calibrations:");
    for family in audit.unresolved_vertical() {
        println!("  {} over {}", family.top_spec, family.bottom_spec);
    }
    println!("Suggested unresolved horizontal calibrations:");
    for family in audit.unresolved_horizontal() {
        println!(
            "  {} + {} + {}",
            family.left_spec, family.connector_name, family.right_spec
        );
    }

    Ok(())
}
