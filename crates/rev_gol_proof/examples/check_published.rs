use anyhow::{bail, Context, Result};
use rev_gol::config::SolverBackend;
use rev_gol_proof::published::{
    load_published_gadget, published_part1_specs, published_root, verify_published_spec,
};
use rev_gol_proof::verifier::{GadgetVerifier, GadgetVerifierConfig};
use std::time::Duration;

#[derive(Debug, Clone)]
struct Options {
    filter: Option<String>,
    backend: SolverBackend,
    threads: Option<usize>,
    timeout: Option<Duration>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            filter: None,
            backend: SolverBackend::Parkissat,
            threads: None,
            timeout: None,
        }
    }
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--filter" => {
                    options.filter = Some(
                        args.next()
                            .context("--filter requires a value")?
                            .to_string(),
                    );
                }
                "--backend" => {
                    let value = args.next().context("--backend requires a value")?;
                    options.backend = match value.as_str() {
                        "cadical" => SolverBackend::Cadical,
                        "parkissat" => SolverBackend::Parkissat,
                        _ => bail!("Unsupported backend '{value}'"),
                    };
                }
                "--threads" => {
                    let value = args.next().context("--threads requires a value")?;
                    options.threads = Some(
                        value
                            .parse::<usize>()
                            .with_context(|| format!("Invalid thread count '{value}'"))?,
                    );
                }
                "--all-threads" => {
                    options.threads = None;
                }
                "--timeout-seconds" => {
                    let value = args.next().context("--timeout-seconds requires a value")?;
                    options.timeout =
                        Some(Duration::from_secs(value.parse::<u64>().with_context(
                            || format!("Invalid timeout value '{value}'"),
                        )?));
                }
                "--no-timeout" => {
                    options.timeout = None;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ if arg.starts_with("--") => bail!("Unknown option '{arg}'"),
                _ => {
                    if options.filter.is_some() {
                        bail!("Unexpected positional argument '{arg}'");
                    }
                    options.filter = Some(arg);
                }
            }
        }

        Ok(options)
    }
}

fn print_usage() {
    println!("Usage: cargo run -p rev_gol_proof --example check_published -- [OPTIONS] [FILTER]");
    println!();
    println!("Options:");
    println!("  --filter <TEXT>           Verify only specs whose names contain TEXT");
    println!("  --backend <cadical|parkissat>");
    println!("                            SAT backend to use");
    println!("  --threads <N>             Use exactly N threads when supported");
    println!("  --all-threads             Let the backend choose the thread count");
    println!("  --timeout-seconds <N>     Stop solver calls after N seconds");
    println!("  --no-timeout              Disable solver timeout");
}

fn main() -> Result<()> {
    let options = Options::parse()?;
    let root = published_root();
    let verifier = GadgetVerifier::new(GadgetVerifierConfig {
        backend: options.backend,
        num_threads: options.threads,
        enable_preprocessing: true,
        verbosity: 0,
        timeout: options.timeout,
    });

    for spec in published_part1_specs() {
        if let Some(filter) = &options.filter {
            if !spec.name.contains(filter) {
                continue;
            }
        }

        let report = verify_published_spec(&verifier, &root, &spec)?;
        println!(
            "{}: success={} size={} align={} allowed={} forbidden={} charging={}",
            spec.name,
            report.is_success(),
            report.size_matches,
            report.alignment_matches,
            report.relation_report.allowed_assignments_hold,
            report.relation_report.forbidden_assignments_hold,
            report
                .charging_reports
                .iter()
                .all(|check| check.all_outputs_are_named_states)
        );

        if !report.is_success() {
            let (published, _) = load_published_gadget(&root, &spec)?;
            println!(
                "  expected align={:?}, actual align={:?}",
                spec.align,
                published.phase_alignment()
            );
            println!("  allowed results:");
            for result in &report.relation_report.allowed_results {
                println!(
                    "    {:?} => {}",
                    result.assignment.states, result.satisfiable
                );
            }
            println!("  forbidden results:");
            for result in &report.relation_report.forbidden_results {
                println!(
                    "    {:?} => {}",
                    result.assignment.states, result.satisfiable
                );
            }
            println!("  charging results:");
            for result in &report.charging_reports {
                println!(
                    "    {:?} -> {:?}, named={}",
                    result.fixed_assignment.states,
                    result.observed_outputs,
                    result.all_outputs_are_named_states
                );
            }
        }
    }

    Ok(())
}
