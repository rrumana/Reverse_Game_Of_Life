//! Symbolic certification of external-input boundary encodings.
//!
//! The compiler and routing witness already treat primary inputs as explicit external nets rather
//! than published gadgets. This module discharges that generalized infinite-board obligation by
//! checking that every external net enters the routed fabric through a distinct binary routing
//! port with exactly the identity relation expected by the wire protocol.

use crate::compiler::{CompiledConstruction, Endpoint, PortRef};
use crate::published::published_spec_named;
use crate::routing::{macro_port_dir, RouteDir, RoutingWitness};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputEncodingMode {
    Variable,
    ConstantZero,
    ConstantOne,
}

impl InputEncodingMode {
    fn as_str(self) -> &'static str {
        match self {
            InputEncodingMode::Variable => "variable",
            InputEncodingMode::ConstantZero => "const_0",
            InputEncodingMode::ConstantOne => "const_1",
        }
    }

    fn expected_boundary_values(self) -> BTreeSet<u8> {
        match self {
            InputEncodingMode::Variable => [0u8, 1u8].into_iter().collect(),
            InputEncodingMode::ConstantZero => [0u8].into_iter().collect(),
            InputEncodingMode::ConstantOne => [1u8].into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBoundaryEntry {
    pub variable: String,
    pub mode: InputEncodingMode,
    pub target: PortRef,
    pub target_spec_name: &'static str,
    pub target_dir: RouteDir,
    pub source_coord: (i32, i32),
    pub source_spec_name: &'static str,
    pub boundary_dir: RouteDir,
    pub routed_dir: RouteDir,
    pub supported_boundary_values: BTreeSet<u8>,
}

impl InputBoundaryEntry {
    pub fn family_label(&self) -> String {
        format!(
            "{} boundary --{}--> {} --{}--> routed fabric",
            self.mode.as_str(),
            self.boundary_dir.as_str(),
            self.source_spec_name,
            self.routed_dir.as_str(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBoundaryFamily {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBoundaryCertificate {
    pub entries: Vec<InputBoundaryEntry>,
    pub families: Vec<InputBoundaryFamily>,
    pub distinct_source_coords: bool,
    pub all_sources_binary_identity: bool,
    pub variable_sources_independent: bool,
}

impl InputBoundaryCertificate {
    pub fn is_complete(&self) -> bool {
        self.distinct_source_coords
            && self.all_sources_binary_identity
            && self.variable_sources_independent
    }

    pub fn render_summary(&self) -> String {
        let variable_entries = self
            .entries
            .iter()
            .filter(|entry| entry.mode == InputEncodingMode::Variable)
            .count();
        let constant_entries = self.entries.len().saturating_sub(variable_entries);
        format!(
            "input_boundary_encoding_complete={} input_entries={} input_families={} variable_inputs={} constant_inputs={}",
            self.is_complete(),
            self.entries.len(),
            self.families.len(),
            variable_entries,
            constant_entries,
        )
    }

    pub fn render_family_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("input boundary families: {}", self.families.len()));
        for family in &self.families {
            lines.push(format!("  {} x{}", family.label, family.count));
        }
        lines.join("\n")
    }
}

pub fn certify_input_boundary_encoding(
    construction: &CompiledConstruction,
    witness: &RoutingWitness,
) -> Result<InputBoundaryCertificate> {
    let route_cells_by_coord = witness
        .route_cells
        .iter()
        .map(|cell| (cell.coord, cell))
        .collect::<HashMap<_, _>>();
    let instances_by_id = construction
        .instances
        .iter()
        .map(|instance| (instance.id, instance))
        .collect::<HashMap<_, _>>();

    let mut entries = Vec::new();
    for path in &witness.net_paths {
        let variable = match &path.from {
            Endpoint::ExternalInput { variable } => variable.clone(),
            Endpoint::InstancePort(_) => continue,
        };

        let target_instance = instances_by_id
            .get(&path.to.instance)
            .with_context(|| format!("Missing target instance {:?}", path.to.instance))?;
        let target_spec_name = target_instance
            .kind
            .published_spec_name()
            .with_context(|| format!("Missing published spec for {:?}", target_instance.kind))?;
        let target_dir = macro_port_dir(&target_instance.kind, path.to.port)?;
        let source_coord = *path
            .cells
            .first()
            .with_context(|| format!("External input '{}' has an empty route path", variable))?;
        let next_coord = *path
            .cells
            .get(1)
            .with_context(|| format!("External input '{}' route path is too short", variable))?;
        let routed_dir = direction_between(source_coord, next_coord)?;
        let source_cell = route_cells_by_coord.get(&source_coord).with_context(|| {
            format!(
                "External input '{}' source cell {:?} is missing from the routing witness",
                variable, source_coord
            )
        })?;
        anyhow::ensure!(
            source_cell.dirs.len() == 2,
            "External input '{}' source cell {:?} should be a two-port routing piece, found {:?}",
            variable,
            source_coord,
            source_cell.dirs
        );
        anyhow::ensure!(
            source_cell.dirs.contains(&routed_dir),
            "External input '{}' source cell {:?} does not point into its routed path",
            variable,
            source_coord
        );

        let boundary_dir = source_cell
            .dirs
            .iter()
            .copied()
            .find(|dir| *dir != routed_dir)
            .context("Failed to determine external boundary direction")?;
        let projected_relation =
            projected_relation(source_cell.spec_name, boundary_dir, routed_dir)?;
        let expected_identity = [(0u8, 0u8), (1u8, 1u8)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        anyhow::ensure!(
            projected_relation == expected_identity,
            "External input '{}' enters through {} with projected relation {:?}, expected {:?}",
            variable,
            source_cell.spec_name,
            projected_relation,
            expected_identity
        );

        let mode = input_mode_for_variable(&variable)?;
        let supported_boundary_values = projected_relation
            .iter()
            .map(|(boundary, _)| *boundary)
            .collect::<BTreeSet<_>>();
        anyhow::ensure!(
            mode.expected_boundary_values().is_subset(&supported_boundary_values),
            "External input '{}' in mode {} is missing expected boundary values {:?}",
            variable,
            mode.as_str(),
            mode.expected_boundary_values()
        );

        entries.push(InputBoundaryEntry {
            variable,
            mode,
            target: path.to.clone(),
            target_spec_name,
            target_dir,
            source_coord,
            source_spec_name: source_cell.spec_name,
            boundary_dir,
            routed_dir,
            supported_boundary_values,
        });
    }

    let source_coords = entries
        .iter()
        .map(|entry| entry.source_coord)
        .collect::<BTreeSet<_>>();
    let variable_coords = entries
        .iter()
        .filter(|entry| entry.mode == InputEncodingMode::Variable)
        .map(|entry| entry.source_coord)
        .collect::<BTreeSet<_>>();
    let distinct_source_coords = source_coords.len() == entries.len();
    let variable_sources_independent = variable_coords.len()
        == entries
            .iter()
            .filter(|entry| entry.mode == InputEncodingMode::Variable)
            .count();
    let all_sources_binary_identity = entries.iter().all(|entry| {
        entry.supported_boundary_values == [0u8, 1u8].into_iter().collect::<BTreeSet<_>>()
    });

    let mut family_counts = BTreeMap::<String, usize>::new();
    for entry in &entries {
        *family_counts.entry(entry.family_label()).or_default() += 1;
    }
    let families = family_counts
        .into_iter()
        .map(|(label, count)| InputBoundaryFamily { label, count })
        .collect();

    Ok(InputBoundaryCertificate {
        entries,
        families,
        distinct_source_coords,
        all_sources_binary_identity,
        variable_sources_independent,
    })
}

fn input_mode_for_variable(variable: &str) -> Result<InputEncodingMode> {
    match variable {
        "const_0" => Ok(InputEncodingMode::ConstantZero),
        "const_1" => Ok(InputEncodingMode::ConstantOne),
        _ => Ok(InputEncodingMode::Variable),
    }
}

fn projected_relation(
    spec_name: &'static str,
    boundary_dir: RouteDir,
    routed_dir: RouteDir,
) -> Result<BTreeSet<(u8, u8)>> {
    let spec = published_spec_named(spec_name)
        .with_context(|| format!("Unknown published spec '{}'", spec_name))?;
    let boundary_port = route_dir_port_name(boundary_dir);
    let routed_port = route_dir_port_name(routed_dir);
    let wires = spec
        .relation_wires
        .chars()
        .map(|ch| ch.to_string())
        .collect::<Vec<_>>();
    let boundary_idx = wires
        .iter()
        .position(|wire| wire == boundary_port)
        .with_context(|| format!("Spec '{}' has no {} port", spec_name, boundary_port))?;
    let routed_idx = wires
        .iter()
        .position(|wire| wire == routed_port)
        .with_context(|| format!("Spec '{}' has no {} port", spec_name, routed_port))?;

    Ok(spec
        .relation_items
        .iter()
        .map(|row| (row[boundary_idx], row[routed_idx]))
        .collect())
}

fn route_dir_port_name(dir: RouteDir) -> &'static str {
    match dir {
        RouteDir::N => "N",
        RouteDir::E => "E",
        RouteDir::S => "S",
        RouteDir::W => "W",
    }
}

fn direction_between(from: (i32, i32), to: (i32, i32)) -> Result<RouteDir> {
    match (to.0 - from.0, to.1 - from.1) {
        (0, -1) => Ok(RouteDir::N),
        (1, 0) => Ok(RouteDir::E),
        (0, 1) => Ok(RouteDir::S),
        (-1, 0) => Ok(RouteDir::W),
        _ => anyhow::bail!("Cells {:?} and {:?} are not adjacent", from, to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;
    use crate::routing::construct_routing_witness;

    #[test]
    fn test_certify_input_boundary_encoding_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let witness = construct_routing_witness(&construction).unwrap();
        let certificate = certify_input_boundary_encoding(&construction, &witness).unwrap();

        assert!(certificate.is_complete());
        assert_eq!(certificate.entries.len(), 3);
        assert_eq!(certificate.families.len(), 1);
        assert!(certificate
            .entries
            .iter()
            .all(|entry| entry.source_spec_name == "horizontal wire tile"));
        assert!(certificate
            .entries
            .iter()
            .all(|entry| entry.mode == InputEncodingMode::Variable));
        assert!(certificate
            .render_summary()
            .contains("input_boundary_encoding_complete=true"));
    }
}
