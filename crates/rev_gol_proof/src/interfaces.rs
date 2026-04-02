//! SAT-backed discharge of local routing interface families.
//!
//! This module takes the concrete interface families extracted from a [`RoutingWitness`]
//! and turns each family into a small composite gadget check. The key idea is local:
//! compose the relevant published patterns at a candidate placement, expose every
//! non-shared binary wire port under a distinct name, derive the expected external
//! relation by existentially quantifying the shared internal wires, and ask the SAT
//! verifier whether the composed pattern realizes exactly that relation.

use crate::board::{curated_horizontal_candidate_deltas, curated_vertical_candidate_deltas};
use crate::published::{
    compose_published_patterns, load_published_gadget, published_connector_specs,
    published_root, published_spec_named, relation_assignments, PublishedPattern, PublishedSpec,
};
use crate::routing::{HorizontalInterfaceFamily, RoutingWitness, VerticalInterfaceFamily};
use crate::verifier::{
    CellCoord, CellLiteral, GadgetPattern, GadgetVerifier, Port, PortAssignment,
    RelationCheckReport,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

const HORIZONTAL_DX_OFFSETS: [isize; 7] = [0, -3, 3, -6, 6, -9, 9];
const HORIZONTAL_DY_OFFSETS: [isize; 3] = [0, -3, 3];
const VERTICAL_DX_OFFSETS: [isize; 3] = [0, -3, 3];
const VERTICAL_DY_OFFSETS: [isize; 9] = [0, -3, 3, -6, 6, -9, 9, -12, 12];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceOrientation {
    Horizontal,
    Vertical,
}

impl InterfaceOrientation {
    fn as_str(self) -> &'static str {
        match self {
            InterfaceOrientation::Horizontal => "horizontal",
            InterfaceOrientation::Vertical => "vertical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceBasisEntry {
    pub orientation: InterfaceOrientation,
    pub label: String,
    pub witness_count: usize,
    pub external_ports: Vec<String>,
    pub expected_allowed_rows: usize,
    pub candidate_count: usize,
}

impl InterfaceBasisEntry {
    fn from_report(report: &InterfaceLemmaReport) -> Result<Self> {
        anyhow::ensure!(
            report.is_success(),
            "Cannot build an interface basis entry from failed report '{}'",
            report.label
        );
        Ok(Self {
            orientation: report.orientation,
            label: report.label.clone(),
            witness_count: report.count,
            external_ports: report.external_ports.clone(),
            expected_allowed_rows: report.expected_allowed_rows,
            candidate_count: report.candidate_count,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceBasisCertificate {
    pub horizontal: Vec<InterfaceBasisEntry>,
    pub vertical: Vec<InterfaceBasisEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterInterfaceBasis {
    pub horizontal: Vec<HorizontalInterfaceFamily>,
    pub vertical: Vec<VerticalInterfaceFamily>,
}

impl RouterInterfaceBasis {
    pub fn render_summary(&self) -> String {
        format!(
            "router_interface_basis_finite=true horizontal_families={} vertical_families={}",
            self.horizontal.len(),
            self.vertical.len()
        )
    }

    pub fn render_family_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(self.render_summary());
        lines.push(format!("horizontal router basis families: {}", self.horizontal.len()));
        for family in &self.horizontal {
            lines.push(format!("  {}", horizontal_family_label(family)));
        }
        lines.push(format!("vertical router basis families: {}", self.vertical.len()));
        for family in &self.vertical {
            lines.push(format!("  {}", vertical_family_label(family)));
        }
        lines.join("\n")
    }
}

impl InterfaceBasisCertificate {
    pub fn from_summary(summary: &InterfaceLemmaSummary) -> Result<Self> {
        Ok(Self {
            horizontal: summary
                .horizontal
                .iter()
                .map(InterfaceBasisEntry::from_report)
                .collect::<Result<Vec<_>>>()?,
            vertical: summary
                .vertical
                .iter()
                .map(InterfaceBasisEntry::from_report)
                .collect::<Result<Vec<_>>>()?,
        })
    }

    pub fn covers_witness(&self, witness: &RoutingWitness) -> Result<()> {
        self.covers_labels(
            witness
                .horizontal_families
                .iter()
                .map(horizontal_family_label)
                .collect(),
            witness
                .vertical_families
                .iter()
                .map(vertical_family_label)
                .collect(),
            "witness",
        )
    }

    pub fn covers_router_basis(&self, basis: &RouterInterfaceBasis) -> Result<()> {
        self.covers_labels(
            basis.horizontal.iter().map(horizontal_family_label).collect(),
            basis.vertical.iter().map(vertical_family_label).collect(),
            "router basis",
        )
    }

    fn covers_labels(
        &self,
        required_horizontal: Vec<String>,
        required_vertical: Vec<String>,
        scope: &str,
    ) -> Result<()> {
        let horizontal_labels = self
            .horizontal
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<HashSet<_>>();
        let vertical_labels = self
            .vertical
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<HashSet<_>>();

        let missing_horizontal = required_horizontal
            .into_iter()
            .filter(|label| !horizontal_labels.contains(label.as_str()))
            .collect::<Vec<_>>();
        let missing_vertical = required_vertical
            .into_iter()
            .filter(|label| !vertical_labels.contains(label.as_str()))
            .collect::<Vec<_>>();

        anyhow::ensure!(
            missing_horizontal.is_empty() && missing_vertical.is_empty(),
            "Interface basis certificate is missing {} families: horizontal={:?} vertical={:?}",
            scope,
            missing_horizontal,
            missing_vertical
        );
        Ok(())
    }

    pub fn render_summary(&self) -> String {
        format!(
            "interface_basis_certificate_valid=true horizontal_entries={} vertical_entries={}",
            self.horizontal.len(),
            self.vertical.len()
        )
    }

    pub fn save_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path_ref = path.as_ref();
        let text = serde_json::to_string_pretty(self)
            .context("Failed to serialize interface basis certificate")?;
        std::fs::write(path_ref, text).with_context(|| {
            format!(
                "Failed to write interface basis certificate to {}",
                path_ref.display()
            )
        })
    }

    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let text = std::fs::read_to_string(path_ref).with_context(|| {
            format!(
                "Failed to read interface basis certificate from {}",
                path_ref.display()
            )
        })?;
        serde_json::from_str(&text).with_context(|| {
            format!(
                "Failed to parse interface basis certificate from {}",
                path_ref.display()
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfacePlacement {
    pub piece_origins: Vec<(String, isize, isize)>,
}

#[derive(Debug, Clone)]
pub struct InterfaceLemmaReport {
    pub orientation: InterfaceOrientation,
    pub label: String,
    pub count: usize,
    pub external_ports: Vec<String>,
    pub expected_allowed_rows: usize,
    pub candidate_count: usize,
    pub placement: Option<InterfacePlacement>,
    pub relation_report: Option<RelationCheckReport>,
    pub error: Option<String>,
}

impl InterfaceLemmaReport {
    pub fn is_success(&self) -> bool {
        self.relation_report.as_ref().is_some_and(|report| {
            report.allowed_assignments_hold && report.forbidden_assignments_hold
        })
    }

    pub fn render_summary_line(&self) -> String {
        format!(
            "{} [{}]: success={} count={} candidates={} external_ports={} expected_allowed_rows={}{}",
            self.label,
            self.orientation.as_str(),
            self.is_success(),
            self.count,
            self.candidate_count,
            self.external_ports.join(","),
            self.expected_allowed_rows,
            self.error
                .as_ref()
                .map(|error| format!(" error={error}"))
                .unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone)]
pub struct InterfaceLemmaSummary {
    pub horizontal: Vec<InterfaceLemmaReport>,
    pub vertical: Vec<InterfaceLemmaReport>,
}

impl InterfaceLemmaSummary {
    pub fn is_success(&self) -> bool {
        self.horizontal.iter().all(InterfaceLemmaReport::is_success)
            && self.vertical.iter().all(InterfaceLemmaReport::is_success)
    }

    pub fn render_summary(&self) -> String {
        format!(
            "interface_lemmas_valid={} horizontal_verified={}/{} vertical_verified={}/{}",
            self.is_success(),
            self.horizontal
                .iter()
                .filter(|report| report.is_success())
                .count(),
            self.horizontal.len(),
            self.vertical
                .iter()
                .filter(|report| report.is_success())
                .count(),
            self.vertical.len(),
        )
    }
}

pub fn discharge_routing_witness_interfaces(
    verifier: &GadgetVerifier,
    witness: &RoutingWitness,
    discharge_horizontal: bool,
    discharge_vertical: bool,
    filter: Option<&str>,
    max_candidates_per_family: Option<usize>,
) -> Result<InterfaceLemmaSummary> {
    let horizontal = if discharge_horizontal {
        witness
            .horizontal_families
            .iter()
            .filter(|family| family_matches_filter(&horizontal_label(family), filter))
            .map(|family| {
                discharge_horizontal_interface_family(verifier, family, max_candidates_per_family)
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let vertical = if discharge_vertical {
        witness
            .vertical_families
            .iter()
            .filter(|family| family_matches_filter(&vertical_label(family), filter))
            .map(|family| {
                discharge_vertical_interface_family(verifier, family, max_candidates_per_family)
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    Ok(InterfaceLemmaSummary {
        horizontal,
        vertical,
    })
}

pub fn filtered_horizontal_families<'a>(
    witness: &'a RoutingWitness,
    filter: Option<&str>,
) -> Vec<&'a HorizontalInterfaceFamily> {
    witness
        .horizontal_families
        .iter()
        .filter(|family| family_matches_filter(&horizontal_label(family), filter))
        .collect()
}

pub fn filtered_vertical_families<'a>(
    witness: &'a RoutingWitness,
    filter: Option<&str>,
) -> Vec<&'a VerticalInterfaceFamily> {
    witness
        .vertical_families
        .iter()
        .filter(|family| family_matches_filter(&vertical_label(family), filter))
        .collect()
}

pub fn enumerate_router_interface_basis() -> Result<RouterInterfaceBasis> {
    let connector_specs = published_connector_specs()
        .into_iter()
        .filter_map(|spec| {
            spec.align
                .and_then(|(east, _, west, _)| Some(((west?, east?), spec.name)))
        })
        .collect::<BTreeMap<_, _>>();
    let routing_profiles = routing_interface_profiles()?;
    let macro_profiles = macro_interface_profiles()?;
    let horizontal_wire = interface_profile("horizontal wire tile")?;
    let vertical_wire = interface_profile("vertical wire tile")?;

    let mut horizontal = BTreeMap::<(&'static str, &'static str, &'static str), ()>::new();
    let mut vertical = BTreeMap::<(&'static str, &'static str), ()>::new();

    for left in routing_profiles.iter().filter(|profile| profile.east_phase.is_some()) {
        for right in routing_profiles.iter().filter(|profile| profile.west_phase.is_some()) {
            let connector = connector_name_for_profiles(left, right, &connector_specs)?;
            horizontal.insert((left.spec_name, connector, right.spec_name), ());
        }
    }
    for left in macro_profiles.iter().filter(|profile| profile.east_phase.is_some()) {
        let connector = connector_name_for_profiles(left, &horizontal_wire, &connector_specs)?;
        horizontal.insert((left.spec_name, connector, horizontal_wire.spec_name), ());
    }
    for right in macro_profiles.iter().filter(|profile| profile.west_phase.is_some()) {
        let connector = connector_name_for_profiles(&horizontal_wire, right, &connector_specs)?;
        horizontal.insert((horizontal_wire.spec_name, connector, right.spec_name), ());
    }

    for top in routing_profiles.iter().filter(|profile| profile.south_phase.is_some()) {
        for bottom in routing_profiles.iter().filter(|profile| profile.north_phase.is_some()) {
            if !vertical_family_is_router_realisable(top, bottom) {
                continue;
            }
            vertical.insert((top.spec_name, bottom.spec_name), ());
        }
    }
    for top in macro_profiles.iter().filter(|profile| profile.south_phase.is_some()) {
        vertical.insert((top.spec_name, vertical_wire.spec_name), ());
    }
    for bottom in macro_profiles.iter().filter(|profile| profile.north_phase.is_some()) {
        vertical.insert((vertical_wire.spec_name, bottom.spec_name), ());
    }

    Ok(RouterInterfaceBasis {
        horizontal: horizontal
            .into_keys()
            .map(|(left_spec_name, connector_spec_name, right_spec_name)| {
                HorizontalInterfaceFamily {
                    left_spec_name,
                    connector_spec_name,
                    right_spec_name,
                    count: 1,
                }
            })
            .collect(),
        vertical: vertical
            .into_keys()
            .map(|(top_spec_name, bottom_spec_name)| VerticalInterfaceFamily {
                top_spec_name,
                bottom_spec_name,
                count: 1,
            })
            .collect(),
    })
}

pub fn horizontal_family_label(family: &HorizontalInterfaceFamily) -> String {
    horizontal_label(family)
}

pub fn vertical_family_label(family: &VerticalInterfaceFamily) -> String {
    vertical_label(family)
}

pub fn discharge_horizontal_interface_family(
    verifier: &GadgetVerifier,
    family: &HorizontalInterfaceFamily,
    max_candidates_per_family: Option<usize>,
) -> Result<InterfaceLemmaReport> {
    let left = LoadedPiece::load("left", family.left_spec_name)?;
    let connector = LoadedPiece::load("connector", family.connector_spec_name)?;
    let right = LoadedPiece::load("right", family.right_spec_name)?;

    let left_east = left.anchor("E")?;
    let connector_west = connector.anchor("W")?;
    let connector_east = connector.anchor("E")?;
    let right_west = right.anchor("W")?;
    let connector_origin = (
        left_east.x - connector_west.x,
        left_east.y - connector_west.y,
    );
    let default_right_origin = (
        connector_origin.0 + connector_east.x - right_west.x,
        connector_origin.1 + connector_east.y - right_west.y,
    );
    let expected = composite_relation(
        &[&left, &connector, &right],
        &[
            SharedPort::new("left", "E", "connector", "W"),
            SharedPort::new("connector", "E", "right", "W"),
        ],
    )?;
    let mut last_error = None;
    let mut checked = 0usize;

    for right_origin in horizontal_candidate_origins(
        family,
        (default_right_origin.0 as i32, default_right_origin.1 as i32),
    ) {
        if max_candidates_per_family.is_some_and(|limit| checked >= limit) {
            break;
        }
        checked += 1;
        let placement = InterfacePlacement {
            piece_origins: vec![
                ("left".to_string(), 0, 0),
                (
                    "connector".to_string(),
                    connector_origin.0,
                    connector_origin.1,
                ),
                ("right".to_string(), right_origin.0, right_origin.1),
            ],
        };
        match verify_composite_family(
            verifier,
            format!("horizontal:{}", horizontal_label(family)),
            &[&left, &connector, &right],
            &placement,
            &[
                SharedPort::new("left", "E", "connector", "W"),
                SharedPort::new("connector", "E", "right", "W"),
            ],
            &expected,
        ) {
            Ok(report) if report.allowed_assignments_hold && report.forbidden_assignments_hold => {
                return Ok(InterfaceLemmaReport {
                    orientation: InterfaceOrientation::Horizontal,
                    label: horizontal_label(family),
                    count: family.count,
                    external_ports: expected.external_ports.clone(),
                    expected_allowed_rows: expected.allowed.len(),
                    candidate_count: checked,
                    placement: Some(placement),
                    relation_report: Some(report),
                    error: None,
                });
            }
            Ok(report) => {
                last_error = Some(format!(
                    "relation mismatch: allowed_ok={} forbidden_ok={}",
                    report.allowed_assignments_hold, report.forbidden_assignments_hold
                ));
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Ok(InterfaceLemmaReport {
        orientation: InterfaceOrientation::Horizontal,
        label: horizontal_label(family),
        count: family.count,
        external_ports: expected.external_ports,
        expected_allowed_rows: expected.allowed.len(),
        candidate_count: checked,
        placement: None,
        relation_report: None,
        error: last_error.or_else(|| Some("No successful candidate found".to_string())),
    })
}

pub fn discharge_vertical_interface_family(
    verifier: &GadgetVerifier,
    family: &VerticalInterfaceFamily,
    max_candidates_per_family: Option<usize>,
) -> Result<InterfaceLemmaReport> {
    let top = LoadedPiece::load("top", family.top_spec_name)?;
    let bottom = LoadedPiece::load("bottom", family.bottom_spec_name)?;

    let top_south = top.anchor("S")?;
    let bottom_north = bottom.anchor("N")?;
    let default_bottom_origin = (top_south.x - bottom_north.x, top_south.y - bottom_north.y);
    let expected = composite_relation(
        &[&top, &bottom],
        &[SharedPort::new("top", "S", "bottom", "N")],
    )?;
    let mut last_error = None;
    let mut checked = 0usize;

    for bottom_origin in vertical_candidate_origins(
        family,
        (
            default_bottom_origin.0 as i32,
            default_bottom_origin.1 as i32,
        ),
    ) {
        if max_candidates_per_family.is_some_and(|limit| checked >= limit) {
            break;
        }
        checked += 1;
        let placement = InterfacePlacement {
            piece_origins: vec![
                ("top".to_string(), 0, 0),
                ("bottom".to_string(), bottom_origin.0, bottom_origin.1),
            ],
        };
        match verify_composite_family(
            verifier,
            format!("vertical:{}", vertical_label(family)),
            &[&top, &bottom],
            &placement,
            &[SharedPort::new("top", "S", "bottom", "N")],
            &expected,
        ) {
            Ok(report) if report.allowed_assignments_hold && report.forbidden_assignments_hold => {
                return Ok(InterfaceLemmaReport {
                    orientation: InterfaceOrientation::Vertical,
                    label: vertical_label(family),
                    count: family.count,
                    external_ports: expected.external_ports.clone(),
                    expected_allowed_rows: expected.allowed.len(),
                    candidate_count: checked,
                    placement: Some(placement),
                    relation_report: Some(report),
                    error: None,
                });
            }
            Ok(report) => {
                last_error = Some(format!(
                    "relation mismatch: allowed_ok={} forbidden_ok={}",
                    report.allowed_assignments_hold, report.forbidden_assignments_hold
                ));
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Ok(InterfaceLemmaReport {
        orientation: InterfaceOrientation::Vertical,
        label: vertical_label(family),
        count: family.count,
        external_ports: expected.external_ports,
        expected_allowed_rows: expected.allowed.len(),
        candidate_count: checked,
        placement: None,
        relation_report: None,
        error: last_error.or_else(|| Some("No successful candidate found".to_string())),
    })
}

fn verify_composite_family(
    verifier: &GadgetVerifier,
    name: String,
    pieces: &[&LoadedPiece],
    placement: &InterfacePlacement,
    shared_ports: &[SharedPort],
    expected: &CompositeRelation,
) -> Result<RelationCheckReport> {
    let gadget = build_composite_gadget(name, pieces, placement, shared_ports)?;
    verifier.verify_relation(&gadget, &expected.allowed, &expected.forbidden)
}

fn build_composite_gadget(
    name: String,
    pieces: &[&LoadedPiece],
    placement: &InterfacePlacement,
    shared_ports: &[SharedPort],
) -> Result<GadgetPattern> {
    let placements = placement_map(placement)?;
    let raw_placements = pieces
        .iter()
        .map(|piece| {
            let &(x, y) = placements
                .get(piece.role)
                .with_context(|| format!("Missing placement for piece '{}'", piece.role))?;
            Ok((&piece.pattern, x, y))
        })
        .collect::<Result<Vec<_>>>()?;
    let composed = compose_published_patterns(&raw_placements)?;
    let min_x = raw_placements
        .iter()
        .map(|(_, x, _)| *x)
        .min()
        .context("Missing minimum x for interface composition")?;
    let min_y = raw_placements
        .iter()
        .map(|(_, _, y)| *y)
        .min()
        .context("Missing minimum y for interface composition")?;
    let target = composed.to_target_grid()?;

    let shared = shared_port_names(shared_ports);
    let mut gadget = GadgetPattern::new(name, target);
    let mut base_literals = HashSet::new();

    for piece in pieces {
        let &(origin_x, origin_y) = placements
            .get(piece.role)
            .with_context(|| format!("Missing placement for piece '{}'", piece.role))?;
        let translated_x = origin_x - min_x;
        let translated_y = origin_y - min_y;

        for port in &piece.gadget.ports {
            if shared.contains(&(piece.role.to_string(), port.name.clone())) {
                continue;
            }

            let renamed = rename_port(piece.role, &port.name);
            let translated_port =
                port.states
                    .iter()
                    .fold(Port::new(renamed), |port_out, (state, literals)| {
                        port_out.with_state(
                            state.clone(),
                            literals
                                .iter()
                                .map(|literal| {
                                    translate_literal(*literal, translated_x, translated_y)
                                })
                                .collect::<Vec<_>>(),
                        )
                    });
            gadget = gadget.with_port(translated_port);
        }

        for literal in &piece.gadget.base_predecessor_literals {
            base_literals.insert(translate_literal(*literal, translated_x, translated_y));
        }
    }

    let mut base_predecessor_literals = base_literals.into_iter().collect::<Vec<_>>();
    base_predecessor_literals.sort_by_key(|literal| literal.coord);
    Ok(gadget.with_base_predecessor_literals(base_predecessor_literals))
}

fn composite_relation(
    pieces: &[&LoadedPiece],
    shared_ports: &[SharedPort],
) -> Result<CompositeRelation> {
    let shared_lookup = shared_port_names(shared_ports);
    let component_rows = pieces
        .iter()
        .copied()
        .map(|piece| {
            Ok((
                piece,
                relation_assignments(&piece.spec)
                    .0
                    .into_iter()
                    .map(|assignment| assignment.states)
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut allowed = BTreeSet::<BTreeMap<String, String>>::new();
    let mut current = Vec::<(&LoadedPiece, &BTreeMap<String, String>)>::new();
    enumerate_composite_rows(&component_rows, shared_ports, &mut current, &mut allowed);

    let external_ports = pieces
        .iter()
        .flat_map(|piece| piece.relation_ports())
        .filter_map(|(role, port_name)| {
            if shared_lookup.contains(&(role.to_string(), port_name.to_string())) {
                None
            } else {
                Some(rename_port(role, port_name))
            }
        })
        .collect::<Vec<_>>();

    let allowed_assignments = allowed
        .into_iter()
        .map(|states| PortAssignment { states })
        .collect::<Vec<_>>();
    let forbidden_assignments = complement_assignments(&external_ports, &allowed_assignments);

    Ok(CompositeRelation {
        external_ports,
        allowed: allowed_assignments,
        forbidden: forbidden_assignments,
    })
}

fn enumerate_composite_rows<'a>(
    component_rows: &'a [(&'a LoadedPiece, Vec<BTreeMap<String, String>>)],
    shared_ports: &[SharedPort],
    current: &mut Vec<(&'a LoadedPiece, &'a BTreeMap<String, String>)>,
    allowed: &mut BTreeSet<BTreeMap<String, String>>,
) {
    if current.len() == component_rows.len() {
        if !rows_respect_shared_ports(current, shared_ports) {
            return;
        }

        let states = current
            .iter()
            .flat_map(|(piece, assignment)| {
                piece.relation_ports().filter_map(|(role, port_name)| {
                    if shared_ports.iter().any(|shared| {
                        (shared.left_role == role && shared.left_port == port_name)
                            || (shared.right_role == role && shared.right_port == port_name)
                    }) {
                        None
                    } else {
                        Some((rename_port(role, port_name), assignment[port_name].clone()))
                    }
                })
            })
            .collect::<BTreeMap<_, _>>();
        allowed.insert(states);
        return;
    }

    let (piece, rows) = &component_rows[current.len()];
    for row in rows {
        current.push((piece, row));
        enumerate_composite_rows(component_rows, shared_ports, current, allowed);
        current.pop();
    }
}

fn rows_respect_shared_ports(
    current: &[(&LoadedPiece, &BTreeMap<String, String>)],
    shared_ports: &[SharedPort],
) -> bool {
    for shared in shared_ports {
        let Some(left_state) = current
            .iter()
            .find(|(piece, _)| piece.role == shared.left_role)
            .and_then(|(_, assignment)| assignment.get(shared.left_port))
        else {
            return false;
        };
        let Some(right_state) = current
            .iter()
            .find(|(piece, _)| piece.role == shared.right_role)
            .and_then(|(_, assignment)| assignment.get(shared.right_port))
        else {
            return false;
        };
        if left_state != right_state {
            return false;
        }
    }
    true
}

fn complement_assignments(
    external_ports: &[String],
    allowed_assignments: &[PortAssignment],
) -> Vec<PortAssignment> {
    let allowed = allowed_assignments
        .iter()
        .map(|assignment| assignment.states.clone())
        .collect::<BTreeSet<_>>();
    let mut forbidden = Vec::new();
    let total = 1usize << external_ports.len();
    for mask in 0..total {
        let mut assignment = PortAssignment::new();
        for (idx, port_name) in external_ports.iter().enumerate() {
            assignment = assignment.with_state(
                port_name.clone(),
                if (mask & (1usize << idx)) != 0 {
                    "1"
                } else {
                    "0"
                },
            );
        }
        if !allowed.contains(&assignment.states) {
            forbidden.push(assignment);
        }
    }
    forbidden
}

fn placement_map(placement: &InterfacePlacement) -> Result<BTreeMap<&str, (isize, isize)>> {
    let mut placements = BTreeMap::new();
    for (piece, x, y) in &placement.piece_origins {
        let key = piece.as_str();
        if placements.insert(key, (*x, *y)).is_some() {
            anyhow::bail!("Duplicate placement for piece '{piece}'");
        }
    }
    Ok(placements)
}

fn shared_port_names(shared_ports: &[SharedPort]) -> HashSet<(String, String)> {
    shared_ports
        .iter()
        .flat_map(|shared| {
            [
                (shared.left_role.to_string(), shared.left_port.to_string()),
                (shared.right_role.to_string(), shared.right_port.to_string()),
            ]
        })
        .collect()
}

fn rename_port(role: &str, port_name: &str) -> String {
    format!("{role}_{port_name}")
}

fn translate_literal(literal: CellLiteral, dx: isize, dy: isize) -> CellLiteral {
    CellLiteral {
        coord: CellCoord::new(literal.coord.x + dx, literal.coord.y + dy),
        alive: literal.alive,
    }
}

fn family_matches_filter(label: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| label.contains(filter))
}

fn horizontal_candidate_origins(
    family: &HorizontalInterfaceFamily,
    default_right_origin: (i32, i32),
) -> Vec<(isize, isize)> {
    let mut candidates = curated_horizontal_candidate_deltas(
        family.left_spec_name,
        family.connector_spec_name,
        family.right_spec_name,
    )
    .into_iter()
    .chain(HORIZONTAL_DX_OFFSETS.into_iter().flat_map(|dx| {
        HORIZONTAL_DY_OFFSETS.into_iter().map(move |dy| {
            (
                default_right_origin.0 + dx as i32,
                default_right_origin.1 + dy as i32,
            )
        })
    }))
    .map(|(x, y)| (x as isize, y as isize))
    .collect::<Vec<_>>();
    push_unique_origin(
        &mut candidates,
        (
            default_right_origin.0 as isize,
            default_right_origin.1 as isize,
        ),
    );
    candidates
}

fn vertical_candidate_origins(
    family: &VerticalInterfaceFamily,
    default_bottom_origin: (i32, i32),
) -> Vec<(isize, isize)> {
    let mut candidates =
        curated_vertical_candidate_deltas(family.top_spec_name, family.bottom_spec_name)
            .into_iter()
            .chain(VERTICAL_DX_OFFSETS.into_iter().flat_map(|dx| {
                VERTICAL_DY_OFFSETS.into_iter().map(move |dy| {
                    (
                        default_bottom_origin.0 + dx as i32,
                        default_bottom_origin.1 + dy as i32,
                    )
                })
            }))
            .map(|(x, y)| (x as isize, y as isize))
            .collect::<Vec<_>>();
    push_unique_origin(
        &mut candidates,
        (
            default_bottom_origin.0 as isize,
            default_bottom_origin.1 as isize,
        ),
    );
    candidates
}

fn push_unique_origin(candidates: &mut Vec<(isize, isize)>, candidate: (isize, isize)) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn horizontal_label(family: &HorizontalInterfaceFamily) -> String {
    format!(
        "{} --{}--> {}",
        family.left_spec_name, family.connector_spec_name, family.right_spec_name
    )
}

fn vertical_label(family: &VerticalInterfaceFamily) -> String {
    format!("{} / {}", family.top_spec_name, family.bottom_spec_name)
}

#[derive(Debug, Clone, Copy)]
struct InterfaceProfile {
    spec_name: &'static str,
    east_phase: Option<i8>,
    north_phase: Option<i8>,
    west_phase: Option<i8>,
    south_phase: Option<i8>,
}

fn interface_profile(spec_name: &'static str) -> Result<InterfaceProfile> {
    let spec =
        published_spec_named(spec_name).with_context(|| format!("Unknown published spec '{spec_name}'"))?;
    let (east_phase, north_phase, west_phase, south_phase) = match spec.align {
        Some(align) => align,
        None => {
            let pattern_path = published_root().join(spec.path);
            PublishedPattern::from_csv_file(&pattern_path)
                .with_context(|| {
                    format!(
                        "Failed to load published pattern '{}' for interface-basis alignment fallback",
                        spec_name
                    )
                })?
                .phase_alignment()
        }
    };
    Ok(InterfaceProfile {
        spec_name,
        east_phase,
        north_phase,
        west_phase,
        south_phase,
    })
}

fn routing_interface_profiles() -> Result<Vec<InterfaceProfile>> {
    [
        "horizontal wire tile",
        "vertical wire tile",
        "NE turn tile",
        "NW turn tile",
        "SW turn tile",
        "SE turn tile",
        "crossing tile",
    ]
    .into_iter()
    .map(interface_profile)
    .collect()
}

fn macro_interface_profiles() -> Result<Vec<InterfaceProfile>> {
    ["NOT gate tile", "OR gate tile", "splitter tile", "enforcer gadget"]
        .into_iter()
        .map(interface_profile)
        .collect()
}

fn connector_name_for_profiles(
    left: &InterfaceProfile,
    right: &InterfaceProfile,
    connector_specs: &BTreeMap<(i8, i8), &'static str>,
) -> Result<&'static str> {
    let west_phase = left
        .east_phase
        .with_context(|| format!("Spec '{}' has no east phase", left.spec_name))?;
    let east_phase = right
        .west_phase
        .with_context(|| format!("Spec '{}' has no west phase", right.spec_name))?;
    connector_specs
        .get(&(west_phase, east_phase))
        .copied()
        .with_context(|| {
            format!(
                "Missing horizontal connector for {} east phase {} to {} west phase {}",
                left.spec_name, west_phase, right.spec_name, east_phase
            )
        })
}

fn vertical_family_is_router_realisable(
    top: &InterfaceProfile,
    bottom: &InterfaceProfile,
) -> bool {
    // The deterministic router only overlays horizontal lanes on rows separated by at least 3,
    // so a single vertical column cannot contain crossings on two consecutive cells.
    !(top.spec_name == "crossing tile" && bottom.spec_name == "crossing tile")
}

#[derive(Debug, Clone, Copy)]
struct SharedPort {
    left_role: &'static str,
    left_port: &'static str,
    right_role: &'static str,
    right_port: &'static str,
}

impl SharedPort {
    const fn new(
        left_role: &'static str,
        left_port: &'static str,
        right_role: &'static str,
        right_port: &'static str,
    ) -> Self {
        Self {
            left_role,
            left_port,
            right_role,
            right_port,
        }
    }
}

#[derive(Debug, Clone)]
struct CompositeRelation {
    external_ports: Vec<String>,
    allowed: Vec<PortAssignment>,
    forbidden: Vec<PortAssignment>,
}

#[derive(Debug, Clone)]
struct LoadedPiece {
    role: &'static str,
    spec: PublishedSpec,
    pattern: PublishedPattern,
    gadget: GadgetPattern,
}

impl LoadedPiece {
    fn load(role: &'static str, spec_name: &'static str) -> Result<Self> {
        let root = published_root();
        let spec = published_spec_named(spec_name)
            .with_context(|| format!("Unknown published spec '{spec_name}'"))?;
        let (pattern, gadget) = load_published_gadget(&root, &spec)?;
        Ok(Self {
            role,
            spec,
            pattern,
            gadget,
        })
    }

    fn anchor(&self, port_name: &str) -> Result<CellCoord> {
        let anchors = self.pattern.find_wires();
        match port_name {
            "E" => anchors.east,
            "N" => anchors.north,
            "W" => anchors.west,
            "S" => anchors.south,
            _ => None,
        }
        .with_context(|| {
            format!(
                "Published spec '{}' is missing anchor for {}",
                self.spec.name, port_name
            )
        })
    }

    fn relation_ports(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.spec
            .relation_wires
            .chars()
            .map(|port| {
                (
                    self.role,
                    match port {
                        'E' => "E",
                        'N' => "N",
                        'W' => "W",
                        'S' => "S",
                        _ => "",
                    },
                )
            })
            .filter(|(_, port_name)| !port_name.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;
    use crate::routing::{
        construct_routing_witness, HorizontalInterfaceFamily, VerticalInterfaceFamily,
    };

    #[test]
    fn test_symbolic_horizontal_wire_family_relation() {
        let family = HorizontalInterfaceFamily {
            left_spec_name: "horizontal wire tile",
            connector_spec_name: "connector 0 to 0",
            right_spec_name: "horizontal wire tile",
            count: 1,
        };
        let left = LoadedPiece::load("left", family.left_spec_name).unwrap();
        let connector = LoadedPiece::load("connector", family.connector_spec_name).unwrap();
        let right = LoadedPiece::load("right", family.right_spec_name).unwrap();
        let relation = composite_relation(
            &[&left, &connector, &right],
            &[
                SharedPort::new("left", "E", "connector", "W"),
                SharedPort::new("connector", "E", "right", "W"),
            ],
        )
        .unwrap();
        assert_eq!(
            relation.external_ports,
            vec!["left_W".to_string(), "right_E".to_string()]
        );
        assert_eq!(relation.allowed.len(), 2);
        assert_eq!(relation.forbidden.len(), 2);
    }

    #[test]
    fn test_symbolic_vertical_wire_family_relation() {
        let family = VerticalInterfaceFamily {
            top_spec_name: "vertical wire tile",
            bottom_spec_name: "vertical wire tile",
            count: 1,
        };
        let top = LoadedPiece::load("top", family.top_spec_name).unwrap();
        let bottom = LoadedPiece::load("bottom", family.bottom_spec_name).unwrap();
        let relation = composite_relation(
            &[&top, &bottom],
            &[SharedPort::new("top", "S", "bottom", "N")],
        )
        .unwrap();
        assert_eq!(
            relation.external_ports,
            vec!["top_N".to_string(), "bottom_S".to_string()]
        );
        assert_eq!(relation.allowed.len(), 2);
        assert_eq!(relation.forbidden.len(), 2);
    }

    #[test]
    #[ignore = "SAT-backed interface discharge is available but intentionally expensive"]
    fn test_discharge_horizontal_wire_family() {
        let verifier = crate::contracts::default_contract_verifier(None);
        let family = HorizontalInterfaceFamily {
            left_spec_name: "horizontal wire tile",
            connector_spec_name: "connector 0 to 0",
            right_spec_name: "horizontal wire tile",
            count: 1,
        };
        let report = discharge_horizontal_interface_family(&verifier, &family, None).unwrap();
        assert!(report.is_success(), "{}", report.render_summary_line());
    }

    #[test]
    fn test_interface_basis_certificate_covers_small_witness() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let witness = construct_routing_witness(&construction).unwrap();
        let basis = InterfaceBasisCertificate {
            horizontal: witness
                .horizontal_families
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: InterfaceOrientation::Horizontal,
                    label: horizontal_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
            vertical: witness
                .vertical_families
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: InterfaceOrientation::Vertical,
                    label: vertical_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
        };
        basis.covers_witness(&witness).unwrap();
    }

    #[test]
    fn test_router_interface_basis_covers_small_witness() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let witness = construct_routing_witness(&construction).unwrap();
        let basis = enumerate_router_interface_basis().unwrap();
        let certificate = InterfaceBasisCertificate {
            horizontal: basis
                .horizontal
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: InterfaceOrientation::Horizontal,
                    label: horizontal_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
            vertical: basis
                .vertical
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: InterfaceOrientation::Vertical,
                    label: vertical_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
        };
        certificate.covers_router_basis(&basis).unwrap();
        certificate.covers_witness(&witness).unwrap();
    }

    #[test]
    fn test_router_interface_basis_excludes_unrealisable_crossing_stack() {
        let basis = enumerate_router_interface_basis().unwrap();
        assert!(!basis
            .vertical
            .iter()
            .any(|family| family.top_spec_name == "crossing tile"
                && family.bottom_spec_name == "crossing tile"));
    }
}
