use anyhow::{bail, Context, Result};
use rev_gol_proof::board::{
    audit_published_board_motifs, build_published_board_with_options, BoardBuildOptions,
};
use rev_gol_proof::compiler::ConstructionCompiler;
use rev_gol_proof::contracts::{
    certify_compiled_construction, default_contract_verifier, discharge_construction_basis,
};
use rev_gol_proof::dimacs::parse_dimacs_file;
use rev_gol_proof::interfaces::{
    discharge_horizontal_interface_family, discharge_vertical_interface_family,
    enumerate_router_interface_basis, filtered_horizontal_families, filtered_vertical_families,
    horizontal_family_label, vertical_family_label, InterfaceBasisCertificate,
    InterfaceLemmaSummary,
};
use rev_gol_proof::reduction::exhaustive_reduction_report;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    output_grid: Option<PathBuf>,
    exhaustive_check: bool,
    audit_board: bool,
    build_board: bool,
    discharge_contracts: bool,
    discharge_logical_contracts: bool,
    discharge_routing_contracts: bool,
    discharge_interfaces: bool,
    discharge_horizontal_interfaces: bool,
    discharge_vertical_interfaces: bool,
    contract_filter: Option<String>,
    interface_filter: Option<String>,
    interface_max_candidates: Option<usize>,
    print_interface_families: bool,
    print_router_interface_basis: bool,
    discharge_router_interface_basis: bool,
    load_interface_basis: Option<PathBuf>,
    save_interface_basis: Option<PathBuf>,
    contract_timeout: Option<Duration>,
    allow_exact_placement: bool,
    exact_placement_limit: Option<usize>,
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
                "--audit-board" => out.audit_board = true,
                "--build-board" => out.build_board = true,
                "--discharge-contracts" => out.discharge_contracts = true,
                "--discharge-logical-contracts" => out.discharge_logical_contracts = true,
                "--discharge-routing-contracts" => out.discharge_routing_contracts = true,
                "--discharge-interfaces" => out.discharge_interfaces = true,
                "--discharge-horizontal-interfaces" => out.discharge_horizontal_interfaces = true,
                "--discharge-vertical-interfaces" => out.discharge_vertical_interfaces = true,
                "--contract-filter" => {
                    out.contract_filter =
                        Some(args.next().context("--contract-filter requires a value")?);
                }
                "--interface-filter" => {
                    out.interface_filter =
                        Some(args.next().context("--interface-filter requires a value")?);
                }
                "--interface-max-candidates" => {
                    let raw = args
                        .next()
                        .context("--interface-max-candidates requires an integer")?;
                    out.interface_max_candidates =
                        Some(raw.parse().with_context(|| {
                            format!("Invalid interface candidate limit '{raw}'")
                        })?);
                }
                "--print-interface-families" => out.print_interface_families = true,
                "--print-router-interface-basis" => out.print_router_interface_basis = true,
                "--discharge-router-interface-basis" => {
                    out.discharge_router_interface_basis = true;
                }
                "--load-interface-basis" => {
                    out.load_interface_basis = Some(PathBuf::from(
                        args.next()
                            .context("--load-interface-basis requires a path")?,
                    ));
                }
                "--save-interface-basis" => {
                    out.save_interface_basis = Some(PathBuf::from(
                        args.next()
                            .context("--save-interface-basis requires a path")?,
                    ));
                }
                "--contract-timeout-seconds" => {
                    let raw = args
                        .next()
                        .context("--contract-timeout-seconds requires an integer")?;
                    let seconds = raw
                        .parse::<u64>()
                        .with_context(|| format!("Invalid contract timeout '{raw}'"))?;
                    out.contract_timeout = Some(Duration::from_secs(seconds));
                }
                "--allow-exact-placement" => out.allow_exact_placement = true,
                "--exact-placement-limit" => {
                    let raw = args
                        .next()
                        .context("--exact-placement-limit requires an integer")?;
                    out.exact_placement_limit = Some(
                        raw.parse()
                            .with_context(|| format!("Invalid exact-placement limit '{raw}'"))?,
                    );
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => bail!("Unknown option '{arg}'"),
            }
        }
        if out.output_grid.is_some() {
            out.build_board = true;
        }
        if out.discharge_contracts {
            out.discharge_logical_contracts = true;
            out.discharge_routing_contracts = true;
        }
        if out.discharge_interfaces {
            out.discharge_horizontal_interfaces = true;
            out.discharge_vertical_interfaces = true;
        }
        if out.allow_exact_placement && !out.build_board {
            bail!("--allow-exact-placement requires --build-board or --output-grid");
        }
        if out.exact_placement_limit.is_some() && !out.build_board {
            bail!("--exact-placement-limit requires --build-board or --output-grid");
        }
        if out.save_interface_basis.is_some()
            && !(out.discharge_router_interface_basis
                || (out.discharge_horizontal_interfaces && out.discharge_vertical_interfaces))
        {
            bail!(
                "--save-interface-basis requires either --discharge-router-interface-basis or a complete witness interface discharge via --discharge-interfaces or both --discharge-horizontal-interfaces and --discharge-vertical-interfaces"
            );
        }
        Ok(out)
    }
}

fn print_usage() {
    println!(concat!(
        "Usage: cargo run -p rev_gol_proof --example compile_dimacs -- --input <FILE>",
        " [--check-exhaustive] [--audit-board]",
        " [--discharge-contracts | --discharge-logical-contracts | --discharge-routing-contracts]",
        " [--contract-filter <TEXT>]",
        " [--discharge-interfaces | --discharge-horizontal-interfaces | --discharge-vertical-interfaces]",
        " [--interface-filter <TEXT>]",
        " [--interface-max-candidates <N>]",
        " [--print-interface-families]",
        " [--print-router-interface-basis]",
        " [--discharge-router-interface-basis]",
        " [--load-interface-basis <FILE>] [--save-interface-basis <FILE>]",
        " [--contract-timeout-seconds <N>]",
        " [--build-board [--allow-exact-placement] [--exact-placement-limit <N>]]",
        " [--output-grid <FILE>]"
    ));
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
    let mut certificate = certify_compiled_construction(&construction)?;
    if let Some(path) = options.load_interface_basis.as_ref() {
        let basis = InterfaceBasisCertificate::load_json(path)?;
        certificate = certificate.with_interface_basis_certificate(&basis)?;
        println!("{}", basis.render_summary());
    }
    println!("{}", certificate.render_summary());
    if options.print_interface_families {
        println!(
            "{}",
            certificate
                .routing_witness
                .render_interface_family_summary()
        );
    }
    if options.print_router_interface_basis {
        let basis = enumerate_router_interface_basis()?;
        println!("{}", basis.render_family_summary());
    }
    if options.discharge_logical_contracts || options.discharge_routing_contracts {
        let verifier = default_contract_verifier(options.contract_timeout);
        let discharge = discharge_construction_basis(
            &verifier,
            options.discharge_logical_contracts,
            options.discharge_routing_contracts,
            options.contract_filter.as_deref(),
        )?;
        println!("{}", discharge.render_summary());
        for report in discharge
            .logical
            .iter()
            .chain(discharge.routing.iter())
            .filter(|report| !report.is_success())
        {
            println!(
                "  contract {} [{}]: symbolic_match={} published_success={} error={}",
                report.label,
                report.published_spec_name,
                report.symbolic_relation_matches,
                report.published_success,
                report.error.as_deref().unwrap_or("<none>")
            );
        }
    }
    if options.discharge_router_interface_basis {
        let verifier = default_contract_verifier(options.contract_timeout);
        let basis = enumerate_router_interface_basis()?;
        let horizontal_families = basis
            .horizontal
            .iter()
            .filter(|family| {
                options.interface_filter.as_deref().is_none_or(|filter| {
                    horizontal_family_label(family).contains(filter)
                })
            })
            .collect::<Vec<_>>();
        let vertical_families = basis
            .vertical
            .iter()
            .filter(|family| {
                options.interface_filter.as_deref().is_none_or(|filter| {
                    vertical_family_label(family).contains(filter)
                })
            })
            .collect::<Vec<_>>();
        let mut horizontal_reports = Vec::new();
        let mut vertical_reports = Vec::new();

        println!("{}", basis.render_summary());
        for (idx, family) in horizontal_families.iter().enumerate() {
            println!(
                "discharging router-basis horizontal interface {}/{}: {}",
                idx + 1,
                horizontal_families.len(),
                horizontal_family_label(family)
            );
            io::stdout().flush()?;
            let report = discharge_horizontal_interface_family(
                &verifier,
                family,
                options.interface_max_candidates,
            )?;
            println!("  {}", report.render_summary_line());
            horizontal_reports.push(report);
        }
        for (idx, family) in vertical_families.iter().enumerate() {
            println!(
                "discharging router-basis vertical interface {}/{}: {}",
                idx + 1,
                vertical_families.len(),
                vertical_family_label(family)
            );
            io::stdout().flush()?;
            let report = discharge_vertical_interface_family(
                &verifier,
                family,
                options.interface_max_candidates,
            )?;
            println!("  {}", report.render_summary_line());
            vertical_reports.push(report);
        }

        let discharge = InterfaceLemmaSummary {
            horizontal: horizontal_reports,
            vertical: vertical_reports,
        };
        println!("{}", discharge.render_summary());
        if discharge.is_success() {
            let basis_certificate = InterfaceBasisCertificate::from_summary(&discharge)?;
            basis_certificate.covers_router_basis(&basis)?;
            basis_certificate.covers_witness(&certificate.routing_witness)?;
            println!("{}", basis_certificate.render_summary());
            if let Some(path) = options.save_interface_basis.as_ref() {
                basis_certificate.save_json(path)?;
                println!("saved interface basis certificate to {}", path.display());
            }
        }
        for report in discharge
            .horizontal
            .iter()
            .chain(discharge.vertical.iter())
            .filter(|report| !report.is_success())
        {
            println!("  {}", report.render_summary_line());
        }
    }
    if options.discharge_horizontal_interfaces || options.discharge_vertical_interfaces {
        let verifier = default_contract_verifier(options.contract_timeout);
        let horizontal_families = if options.discharge_horizontal_interfaces {
            filtered_horizontal_families(
                &certificate.routing_witness,
                options.interface_filter.as_deref(),
            )
        } else {
            Vec::new()
        };
        let vertical_families = if options.discharge_vertical_interfaces {
            filtered_vertical_families(
                &certificate.routing_witness,
                options.interface_filter.as_deref(),
            )
        } else {
            Vec::new()
        };
        let mut horizontal_reports = Vec::new();
        let mut vertical_reports = Vec::new();

        for (idx, family) in horizontal_families.iter().enumerate() {
            println!(
                "discharging horizontal interface {}/{}: {}",
                idx + 1,
                horizontal_families.len(),
                horizontal_family_label(family)
            );
            io::stdout().flush()?;
            let report = discharge_horizontal_interface_family(
                &verifier,
                family,
                options.interface_max_candidates,
            )?;
            println!("  {}", report.render_summary_line());
            horizontal_reports.push(report);
        }

        for (idx, family) in vertical_families.iter().enumerate() {
            println!(
                "discharging vertical interface {}/{}: {}",
                idx + 1,
                vertical_families.len(),
                vertical_family_label(family)
            );
            io::stdout().flush()?;
            let report = discharge_vertical_interface_family(
                &verifier,
                family,
                options.interface_max_candidates,
            )?;
            println!("  {}", report.render_summary_line());
            vertical_reports.push(report);
        }

        let discharge = InterfaceLemmaSummary {
            horizontal: horizontal_reports,
            vertical: vertical_reports,
        };
        println!("{}", discharge.render_summary());
        if discharge.is_success()
            && options.discharge_horizontal_interfaces
            && options.discharge_vertical_interfaces
        {
            let basis = InterfaceBasisCertificate::from_summary(&discharge)?;
            match enumerate_router_interface_basis()
                .and_then(|router_basis| basis.covers_router_basis(&router_basis))
            {
                Ok(()) => {
                    basis.covers_witness(&certificate.routing_witness)?;
                    println!("{}", basis.render_summary());
                    if let Some(path) = options.save_interface_basis.as_ref() {
                        basis.save_json(path)?;
                        println!("saved interface basis certificate to {}", path.display());
                    }
                }
                Err(err) => {
                    println!(
                        "interface basis certificate is witness-complete but not router-basis-complete: {err}"
                    );
                }
            }
        }
        for report in discharge
            .horizontal
            .iter()
            .chain(discharge.vertical.iter())
            .filter(|report| !report.is_success())
        {
            println!("  {}", report.render_summary_line());
        }
    }

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

    if options.build_board {
        let mut board_options = BoardBuildOptions::default();
        board_options.allow_exact_placement_search = options.allow_exact_placement;
        if let Some(limit) = options.exact_placement_limit {
            board_options.exact_search_state_limit = Some(limit);
        }

        println!(
            "attempting published board assembly (experimental; exact fallback enabled={})",
            board_options.allow_exact_placement_search
        );
        match build_published_board_with_options(&construction, board_options) {
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
                match audit_published_board_motifs(&construction) {
                    Ok(audit) => println!("{}", audit.render_summary()),
                    Err(audit_err) => println!("board motif audit unavailable: {audit_err}"),
                }
            }
        }
    } else if options.audit_board {
        let audit = audit_published_board_motifs(&construction)?;
        println!("{}", audit.render_summary());
    }

    Ok(())
}
