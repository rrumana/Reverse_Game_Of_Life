use anyhow::{bail, Result};
use rev_gol::config::SolverBackend;
use rev_gol_proof::published::{
    published_root, search_composed_pattern_positions, CompositionSearchPiece, PublishedSpec,
};
use rev_gol_proof::verifier::{GadgetVerifier, GadgetVerifierConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    HorizontalConnector00,
    HorizontalConnector0ToMinus1SwTurn,
    VerticalSwTurnOverOrGate,
    VerticalSeTurnOverNwTurn,
}

impl Case {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            None | Some("horizontal-connector-00") => Ok(Self::HorizontalConnector00),
            Some("horizontal-connector-0-to--1-sw-turn") => {
                Ok(Self::HorizontalConnector0ToMinus1SwTurn)
            }
            Some("vertical-sw-turn-over-or-gate") => Ok(Self::VerticalSwTurnOverOrGate),
            Some("vertical-se-turn-over-nw-turn") => Ok(Self::VerticalSeTurnOverNwTurn),
            Some("--help") | Some("-h") => {
                print_usage();
                std::process::exit(0);
            }
            Some(other) => bail!("Unknown composition case '{other}'"),
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p rev_gol_proof --example check_composition -- [horizontal-connector-00|horizontal-connector-0-to--1-sw-turn|vertical-sw-turn-over-or-gate|vertical-se-turn-over-nw-turn]"
    );
}

fn relation_spec(
    name: &'static str,
    align: Option<(Option<i8>, Option<i8>, Option<i8>, Option<i8>)>,
    relation_wires: &'static str,
    relation_items: Vec<Vec<u8>>,
) -> PublishedSpec {
    PublishedSpec {
        path: "",
        name,
        size: None,
        align,
        charging: vec![],
        relation_wires,
        relation_items,
    }
}

fn main() -> Result<()> {
    let case = Case::parse()?;
    let root = published_root();
    let verifier = GadgetVerifier::new(GadgetVerifierConfig {
        backend: SolverBackend::Parkissat,
        num_threads: None,
        enable_preprocessing: true,
        verbosity: 0,
        timeout: None,
    });

    let (pieces, target_spec) = match case {
        Case::HorizontalConnector00 => (
            vec![
                CompositionSearchPiece::fixed("horizontal wire tile", 0, 0),
                CompositionSearchPiece::fixed("connector 0 to 0", 90, 0),
                CompositionSearchPiece::with_options(
                    "horizontal wire tile",
                    (258isize..=262).collect(),
                    (-2isize..=2).collect(),
                ),
            ],
            relation_spec(
                "horizontal wire through connector 0 to 0",
                Some((Some(0), None, Some(0), None)),
                "EW",
                vec![vec![0, 0], vec![1, 1]],
            ),
        ),
        Case::HorizontalConnector0ToMinus1SwTurn => (
            vec![
                CompositionSearchPiece::fixed("horizontal wire tile", 0, 0),
                CompositionSearchPiece::fixed("connector 0 to -1", 90, 0),
                CompositionSearchPiece::with_options(
                    "SW turn tile",
                    (257isize..=261).collect(),
                    (-2isize..=2).collect(),
                ),
            ],
            relation_spec(
                "horizontal wire through connector 0 to -1 into SW turn",
                Some((None, None, Some(0), Some(-1))),
                "SW",
                vec![vec![0, 0], vec![1, 1]],
            ),
        ),
        Case::VerticalSwTurnOverOrGate => (
            vec![
                CompositionSearchPiece::fixed("SW turn tile", 0, 0),
                CompositionSearchPiece::with_options(
                    "OR gate tile",
                    (0isize..=12).collect(),
                    (84isize..=90).collect(),
                ),
            ],
            relation_spec(
                "SW turn stacked over OR gate",
                Some((Some(0), None, Some(-1), Some(-1))),
                "EWS",
                vec![vec![0, 0, 0], vec![1, 0, 1], vec![1, 1, 0], vec![1, 1, 1]],
            ),
        ),
        Case::VerticalSeTurnOverNwTurn => (
            vec![
                CompositionSearchPiece::fixed("SE turn tile", 0, 0),
                CompositionSearchPiece::with_options(
                    "NW turn tile",
                    (0isize..=8).collect(),
                    (84isize..=90).collect(),
                ),
            ],
            relation_spec(
                "SE turn stacked over NW turn",
                Some((Some(-1), None, Some(1), None)),
                "EW",
                vec![vec![0, 0], vec![1, 1]],
            ),
        ),
    };

    let results = search_composed_pattern_positions(&verifier, &root, &pieces, &target_spec, 8)?;
    println!("found {} matching placements", results.len());
    for result in results {
        println!("candidate:");
        for (name, x, y) in result.placements {
            println!("  {name} @ ({x}, {y})");
        }
        println!(
            "  relation ok: allowed={} forbidden={}",
            result.report.relation_report.allowed_assignments_hold,
            result.report.relation_report.forbidden_assignments_hold
        );
    }

    Ok(())
}
