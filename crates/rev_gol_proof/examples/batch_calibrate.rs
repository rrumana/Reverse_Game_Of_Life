use anyhow::{bail, Context, Result};
use rev_gol::config::SolverBackend;
use rev_gol_proof::board::{
    audit_published_board_motifs, HorizontalMotifFamily, VerticalMotifFamily,
};
use rev_gol_proof::compiler::ConstructionCompiler;
use rev_gol_proof::dimacs::parse_dimacs_file;
use rev_gol_proof::published::{
    compose_published_patterns, load_published_gadget, published_root, published_spec_named,
    CompositionSearchPiece, CompositionSearchResult, PublishedPattern, PublishedSpec,
    PublishedVerificationReport,
};
use rev_gol_proof::verifier::{
    CellCoord, CellLiteral, GadgetPattern, GadgetVerifier, GadgetVerifierConfig, Port,
};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Options {
    input: PathBuf,
    jobs: usize,
    threads_per_job: usize,
    max_results: usize,
    horizontal_x_margin: isize,
    horizontal_y_margin: isize,
    vertical_x_margin: isize,
    vertical_y_margin: isize,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut input = None;
        let mut jobs = 4usize;
        let mut threads_per_job = 1usize;
        let mut max_results = 8usize;
        let mut horizontal_x_margin = 4isize;
        let mut horizontal_y_margin = 2isize;
        let mut vertical_x_margin = 12isize;
        let mut vertical_y_margin = 6isize;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--input" => {
                    input = Some(PathBuf::from(
                        args.next().context("--input requires a path")?,
                    ));
                }
                "--jobs" => {
                    jobs = args
                        .next()
                        .context("--jobs requires a value")?
                        .parse()
                        .context("Invalid --jobs value")?;
                }
                "--threads-per-job" => {
                    threads_per_job = args
                        .next()
                        .context("--threads-per-job requires a value")?
                        .parse()
                        .context("Invalid --threads-per-job value")?;
                }
                "--max-results" => {
                    max_results = args
                        .next()
                        .context("--max-results requires a value")?
                        .parse()
                        .context("Invalid --max-results value")?;
                }
                "--horizontal-x-margin" => {
                    horizontal_x_margin = args
                        .next()
                        .context("--horizontal-x-margin requires a value")?
                        .parse()
                        .context("Invalid --horizontal-x-margin value")?;
                }
                "--horizontal-y-margin" => {
                    horizontal_y_margin = args
                        .next()
                        .context("--horizontal-y-margin requires a value")?
                        .parse()
                        .context("Invalid --horizontal-y-margin value")?;
                }
                "--vertical-x-margin" => {
                    vertical_x_margin = args
                        .next()
                        .context("--vertical-x-margin requires a value")?
                        .parse()
                        .context("Invalid --vertical-x-margin value")?;
                }
                "--vertical-y-margin" => {
                    vertical_y_margin = args
                        .next()
                        .context("--vertical-y-margin requires a value")?
                        .parse()
                        .context("Invalid --vertical-y-margin value")?;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => bail!("Unknown option '{arg}'"),
            }
        }

        Ok(Self {
            input: input.context("An input DIMACS file is required")?,
            jobs: jobs.max(1),
            threads_per_job: threads_per_job.max(1),
            max_results,
            horizontal_x_margin,
            horizontal_y_margin,
            vertical_x_margin,
            vertical_y_margin,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo run -p rev_gol_proof --example batch_calibrate -- --input <FILE> [--jobs N] [--threads-per-job N] [--max-results N]"
    );
}

#[derive(Debug, Clone)]
struct JobSpec {
    label: String,
    pieces: Vec<JobPieceSpec>,
    target_spec: GenericRelationSpec,
}

#[derive(Debug, Clone)]
struct JobPieceSpec {
    placement: CompositionSearchPiece,
    external_ports: Vec<JobExternalPort>,
}

#[derive(Debug, Clone)]
struct JobExternalPort {
    source_port: String,
    external_name: String,
}

#[derive(Debug, Clone)]
struct GenericRelationSpec {
    relation_ports: Vec<String>,
    relation_items: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
enum JobUnsupported {
    Horizontal {
        family: HorizontalMotifFamily,
        reason: String,
    },
    Vertical {
        family: VerticalMotifFamily,
        reason: String,
    },
}

#[derive(Debug)]
enum Event {
    Started {
        worker: usize,
        job_label: String,
        index: usize,
        total_jobs: usize,
        total_candidates: usize,
    },
    Progress {
        worker: usize,
        job_label: String,
        checked_candidates: usize,
        total_candidates: usize,
        matches_found: usize,
        current_candidate: Vec<(String, isize, isize)>,
    },
    Finished {
        worker: usize,
        job_label: String,
        index: usize,
        elapsed: Duration,
        results: Vec<CompositionSearchResult>,
    },
    Failed {
        worker: usize,
        job_label: String,
        index: usize,
        elapsed: Duration,
        error: String,
    },
}

#[derive(Debug, Clone)]
struct ActiveJobStatus {
    worker: usize,
    index: usize,
    total_candidates: usize,
    checked_candidates: usize,
    matches_found: usize,
    last_candidate: Vec<(String, isize, isize)>,
    started_at: Instant,
    updated_at: Instant,
}

#[derive(Debug, Clone)]
struct CompletedJob {
    index: usize,
    elapsed: Duration,
    results: Vec<CompositionSearchResult>,
}

#[derive(Debug, Clone)]
struct FailedJob {
    index: usize,
    elapsed: Duration,
    error: String,
}

#[derive(Debug, Clone)]
struct LoadedJobPiece {
    spec_name: String,
    pattern: PublishedPattern,
    gadget: GadgetPattern,
    x_values: Vec<isize>,
    y_values: Vec<isize>,
    external_ports: Vec<JobExternalPort>,
}

fn spec_relation_maps(spec: &PublishedSpec) -> Vec<BTreeMap<String, u8>> {
    let wires: Vec<String> = spec
        .relation_wires
        .chars()
        .map(|ch| ch.to_string())
        .collect();
    spec.relation_items
        .iter()
        .map(|bits| wires.iter().cloned().zip(bits.iter().copied()).collect())
        .collect()
}

fn named_relation_assignments(
    spec: &GenericRelationSpec,
) -> (Vec<rev_gol_proof::verifier::PortAssignment>, Vec<rev_gol_proof::verifier::PortAssignment>) {
    let allowed = spec
        .relation_items
        .iter()
        .map(|bits| {
            let mut assignment = rev_gol_proof::verifier::PortAssignment::new();
            for (port, bit) in spec.relation_ports.iter().zip(bits.iter()) {
                assignment = assignment.with_state(port.clone(), bit.to_string());
            }
            assignment
        })
        .collect::<Vec<_>>();

    let allowed_set = allowed
        .iter()
        .map(|assignment| assignment.states.clone())
        .collect::<HashSet<_>>();

    let mut forbidden = Vec::new();
    let total = 1usize << spec.relation_ports.len();
    for mask in 0..total {
        let mut assignment = rev_gol_proof::verifier::PortAssignment::new();
        for (i, port) in spec.relation_ports.iter().enumerate() {
            let bit = ((mask >> i) & 1) as u8;
            assignment = assignment.with_state(port.clone(), bit.to_string());
        }
        if !allowed_set.contains(&assignment.states) {
            forbidden.push(assignment);
        }
    }

    (allowed, forbidden)
}

fn compose_horizontal_target_spec(
    family: &HorizontalMotifFamily,
) -> Result<GenericRelationSpec> {
    let left = published_spec_named(&family.left_spec)
        .with_context(|| format!("Unknown published spec '{}'", family.left_spec))?;
    let connector = published_spec_named(&family.connector_name)
        .with_context(|| format!("Unknown published spec '{}'", family.connector_name))?;
    let right = published_spec_named(&family.right_spec)
        .with_context(|| format!("Unknown published spec '{}'", family.right_spec))?;

    let left_maps = spec_relation_maps(&left);
    let connector_maps = spec_relation_maps(&connector);
    let right_maps = spec_relation_maps(&right);
    let mut allowed = HashSet::<Vec<u8>>::new();
    let ordered_external = left
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'E')
        .map(|ch| format!("L:{ch}"))
        .chain(
            connector
                .relation_wires
                .chars()
                .filter(|&ch| ch != 'E' && ch != 'W')
                .map(|ch| format!("C:{ch}")),
        )
        .chain(
            right
                .relation_wires
                .chars()
                .filter(|&ch| ch != 'W')
                .map(|ch| format!("R:{ch}")),
        )
        .collect::<Vec<_>>();

    for left_map in &left_maps {
        for connector_map in &connector_maps {
            if left_map.get("E") != connector_map.get("W") {
                continue;
            }
            for right_map in &right_maps {
                if connector_map.get("E") != right_map.get("W") {
                    continue;
                }
                let mut row = Vec::with_capacity(ordered_external.len());
                for wire in &ordered_external {
                    let (map, source_wire) = wire
                        .split_once(':')
                        .with_context(|| format!("Malformed projected wire '{wire}'"))?;
                    let value = match map {
                        "L" => left_map.get(source_wire),
                        "C" => connector_map.get(source_wire),
                        "R" => right_map.get(source_wire),
                        _ => None,
                    }
                    .copied()
                    .with_context(|| format!("Missing projected wire '{wire}'"))?;
                    row.push(value);
                }
                allowed.insert(row);
            }
        }
    }

    let mut relation_items = allowed.into_iter().collect::<Vec<_>>();
    relation_items.sort();
    Ok(GenericRelationSpec {
        relation_ports: ordered_external,
        relation_items,
    })
}

fn compose_vertical_target_spec(family: &VerticalMotifFamily) -> Result<GenericRelationSpec> {
    let top = published_spec_named(&family.top_spec)
        .with_context(|| format!("Unknown published spec '{}'", family.top_spec))?;
    let bottom = published_spec_named(&family.bottom_spec)
        .with_context(|| format!("Unknown published spec '{}'", family.bottom_spec))?;

    let top_maps = spec_relation_maps(&top);
    let bottom_maps = spec_relation_maps(&bottom);
    let mut allowed = HashSet::<Vec<u8>>::new();
    let ordered_external = top
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'S')
        .map(|ch| format!("T:{ch}"))
        .chain(
            bottom
                .relation_wires
                .chars()
                .filter(|&ch| ch != 'N')
                .map(|ch| format!("B:{ch}")),
        )
        .collect::<Vec<_>>();

    for top_map in &top_maps {
        for bottom_map in &bottom_maps {
            if top_map.get("S") != bottom_map.get("N") {
                continue;
            }
            let mut row = Vec::with_capacity(ordered_external.len());
            for wire in &ordered_external {
                let (map, source_wire) = wire
                    .split_once(':')
                    .with_context(|| format!("Malformed projected wire '{wire}'"))?;
                let value = match map {
                    "T" => top_map.get(source_wire),
                    "B" => bottom_map.get(source_wire),
                    _ => None,
                }
                .copied()
                .with_context(|| format!("Missing projected wire '{wire}'"))?;
                row.push(value);
            }
            allowed.insert(row);
        }
    }

    let mut relation_items = allowed.into_iter().collect::<Vec<_>>();
    relation_items.sort();
    Ok(GenericRelationSpec {
        relation_ports: ordered_external,
        relation_items,
    })
}

fn anchor_positions(pattern: &PublishedPattern) -> Result<(Option<(isize, isize)>, Option<(isize, isize)>, Option<(isize, isize)>, Option<(isize, isize)>)> {
    let anchors = pattern.find_wires();
    Ok((
        anchors.east.map(|c| (c.x, c.y)),
        anchors.north.map(|c| (c.x, c.y)),
        anchors.west.map(|c| (c.x, c.y)),
        anchors.south.map(|c| (c.x, c.y)),
    ))
}

fn horizontal_job(
    root: &std::path::Path,
    family: &HorizontalMotifFamily,
    options: &Options,
) -> Result<Option<JobSpec>> {
    let target_spec = compose_horizontal_target_spec(family)?;
    let left_spec = published_spec_named(&family.left_spec).unwrap();
    let connector_spec = published_spec_named(&family.connector_name).unwrap();
    let right_spec = published_spec_named(&family.right_spec).unwrap();
    let (left_pattern, _) = load_published_gadget(root, &left_spec)?;
    let (connector_pattern, _) = load_published_gadget(root, &connector_spec)?;
    let (right_pattern, _) = load_published_gadget(root, &right_spec)?;
    let (connector_east, _, _, _) = anchor_positions(&connector_pattern)?;
    let (_, _, right_west, _) = anchor_positions(&right_pattern)?;
    let connector_east = connector_east.context("Missing connector east anchor")?;
    let right_west = right_west.context("Missing right west anchor")?;
    let predicted_x = 90 + connector_east.0 - right_west.0;
    let predicted_y = connector_east.1 - right_west.1;

    let _ = left_pattern;

    let left_ports = left_spec
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'E')
        .map(|ch| JobExternalPort {
            source_port: ch.to_string(),
            external_name: format!("L:{ch}"),
        })
        .collect::<Vec<_>>();
    let connector_ports = connector_spec
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'E' && ch != 'W')
        .map(|ch| JobExternalPort {
            source_port: ch.to_string(),
            external_name: format!("C:{ch}"),
        })
        .collect::<Vec<_>>();
    let right_ports = right_spec
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'W')
        .map(|ch| JobExternalPort {
            source_port: ch.to_string(),
            external_name: format!("R:{ch}"),
        })
        .collect::<Vec<_>>();

    Ok(Some(JobSpec {
        label: format!(
            "{} + {} + {}",
            family.left_spec, family.connector_name, family.right_spec
        ),
        pieces: vec![
            JobPieceSpec {
                placement: CompositionSearchPiece::fixed(&family.left_spec, 0, 0),
                external_ports: left_ports,
            },
            JobPieceSpec {
                placement: CompositionSearchPiece::fixed(&family.connector_name, 90, 0),
                external_ports: connector_ports,
            },
            JobPieceSpec {
                placement: CompositionSearchPiece::with_options(
                    &family.right_spec,
                    ((predicted_x - options.horizontal_x_margin)
                        ..=(predicted_x + options.horizontal_x_margin))
                        .collect(),
                    ((predicted_y - options.horizontal_y_margin)
                        ..=(predicted_y + options.horizontal_y_margin))
                        .collect(),
                ),
                external_ports: right_ports,
            },
        ],
        target_spec,
    }))
}

fn vertical_job(
    root: &std::path::Path,
    family: &VerticalMotifFamily,
    options: &Options,
) -> Result<Option<JobSpec>> {
    let target_spec = compose_vertical_target_spec(family)?;
    let top_spec = published_spec_named(&family.top_spec).unwrap();
    let bottom_spec = published_spec_named(&family.bottom_spec).unwrap();
    let (top_pattern, _) = load_published_gadget(root, &top_spec)?;
    let (bottom_pattern, _) = load_published_gadget(root, &bottom_spec)?;
    let (_, _, _, top_south) = anchor_positions(&top_pattern)?;
    let (_, bottom_north, _, _) = anchor_positions(&bottom_pattern)?;
    let top_south = top_south.context("Missing top south anchor")?;
    let bottom_north = bottom_north.context("Missing bottom north anchor")?;
    let predicted_x = top_south.0 - bottom_north.0;
    let predicted_y = top_south.1 - bottom_north.1;
    let top_ports = top_spec
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'S')
        .map(|ch| JobExternalPort {
            source_port: ch.to_string(),
            external_name: format!("T:{ch}"),
        })
        .collect::<Vec<_>>();
    let bottom_ports = bottom_spec
        .relation_wires
        .chars()
        .filter(|&ch| ch != 'N')
        .map(|ch| JobExternalPort {
            source_port: ch.to_string(),
            external_name: format!("B:{ch}"),
        })
        .collect::<Vec<_>>();

    Ok(Some(JobSpec {
        label: format!("{} / {}", family.top_spec, family.bottom_spec),
        pieces: vec![
            JobPieceSpec {
                placement: CompositionSearchPiece::fixed(&family.top_spec, 0, 0),
                external_ports: top_ports,
            },
            JobPieceSpec {
                placement: CompositionSearchPiece::with_options(
                    &family.bottom_spec,
                    ((predicted_x - options.vertical_x_margin)
                        ..=(predicted_x + options.vertical_x_margin))
                        .collect(),
                    ((predicted_y - options.vertical_y_margin)
                        ..=(predicted_y + options.vertical_y_margin))
                        .collect(),
                ),
                external_ports: bottom_ports,
            },
        ],
        target_spec,
    }))
}

fn translate_literal(literal: CellLiteral, dx: isize, dy: isize) -> CellLiteral {
    CellLiteral {
        coord: CellCoord::new(literal.coord.x + dx, literal.coord.y + dy),
        alive: literal.alive,
    }
}

fn translate_port(port: &Port, dx: isize, dy: isize) -> Port {
    let mut out = Port::new(port.name.clone());
    for (state_name, literals) in &port.states {
        out = out.with_state(
            state_name.clone(),
            literals
                .iter()
                .copied()
                .map(|literal| translate_literal(literal, dx, dy))
                .collect(),
        );
    }
    out
}

fn load_job_pieces(root: &std::path::Path, job: &JobSpec) -> Result<Vec<LoadedJobPiece>> {
    let mut loaded = Vec::new();
    for piece in &job.pieces {
        let spec = published_spec_named(&piece.placement.spec_name)
            .with_context(|| format!("Unknown published spec '{}'", piece.placement.spec_name))?;
        let (pattern, gadget) = load_published_gadget(root, &spec)?;
        loaded.push(LoadedJobPiece {
            spec_name: piece.placement.spec_name.clone(),
            pattern,
            gadget,
            x_values: piece.placement.x_values.clone(),
            y_values: piece.placement.y_values.clone(),
            external_ports: piece.external_ports.clone(),
        });
    }
    Ok(loaded)
}

fn build_composed_gadget(
    label: &str,
    loaded: &[(LoadedJobPiece, isize, isize)],
) -> Result<(PublishedPattern, GadgetPattern)> {
    let min_x = loaded
        .iter()
        .map(|(_, x, _)| *x)
        .min()
        .context("Missing min_x for composite gadget")?;
    let min_y = loaded
        .iter()
        .map(|(_, _, y)| *y)
        .min()
        .context("Missing min_y for composite gadget")?;

    let placements = loaded
        .iter()
        .map(|(piece, x, y)| (&piece.pattern, *x, *y))
        .collect::<Vec<_>>();
    let published = compose_published_patterns(&placements)?;
    let mut gadget = GadgetPattern::new(label, published.to_target_grid()?);
    let mut merged_ports = BTreeMap::<String, Port>::new();
    let mut merged_base = Vec::<CellLiteral>::new();

    for (piece, x, y) in loaded {
        let dx = x - min_x;
        let dy = y - min_y;
        for port_spec in &piece.external_ports {
            let port = piece
                .gadget
                .ports
                .iter()
                .find(|port| port.name == port_spec.source_port)
                .with_context(|| {
                    format!(
                        "Piece '{}' is missing external port '{}'",
                        piece.spec_name, port_spec.source_port
                    )
                })?;
            let mut translated = translate_port(port, dx, dy);
            translated.name = port_spec.external_name.clone();
            if merged_ports.contains_key(&port_spec.external_name) {
                anyhow::bail!(
                    "Composite gadget exposes duplicate external port '{}'",
                    port_spec.external_name
                );
            }
            merged_ports.insert(port_spec.external_name.clone(), translated);
        }
        merged_base.extend(
            piece.gadget
                .base_predecessor_literals
                .iter()
                .copied()
                .map(|literal| translate_literal(literal, dx, dy)),
        );
    }

    for (_, port) in merged_ports {
        gadget = gadget.with_port(port);
    }
    gadget = gadget.with_base_predecessor_literals(merged_base);
    Ok((published, gadget))
}

fn search_job_positions(
    verifier: &GadgetVerifier,
    root: &std::path::Path,
    job: &JobSpec,
    max_results: usize,
    on_progress: &mut dyn FnMut(usize, usize, usize, Vec<(String, isize, isize)>),
) -> Result<Vec<CompositionSearchResult>> {
    let loaded = load_job_pieces(root, job)?;
    let total_candidates = loaded.iter().fold(1usize, |acc, piece| {
        acc.saturating_mul(piece.x_values.len().saturating_mul(piece.y_values.len()))
    });
    let mut checked_candidates = 0usize;
    let mut results = Vec::new();
    let mut current = Vec::<(LoadedJobPiece, isize, isize)>::new();

    fn recurse(
        verifier: &GadgetVerifier,
        loaded: &[LoadedJobPiece],
        job: &JobSpec,
        max_results: usize,
        current: &mut Vec<(LoadedJobPiece, isize, isize)>,
        checked_candidates: &mut usize,
        total_candidates: usize,
        results: &mut Vec<CompositionSearchResult>,
        on_progress: &mut dyn FnMut(usize, usize, usize, Vec<(String, isize, isize)>),
    ) -> Result<()> {
        if results.len() >= max_results {
            return Ok(());
        }
        if current.len() == loaded.len() {
            *checked_candidates += 1;
            let current_candidate = current
                .iter()
                .map(|(piece, x, y)| (piece.spec_name.clone(), *x, *y))
                .collect::<Vec<_>>();
            on_progress(*checked_candidates, total_candidates, results.len(), current_candidate.clone());
            let (_published, gadget) =
                build_composed_gadget(&format!("search:{}", job.label), current)?;
            let (allowed, forbidden) = named_relation_assignments(&job.target_spec);
            let relation_report = verifier.verify_relation(&gadget, &allowed, &forbidden)?;
            let report = PublishedVerificationReport {
                size_matches: true,
                alignment_matches: true,
                relation_report,
                charging_reports: Vec::new(),
            };
            if report.is_success() {
                results.push(CompositionSearchResult {
                    placements: current_candidate.clone(),
                    report,
                });
                on_progress(*checked_candidates, total_candidates, results.len(), current_candidate);
            }
            return Ok(());
        }

        let piece = &loaded[current.len()];
        for &x in &piece.x_values {
            for &y in &piece.y_values {
                current.push((piece.clone(), x, y));
                recurse(
                    verifier,
                    loaded,
                    job,
                    max_results,
                    current,
                    checked_candidates,
                    total_candidates,
                    results,
                    on_progress,
                )?;
                current.pop();
                if results.len() >= max_results {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    recurse(
        verifier,
        &loaded,
        job,
        max_results,
        &mut current,
        &mut checked_candidates,
        total_candidates,
        &mut results,
        on_progress,
    )?;
    Ok(results)
}

fn main() -> Result<()> {
    let options = Options::parse()?;
    let root = published_root();
    let formula = parse_dimacs_file(&options.input)?;
    let construction = ConstructionCompiler::compile_cnf(&formula)?;
    let audit = audit_published_board_motifs(&construction)?;
    let available_parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let requested_parallelism = options.jobs.saturating_mul(options.threads_per_job);

    let mut jobs = Vec::new();
    let mut unsupported = Vec::new();

    for family in audit.unresolved_horizontal() {
        match horizontal_job(&root, family, &options) {
            Ok(Some(job)) => jobs.push(job),
            Ok(None) => unsupported.push(JobUnsupported::Horizontal {
                family: (*family).clone(),
                reason: "search job was not generated".to_string(),
            }),
            Err(err) => unsupported.push(JobUnsupported::Horizontal {
                family: (*family).clone(),
                reason: err.to_string(),
            }),
        }
    }
    for family in audit.unresolved_vertical() {
        match vertical_job(&root, family, &options) {
            Ok(Some(job)) => jobs.push(job),
            Ok(None) => unsupported.push(JobUnsupported::Vertical {
                family: (*family).clone(),
                reason: "search job was not generated".to_string(),
            }),
            Err(err) => unsupported.push(JobUnsupported::Vertical {
                family: (*family).clone(),
                reason: err.to_string(),
            }),
        }
    }

    println!("audit summary:");
    println!("{}", audit.render_summary());
    println!();
    println!(
        "batch calibration jobs: {} searchable, {} unsupported",
        jobs.len(),
        unsupported.len()
    );
    println!(
        "execution config: jobs={}, threads/job={}, requested solver threads={}, host parallelism={}",
        options.jobs,
        options.threads_per_job,
        requested_parallelism,
        available_parallelism
    );
    if requested_parallelism > available_parallelism {
        println!(
            "warning: requested solver parallelism exceeds host parallelism; this may reduce throughput"
        );
    }
    if requested_parallelism >= available_parallelism.saturating_mul(2) {
        println!(
            "warning: requested solver parallelism is at least 2x host parallelism; memory pressure may be significant"
        );
    }
    for item in &unsupported {
        match item {
            JobUnsupported::Horizontal { family, reason } => println!(
                "  unsupported horizontal: {} + {} + {} ({})",
                family.left_spec, family.connector_name, family.right_spec, reason
            ),
            JobUnsupported::Vertical { family, reason } => println!(
                "  unsupported vertical: {} / {} ({})",
                family.top_spec, family.bottom_spec, reason
            ),
        }
    }
    println!();

    let total_jobs = jobs.len();
    let queue = Arc::new(Mutex::new(
        jobs.into_iter().enumerate().collect::<VecDeque<_>>(),
    ));
    let (tx, rx) = mpsc::channel::<Event>();
    let started_at = Instant::now();

    for worker in 0..options.jobs {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        let root = root.clone();
        let max_results = options.max_results;
        let threads_per_job = options.threads_per_job;
        thread::spawn(move || {
            loop {
                let Some((index, job)) = queue.lock().unwrap().pop_front() else {
                    break;
                };

                let verifier = GadgetVerifier::new(GadgetVerifierConfig {
                    backend: SolverBackend::Parkissat,
                    num_threads: Some(threads_per_job),
                    enable_preprocessing: true,
                    verbosity: 0,
                    timeout: None,
                });
                let total_candidates = job.pieces.iter().fold(1usize, |acc, piece| {
                    acc.saturating_mul(
                        piece.placement
                            .x_values
                            .len()
                            .saturating_mul(piece.placement.y_values.len()),
                    )
                });
                let _ = tx.send(Event::Started {
                    worker,
                    job_label: job.label.clone(),
                    index: index + 1,
                    total_jobs,
                    total_candidates,
                });
                let job_started = Instant::now();
                let mut progress_callback = |checked_candidates: usize,
                                             total_candidates: usize,
                                             matches_found: usize,
                                             current_candidate: Vec<(String, isize, isize)>| {
                    let _ = tx.send(Event::Progress {
                        worker,
                        job_label: job.label.clone(),
                        checked_candidates,
                        total_candidates,
                        matches_found,
                        current_candidate,
                    });
                };

                match search_job_positions(&verifier, &root, &job, max_results, &mut progress_callback) {
                    Ok(results) => {
                        let _ = tx.send(Event::Finished {
                            worker,
                            job_label: job.label,
                            index: index + 1,
                            elapsed: job_started.elapsed(),
                            results,
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(Event::Failed {
                            worker,
                            job_label: job.label,
                            index: index + 1,
                            elapsed: job_started.elapsed(),
                            error: err.to_string(),
                        });
                    }
                }
            }
        });
    }
    drop(tx);

    let mut active = HashMap::<String, ActiveJobStatus>::new();
    let mut finished_jobs = 0usize;
    let mut failed_jobs = 0usize;
    let mut matched_jobs = 0usize;
    let mut last_snapshot = Instant::now();
    let mut completed = BTreeMap::<String, CompletedJob>::new();
    let mut failures = BTreeMap::<String, FailedJob>::new();

    while finished_jobs + failed_jobs < total_jobs {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(event) => match event {
                Event::Started {
                    worker,
                    job_label,
                    index,
                    total_jobs,
                    total_candidates,
                } => {
                    let now = Instant::now();
                    active.insert(
                        job_label.clone(),
                        ActiveJobStatus {
                            worker,
                            index,
                            total_candidates,
                            checked_candidates: 0,
                            matches_found: 0,
                            last_candidate: Vec::new(),
                            started_at: now,
                            updated_at: now,
                        },
                    );
                    println!(
                        "[start] worker {} job {}/{}: {} ({} candidates)",
                        worker, index, total_jobs, job_label, total_candidates
                    );
                }
                Event::Progress {
                    worker,
                    job_label,
                    checked_candidates,
                    total_candidates,
                    matches_found,
                    current_candidate,
                } => {
                    let now = Instant::now();
                    let status = active.entry(job_label.clone()).or_insert(ActiveJobStatus {
                        worker,
                        index: 0,
                        total_candidates,
                        checked_candidates: 0,
                        matches_found: 0,
                        last_candidate: Vec::new(),
                        started_at: now,
                        updated_at: now,
                    });
                    status.total_candidates = total_candidates;
                    status.checked_candidates = checked_candidates;
                    status.matches_found = matches_found;
                    status.last_candidate = current_candidate;
                    status.updated_at = now;
                }
                Event::Finished {
                    worker,
                    job_label,
                    index,
                    elapsed,
                    results,
                } => {
                    finished_jobs += 1;
                    if !results.is_empty() {
                        matched_jobs += 1;
                    }
                    active.remove(&job_label);
                    let matches = results.len();
                    completed.insert(
                        job_label.clone(),
                        CompletedJob {
                            index,
                            elapsed,
                            results,
                        },
                    );
                    println!(
                        "[done] worker {} job {}: {} in {:.1?} (matches={})",
                        worker, index, job_label, elapsed, matches
                    );
                }
                Event::Failed {
                    worker,
                    job_label,
                    index,
                    elapsed,
                    error,
                } => {
                    failed_jobs += 1;
                    active.remove(&job_label);
                    failures.insert(
                        job_label.clone(),
                        FailedJob {
                            index,
                            elapsed,
                            error: error.clone(),
                        },
                    );
                    println!(
                        "[fail] worker {} job {}: {} in {:.1?} ({})",
                        worker, index, job_label, elapsed, error
                    );
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if last_snapshot.elapsed() >= Duration::from_secs(60) {
            println!(
                "[progress] elapsed {:.1?}, finished {}, failed {}, active {}",
                started_at.elapsed(),
                finished_jobs,
                failed_jobs,
                active.len()
            );
            let mut statuses = active.iter().collect::<Vec<_>>();
            statuses.sort_by_key(|(_, status)| status.index);
            for (job_label, status) in statuses {
                println!(
                    "  worker {} job {}: {} {}/{} candidates, matches={}, running {:.1?}, last update {:.1?}",
                    status.worker,
                    status.index,
                    job_label,
                    status.checked_candidates,
                    status.total_candidates,
                    status.matches_found,
                    status.started_at.elapsed(),
                    status.updated_at.elapsed()
                );
                if !status.last_candidate.is_empty() {
                    let candidate = status
                        .last_candidate
                        .iter()
                        .map(|(name, x, y)| format!("{name}@({x},{y})"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!("    candidate: {candidate}");
                }
            }
            last_snapshot = Instant::now();
        }
    }

    println!(
        "batch complete in {:.1?}: finished={}, failed={}, matched_jobs={}, unsupported={}",
        started_at.elapsed(),
        finished_jobs,
        failed_jobs,
        matched_jobs,
        unsupported.len()
    );
    println!();
    println!("Final matched jobs:");
    for (label, job) in completed.iter().filter(|(_, job)| !job.results.is_empty()) {
        println!(
            "  [job {}] {} in {:.1?} with {} match(es)",
            job.index,
            label,
            job.elapsed,
            job.results.len()
        );
        for (result_index, result) in job.results.iter().enumerate() {
            let placements = result
                .placements
                .iter()
                .map(|(name, x, y)| format!("{name}@({x},{y})"))
                .collect::<Vec<_>>()
                .join(", ");
            println!("    match {}: {}", result_index + 1, placements);
        }
    }
    println!("Final zero-match jobs:");
    for (label, job) in completed.iter().filter(|(_, job)| job.results.is_empty()) {
        println!("  [job {}] {} in {:.1?}", job.index, label, job.elapsed);
    }
    println!("Final failed jobs:");
    for (label, job) in &failures {
        println!(
            "  [job {}] {} in {:.1?}: {}",
            job.index, label, job.elapsed, job.error
        );
    }
    println!("Final unsupported families:");
    for item in &unsupported {
        match item {
            JobUnsupported::Horizontal { family, reason } => println!(
                "  horizontal: {} + {} + {} ({})",
                family.left_spec, family.connector_name, family.right_spec, reason
            ),
            JobUnsupported::Vertical { family, reason } => println!(
                "  vertical: {} / {} ({})",
                family.top_spec, family.bottom_spec, reason
            ),
        }
    }

    Ok(())
}
