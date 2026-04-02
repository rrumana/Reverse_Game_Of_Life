//! Constructive rectilinear routing witnesses for compiled macro constructions.
//!
//! This module extracts a deterministic polynomial-area routing witness directly from the
//! compiled macro netlist. It is intentionally separate from `board.rs`: the goal here is not
//! to solve published-pattern placement, but to replace the existential routing obligation in
//! the proof path with an explicit routed piece graph.

use crate::compiler::{
    CompiledConstruction, Endpoint, InstanceId, MacroInstance, MacroKind, PortRef,
};
use crate::published::{
    published_connector_specs, published_root, published_spec_named, PublishedPattern,
};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteDir {
    N,
    E,
    S,
    W,
}

impl RouteDir {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteDir::N => "N",
            RouteDir::E => "E",
            RouteDir::S => "S",
            RouteDir::W => "W",
        }
    }

    fn opposite(self) -> Self {
        match self {
            RouteDir::N => RouteDir::S,
            RouteDir::E => RouteDir::W,
            RouteDir::S => RouteDir::N,
            RouteDir::W => RouteDir::E,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl RouteBounds {
    pub fn width(self) -> usize {
        (self.max_x - self.min_x + 1).max(0) as usize
    }

    pub fn height(self) -> usize {
        (self.max_y - self.min_y + 1).max(0) as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedPiece {
    pub coord: (i32, i32),
    pub spec_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedCell {
    pub coord: (i32, i32),
    pub spec_name: &'static str,
    pub dirs: BTreeSet<RouteDir>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizontalInterface {
    pub left: (i32, i32),
    pub right: (i32, i32),
    pub left_spec_name: &'static str,
    pub connector_spec_name: &'static str,
    pub right_spec_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerticalInterface {
    pub top: (i32, i32),
    pub bottom: (i32, i32),
    pub top_spec_name: &'static str,
    pub bottom_spec_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizontalInterfaceFamily {
    pub left_spec_name: &'static str,
    pub connector_spec_name: &'static str,
    pub right_spec_name: &'static str,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerticalInterfaceFamily {
    pub top_spec_name: &'static str,
    pub bottom_spec_name: &'static str,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetPathWitness {
    pub net_index: usize,
    pub from: Endpoint,
    pub to: PortRef,
    pub cells: Vec<(i32, i32)>,
    pub turns: usize,
    pub crossings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingWitness {
    pub bounds: RouteBounds,
    pub pieces: Vec<RoutedPiece>,
    pub route_cells: Vec<RoutedCell>,
    pub horizontal_interfaces: Vec<HorizontalInterface>,
    pub vertical_interfaces: Vec<VerticalInterface>,
    pub horizontal_families: Vec<HorizontalInterfaceFamily>,
    pub vertical_families: Vec<VerticalInterfaceFamily>,
    pub net_paths: Vec<NetPathWitness>,
    pub primitive_usage: BTreeMap<&'static str, usize>,
}

impl RoutingWitness {
    pub fn route_cell_count(&self) -> usize {
        self.route_cells.len()
    }

    pub fn crossing_count(&self) -> usize {
        self.primitive_usage
            .get("crossing tile")
            .copied()
            .unwrap_or_default()
    }

    pub fn total_path_length(&self) -> usize {
        self.net_paths
            .iter()
            .map(|path| path.cells.len().saturating_sub(1))
            .sum()
    }

    pub fn render_summary(&self) -> String {
        let usage = self
            .primitive_usage
            .iter()
            .map(|(spec, count)| format!("{spec} x{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut lines = Vec::new();
        lines.push(format!(
            "routing_witness_valid=true route_paths={} routed_pieces={} route_cells={} crossings={} total_path_length={} bounds={}x{} horizontal_interface_families={} vertical_interface_families={}",
            self.net_paths.len(),
            self.pieces.len(),
            self.route_cells.len(),
            self.crossing_count(),
            self.total_path_length(),
            self.bounds.width(),
            self.bounds.height(),
            self.horizontal_families.len(),
            self.vertical_families.len(),
        ));
        lines.push(format!("routing_primitive_usage={usage}"));
        lines.join("\n")
    }

    pub fn render_interface_family_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "horizontal interface families: {}",
            self.horizontal_families.len()
        ));
        for family in &self.horizontal_families {
            lines.push(format!(
                "  {} --{}--> {} x{}",
                family.left_spec_name,
                family.connector_spec_name,
                family.right_spec_name,
                family.count
            ));
        }
        lines.push(format!(
            "vertical interface families: {}",
            self.vertical_families.len()
        ));
        for family in &self.vertical_families {
            lines.push(format!(
                "  {} / {} x{}",
                family.top_spec_name, family.bottom_spec_name, family.count
            ));
        }
        lines.join("\n")
    }
}

pub fn construct_routing_witness(construction: &CompiledConstruction) -> Result<RoutingWitness> {
    WitnessRouter::new(construction).build()
}

pub fn macro_port_dir(kind: &MacroKind, port: &str) -> Result<RouteDir> {
    match (kind, port) {
        (MacroKind::NotGate, "in") => Ok(RouteDir::W),
        (MacroKind::NotGate, "out") => Ok(RouteDir::E),
        (MacroKind::OrGate, "lhs") => Ok(RouteDir::N),
        (MacroKind::OrGate, "rhs") => Ok(RouteDir::S),
        (MacroKind::OrGate, "out") => Ok(RouteDir::E),
        (MacroKind::Splitter, "in") => Ok(RouteDir::S),
        (MacroKind::Splitter, "out0") => Ok(RouteDir::E),
        (MacroKind::Splitter, "out1") => Ok(RouteDir::N),
        (MacroKind::Enforcer, "in") => Ok(RouteDir::W),
        (MacroKind::InputPort { .. }, _) => {
            anyhow::bail!("Input ports do not have routed directions")
        }
        _ => anyhow::bail!("Unsupported routed port '{}' on {:?}", port, kind),
    }
}

struct WitnessRouter<'a> {
    construction: &'a CompiledConstruction,
    positions: HashMap<InstanceId, (i32, i32)>,
    external_rows: HashMap<String, i32>,
    channel_margin: i32,
    route_cells: HashMap<(i32, i32), BTreeSet<RouteDir>>,
    instance_links: Vec<((i32, i32), (i32, i32))>,
    net_paths: Vec<(Endpoint, PortRef, Vec<(i32, i32)>)>,
    connector_specs: BTreeMap<(i8, i8), &'static str>,
}

impl<'a> WitnessRouter<'a> {
    fn new(construction: &'a CompiledConstruction) -> Self {
        let net_count = construction.nets.len().max(1) as i32;
        let macro_pitch_x = 240 + net_count * 12;
        let macro_pitch_y = 180 + net_count * 12;
        let channel_margin = ninety_aligned_margin(net_count);
        let positions = construction
            .instances
            .iter()
            .map(|instance| {
                (
                    instance.id,
                    (
                        instance.column as i32 * macro_pitch_x + macro_pitch_x / 2,
                        instance.row as i32 * macro_pitch_y + macro_pitch_y / 2,
                    ),
                )
            })
            .collect();
        let external_rows = construction
            .variable_inputs
            .keys()
            .enumerate()
            .map(|(idx, variable)| {
                (
                    variable.clone(),
                    idx as i32 * macro_pitch_y + macro_pitch_y / 2,
                )
            })
            .collect();
        let connector_specs = published_connector_specs()
            .into_iter()
            .filter_map(|spec| {
                spec.align
                    .and_then(|(east, _, west, _)| Some(((west?, east?), spec.name)))
            })
            .collect();

        Self {
            construction,
            positions,
            external_rows,
            channel_margin,
            route_cells: HashMap::new(),
            instance_links: Vec::new(),
            net_paths: Vec::new(),
            connector_specs,
        }
    }

    fn build(mut self) -> Result<RoutingWitness> {
        for (net_index, net) in self.construction.nets.iter().enumerate() {
            self.route_net(net, net_index)?;
        }

        let mut pieces = BTreeMap::<(i32, i32), &'static str>::new();
        for instance in &self.construction.instances {
            let spec_name = instance
                .kind
                .published_spec_name()
                .with_context(|| format!("No published spec for {:?}", instance.kind))?;
            pieces.insert(self.positions[&instance.id], spec_name);
        }

        let mut route_cells = self
            .route_cells
            .iter()
            .map(|(&coord, dirs)| {
                Ok(RoutedCell {
                    coord,
                    spec_name: routing_spec_for_dirs(dirs)?,
                    dirs: dirs.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        route_cells.sort_by_key(|cell| cell.coord);

        for cell in &route_cells {
            pieces.entry(cell.coord).or_insert(cell.spec_name);
        }

        let (horizontal_interfaces, vertical_interfaces) = self.build_interfaces(&pieces)?;
        let horizontal_families = count_horizontal_families(&horizontal_interfaces);
        let vertical_families = count_vertical_families(&vertical_interfaces);
        let net_paths = self.finalize_net_paths()?;
        let primitive_usage = route_cells.iter().fold(
            BTreeMap::<&'static str, usize>::new(),
            |mut counts, cell| {
                *counts.entry(cell.spec_name).or_default() += 1;
                counts
            },
        );
        let bounds = compute_bounds(pieces.keys().copied().collect::<Vec<_>>().as_slice())?;

        Ok(RoutingWitness {
            bounds,
            pieces: pieces
                .into_iter()
                .map(|(coord, spec_name)| RoutedPiece { coord, spec_name })
                .collect(),
            route_cells,
            horizontal_interfaces,
            vertical_interfaces,
            horizontal_families,
            vertical_families,
            net_paths,
            primitive_usage,
        })
    }

    fn route_net(&mut self, net: &crate::compiler::Net, net_index: usize) -> Result<()> {
        let target = self.instance_endpoint_stub(&net.to)?;
        if let Endpoint::InstancePort(port) = &net.to {
            self.instance_links
                .push((target.coord, self.positions[&port.instance]));
        }
        let source = match &net.from {
            Endpoint::ExternalInput { variable } => Stub {
                coord: (-9 - net_index as i32 * 6, self.external_rows[variable]),
                back_dir: RouteDir::E,
            },
            Endpoint::InstancePort(port) => {
                let stub = self.instance_endpoint_stub(&net.from)?;
                self.instance_links
                    .push((stub.coord, self.positions[&port.instance]));
                stub
            }
        };
        let path = self.route_path(source, target, net_index)?;
        self.install_path(source, target, net_index, &path)?;
        let target_port = match &net.to {
            Endpoint::InstancePort(port) => port.clone(),
            Endpoint::ExternalInput { .. } => {
                anyhow::bail!("Compiled construction net unexpectedly targets an external input")
            }
        };
        self.net_paths.push((net.from.clone(), target_port, path));
        Ok(())
    }

    fn route_path(&self, source: Stub, target: Stub, net_index: usize) -> Result<Vec<(i32, i32)>> {
        let min_y = self.positions.values().map(|(_, y)| *y).min().unwrap_or(0);
        let min_x = self.positions.values().map(|(x, _)| *x).min().unwrap_or(0);
        let max_x = self.positions.values().map(|(x, _)| *x).max().unwrap_or(0);
        let slot = net_index as i32 * 6;
        let trunk_y = min_y - self.channel_margin - slot;
        let source_out = source.back_dir.opposite();
        let target_out = target.back_dir.opposite();
        let source_escape = self.fabric_escape_point(source);
        let target_escape = self.fabric_escape_point(target);
        let source_local_y =
            self.fabric_local_row(source_escape, source_out, net_index, EndpointRole::Source);
        let target_local_y =
            self.fabric_local_row(target_escape, target_out, net_index, EndpointRole::Target);
        let source_bus_x = self.fabric_bus_x(min_x, max_x, source_out, slot, EndpointRole::Source);
        let target_bus_x = self.fabric_bus_x(min_x, max_x, target_out, slot, EndpointRole::Target);

        let mut path = vec![source.coord];
        let mut waypoints = vec![source_escape];
        waypoints.push((source_escape.0, source_local_y));
        waypoints.push((source_bus_x, source_local_y));
        waypoints.push((source_bus_x, trunk_y));
        waypoints.push((target_bus_x, trunk_y));
        waypoints.push((target_bus_x, target_local_y));
        waypoints.push((target_escape.0, target_local_y));
        waypoints.push(target_escape);
        waypoints.push(target.coord);
        waypoints.dedup();
        for waypoint in waypoints {
            extend_manhattan_segment(&mut path, waypoint)?;
        }
        Ok(path)
    }

    fn install_path(
        &mut self,
        source: Stub,
        target: Stub,
        net_index: usize,
        path: &[(i32, i32)],
    ) -> Result<()> {
        for (idx, &cell) in path.iter().enumerate() {
            let dirs = if idx == 0 {
                let next = *path.get(1).context("Route path too short")?;
                let out = direction_between(cell, next)?;
                btreeset2(source.back_dir, out)
            } else if idx + 1 == path.len() {
                let prev = path[idx - 1];
                let incoming = direction_between(prev, cell)?.opposite();
                btreeset2(incoming, target.back_dir)
            } else {
                let prev = path[idx - 1];
                let next = path[idx + 1];
                let incoming = direction_between(prev, cell)?.opposite();
                let outgoing = direction_between(cell, next)?;
                btreeset2(incoming, outgoing)
            };
            self.place_route_cell(cell, dirs, net_index)?;
        }
        Ok(())
    }

    fn place_route_cell(
        &mut self,
        cell: (i32, i32),
        dirs: BTreeSet<RouteDir>,
        net_index: usize,
    ) -> Result<()> {
        match self.route_cells.get(&cell).cloned() {
            None => {
                self.route_cells.insert(cell, dirs);
                Ok(())
            }
            Some(existing) => {
                let horizontal = btreeset2(RouteDir::E, RouteDir::W);
                let vertical = btreeset2(RouteDir::N, RouteDir::S);
                if existing == horizontal && dirs == vertical
                    || existing == vertical && dirs == horizontal
                {
                    self.route_cells.insert(
                        cell,
                        btreeset4(RouteDir::N, RouteDir::E, RouteDir::S, RouteDir::W),
                    );
                    Ok(())
                } else if existing == dirs {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Routing witness conflict at {:?} for net {}: existing {:?}, new {:?}",
                        cell,
                        net_index,
                        existing,
                        dirs
                    )
                }
            }
        }
    }

    fn instance_endpoint_stub(&self, endpoint: &Endpoint) -> Result<Stub> {
        match endpoint {
            Endpoint::ExternalInput { .. } => {
                anyhow::bail!("External inputs do not have fixed instance stubs")
            }
            Endpoint::InstancePort(port) => {
                let instance = self.instance(port.instance)?;
                let (x, y) = self.positions[&port.instance];
                let dir = macro_port_dir(&instance.kind, port.port)?;
                let coord = step((x, y), dir);
                Ok(Stub {
                    coord,
                    back_dir: dir.opposite(),
                })
            }
        }
    }

    fn instance(&self, id: InstanceId) -> Result<&MacroInstance> {
        self.construction
            .instances
            .iter()
            .find(|instance| instance.id == id)
            .with_context(|| format!("Unknown macro instance {}", id.0))
    }

    fn fabric_escape_point(&self, stub: Stub) -> (i32, i32) {
        let center = step(stub.coord, stub.back_dir);
        match stub.back_dir.opposite() {
            RouteDir::E => (center.0 + 3, center.1),
            RouteDir::W => (center.0 - 3, center.1),
            RouteDir::N => (center.0, center.1 - 3),
            RouteDir::S => (center.0, center.1 + 3),
        }
    }

    fn fabric_bus_x(
        &self,
        min_x: i32,
        max_x: i32,
        outward: RouteDir,
        slot: i32,
        role: EndpointRole,
    ) -> i32 {
        let role_offset = match role {
            EndpointRole::Source => 0,
            EndpointRole::Target => 3,
        };
        match outward {
            RouteDir::W => min_x - self.channel_margin - slot - role_offset,
            RouteDir::E | RouteDir::N | RouteDir::S => {
                max_x + self.channel_margin + slot + role_offset
            }
        }
    }

    fn fabric_local_row(
        &self,
        escape: (i32, i32),
        outward: RouteDir,
        net_index: usize,
        role: EndpointRole,
    ) -> i32 {
        let role_offset = match role {
            EndpointRole::Source => 3,
            EndpointRole::Target => 6,
        };
        let delta = role_offset + net_index as i32 * 6;
        match outward {
            RouteDir::S => escape.1 + delta,
            RouteDir::N | RouteDir::E | RouteDir::W => escape.1 - delta,
        }
    }

    fn build_interfaces(
        &self,
        pieces: &BTreeMap<(i32, i32), &'static str>,
    ) -> Result<(Vec<HorizontalInterface>, Vec<VerticalInterface>)> {
        let mut horizontal_relations = Vec::new();
        let mut vertical_relations = Vec::new();

        for (&cell, dirs) in &self.route_cells {
            for &dir in dirs {
                let neighbor = step(cell, dir);
                if !self
                    .route_cells
                    .get(&neighbor)
                    .is_some_and(|neighbor_dirs| neighbor_dirs.contains(&dir.opposite()))
                {
                    continue;
                }
                match dir {
                    RouteDir::N | RouteDir::S => {
                        if dir == RouteDir::S {
                            vertical_relations.push((cell, neighbor));
                        }
                    }
                    RouteDir::E => {
                        horizontal_relations.push((cell, neighbor));
                    }
                    RouteDir::W => {}
                }
            }
        }

        for &(stub, instance) in &self.instance_links {
            if stub.0 == instance.0 {
                if stub.1 < instance.1 {
                    vertical_relations.push((stub, instance));
                } else {
                    vertical_relations.push((instance, stub));
                }
            } else if stub.0 < instance.0 {
                horizontal_relations.push((stub, instance));
            } else {
                horizontal_relations.push((instance, stub));
            }
        }

        horizontal_relations.sort_unstable();
        horizontal_relations.dedup();
        vertical_relations.sort_unstable();
        vertical_relations.dedup();

        let horizontal_interfaces = horizontal_relations
            .into_iter()
            .map(|(left, right)| {
                let left_spec_name = *pieces
                    .get(&left)
                    .with_context(|| format!("Missing left routed piece at {:?}", left))?;
                let right_spec_name = *pieces
                    .get(&right)
                    .with_context(|| format!("Missing right routed piece at {:?}", right))?;
                let connector_spec_name = self.connector_name(left_spec_name, right_spec_name)?;
                Ok(HorizontalInterface {
                    left,
                    right,
                    left_spec_name,
                    connector_spec_name,
                    right_spec_name,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let vertical_interfaces = vertical_relations
            .into_iter()
            .map(|(top, bottom)| {
                let top_spec_name = *pieces
                    .get(&top)
                    .with_context(|| format!("Missing top routed piece at {:?}", top))?;
                let bottom_spec_name = *pieces
                    .get(&bottom)
                    .with_context(|| format!("Missing bottom routed piece at {:?}", bottom))?;
                Ok(VerticalInterface {
                    top,
                    bottom,
                    top_spec_name,
                    bottom_spec_name,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok((horizontal_interfaces, vertical_interfaces))
    }

    fn connector_name(
        &self,
        left_spec_name: &'static str,
        right_spec_name: &'static str,
    ) -> Result<&'static str> {
        let west_phase = phase_for_spec_wire(left_spec_name, RouteDir::E)?;
        let east_phase = phase_for_spec_wire(right_spec_name, RouteDir::W)?;
        self.connector_specs
            .get(&(west_phase, east_phase))
            .copied()
            .with_context(|| {
                format!(
                    "Missing connector for {} east phase {} to {} west phase {}",
                    left_spec_name, west_phase, right_spec_name, east_phase
                )
            })
    }

    fn finalize_net_paths(&self) -> Result<Vec<NetPathWitness>> {
        self.net_paths
            .iter()
            .enumerate()
            .map(|(net_index, (from, to, cells))| {
                let turns = count_turns(cells)?;
                let crossings = cells
                    .iter()
                    .filter(|coord| {
                        self.route_cells
                            .get(coord)
                            .is_some_and(|dirs| dirs.len() == 4)
                    })
                    .count();
                Ok(NetPathWitness {
                    net_index,
                    from: from.clone(),
                    to: to.clone(),
                    cells: cells.clone(),
                    turns,
                    crossings,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum EndpointRole {
    Source,
    Target,
}

#[derive(Debug, Clone, Copy)]
struct Stub {
    coord: (i32, i32),
    back_dir: RouteDir,
}

fn compute_bounds(coords: &[(i32, i32)]) -> Result<RouteBounds> {
    Ok(RouteBounds {
        min_x: coords
            .iter()
            .map(|(x, _)| *x)
            .min()
            .context("Missing minimum x for routing witness")?,
        min_y: coords
            .iter()
            .map(|(_, y)| *y)
            .min()
            .context("Missing minimum y for routing witness")?,
        max_x: coords
            .iter()
            .map(|(x, _)| *x)
            .max()
            .context("Missing maximum x for routing witness")?,
        max_y: coords
            .iter()
            .map(|(_, y)| *y)
            .max()
            .context("Missing maximum y for routing witness")?,
    })
}

fn count_turns(path: &[(i32, i32)]) -> Result<usize> {
    let mut turns = 0usize;
    for triple in path.windows(3) {
        let incoming = direction_between(triple[0], triple[1])?;
        let outgoing = direction_between(triple[1], triple[2])?;
        if incoming != outgoing {
            turns += 1;
        }
    }
    Ok(turns)
}

fn count_horizontal_families(interfaces: &[HorizontalInterface]) -> Vec<HorizontalInterfaceFamily> {
    let mut counts = BTreeMap::<(&'static str, &'static str, &'static str), usize>::new();
    for item in interfaces {
        *counts
            .entry((
                item.left_spec_name,
                item.connector_spec_name,
                item.right_spec_name,
            ))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(
            |((left_spec_name, connector_spec_name, right_spec_name), count)| {
                HorizontalInterfaceFamily {
                    left_spec_name,
                    connector_spec_name,
                    right_spec_name,
                    count,
                }
            },
        )
        .collect()
}

fn count_vertical_families(interfaces: &[VerticalInterface]) -> Vec<VerticalInterfaceFamily> {
    let mut counts = BTreeMap::<(&'static str, &'static str), usize>::new();
    for item in interfaces {
        *counts
            .entry((item.top_spec_name, item.bottom_spec_name))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(
            |((top_spec_name, bottom_spec_name), count)| VerticalInterfaceFamily {
                top_spec_name,
                bottom_spec_name,
                count,
            },
        )
        .collect()
}

fn phase_for_spec_wire(spec_name: &'static str, dir: RouteDir) -> Result<i8> {
    let spec = published_spec_named(spec_name)
        .with_context(|| format!("Unknown published spec '{spec_name}'"))?;
    let (east, north, west, south) = match spec.align {
        Some(align) => align,
        None => {
            let pattern_path = published_root().join(spec.path);
            PublishedPattern::from_csv_file(&pattern_path)
                .with_context(|| {
                    format!(
                        "Failed to load published pattern '{}' for phase alignment fallback",
                        spec_name
                    )
                })?
                .phase_alignment()
        }
    };
    match dir {
        RouteDir::E => east,
        RouteDir::N => north,
        RouteDir::W => west,
        RouteDir::S => south,
    }
    .with_context(|| {
        format!(
            "Published spec '{spec_name}' is missing {} phase",
            dir.as_str()
        )
    })
}

fn extend_manhattan_segment(path: &mut Vec<(i32, i32)>, target: (i32, i32)) -> Result<()> {
    let mut current = *path.last().context("Path is unexpectedly empty")?;
    if current.0 != target.0 && current.1 != target.1 {
        anyhow::bail!(
            "Routing witness produced non-Manhattan waypoints {:?} -> {:?}",
            current,
            target
        );
    }
    let dx = (target.0 - current.0).signum();
    let dy = (target.1 - current.1).signum();
    while current != target {
        current = (current.0 + dx, current.1 + dy);
        path.push(current);
    }
    Ok(())
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

fn step((x, y): (i32, i32), dir: RouteDir) -> (i32, i32) {
    match dir {
        RouteDir::N => (x, y - 1),
        RouteDir::E => (x + 1, y),
        RouteDir::S => (x, y + 1),
        RouteDir::W => (x - 1, y),
    }
}

fn btreeset2(a: RouteDir, b: RouteDir) -> BTreeSet<RouteDir> {
    let mut set = BTreeSet::new();
    set.insert(a);
    set.insert(b);
    set
}

fn btreeset4(a: RouteDir, b: RouteDir, c: RouteDir, d: RouteDir) -> BTreeSet<RouteDir> {
    let mut set = BTreeSet::new();
    set.insert(a);
    set.insert(b);
    set.insert(c);
    set.insert(d);
    set
}

fn ninety_aligned_margin(net_count: i32) -> i32 {
    90 + net_count * 12
}

fn routing_spec_for_dirs(dirs: &BTreeSet<RouteDir>) -> Result<&'static str> {
    match dirs.iter().copied().collect::<Vec<_>>().as_slice() {
        [RouteDir::N, RouteDir::S] => Ok("vertical wire tile"),
        [RouteDir::E, RouteDir::W] => Ok("horizontal wire tile"),
        [RouteDir::N, RouteDir::E] => Ok("NE turn tile"),
        [RouteDir::N, RouteDir::W] => Ok("NW turn tile"),
        [RouteDir::S, RouteDir::W] => Ok("SW turn tile"),
        [RouteDir::E, RouteDir::S] => Ok("SE turn tile"),
        [RouteDir::N, RouteDir::E, RouteDir::S, RouteDir::W] => Ok("crossing tile"),
        _ => anyhow::bail!("No routing tile for direction set {:?}", dirs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;

    #[test]
    fn test_construct_routing_witness_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let witness = construct_routing_witness(&construction).unwrap();

        assert_eq!(witness.net_paths.len(), construction.nets.len());
        assert!(witness.route_cell_count() > 0);
        assert!(witness.bounds.width() > 0);
        assert!(witness.bounds.height() > 0);
        assert!(!witness.horizontal_families.is_empty());
        assert!(!witness.vertical_families.is_empty());
        assert!(witness.primitive_usage.contains_key("horizontal wire tile"));
        assert!(witness
            .render_summary()
            .contains("routing_witness_valid=true"));
    }
}
