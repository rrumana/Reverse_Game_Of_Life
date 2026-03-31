//! Routed board emission for macro-level `SAT -> Rev_GOL` constructions.
//!
//! The current implementation can turn a macro construction into a routed
//! published-pattern board candidate, but the placement solver is still
//! incomplete for some mixed turn/connector layouts.

use crate::compiler::{CompiledConstruction, Endpoint, InstanceId, MacroKind, MacroInstance};
use crate::published::{
    published_part1_specs, published_root, PublishedPattern, WireAnchors,
};
use crate::verifier::CellCoord;
use anyhow::{Context, Result};
use rev_gol::config::BoundaryCondition;
use rev_gol::game_of_life::{
    io::{grid_to_string, save_grid_to_file},
    Grid,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    fn opposite(self) -> Self {
        match self {
            Dir::N => Dir::S,
            Dir::E => Dir::W,
            Dir::S => Dir::N,
            Dir::W => Dir::E,
        }
    }
}

#[derive(Debug, Clone)]
struct PatternInfo {
    pattern: PublishedPattern,
    anchors: WireAnchors,
}

#[derive(Debug, Clone)]
pub struct BoardPiece {
    pub spec_name: String,
    pub origin_x: usize,
    pub origin_y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone)]
pub struct PublishedBoard {
    pub pieces: Vec<BoardPiece>,
    pub target: Grid,
}

impl PublishedBoard {
    pub fn to_grid_string(&self) -> String {
        grid_to_string(&self.target)
    }

    pub fn save_target_grid<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        save_grid_to_file(&self.target, path)
    }
}

pub fn build_published_board(construction: &CompiledConstruction) -> Result<PublishedBoard> {
    let root = published_root();
    let library = PublishedLibrary::load(&root)?;
    FabricAssembler::new(construction, &library).build()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HorizontalMotifFamily {
    pub left_spec: String,
    pub connector_name: String,
    pub right_spec: String,
    pub count: usize,
    pub calibrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VerticalMotifFamily {
    pub top_spec: String,
    pub bottom_spec: String,
    pub count: usize,
    pub calibrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardMotifAuditReport {
    pub horizontal: Vec<HorizontalMotifFamily>,
    pub vertical: Vec<VerticalMotifFamily>,
}

impl BoardMotifAuditReport {
    pub fn unresolved_horizontal(&self) -> Vec<&HorizontalMotifFamily> {
        self.horizontal.iter().filter(|item| !item.calibrated).collect()
    }

    pub fn unresolved_vertical(&self) -> Vec<&VerticalMotifFamily> {
        self.vertical.iter().filter(|item| !item.calibrated).collect()
    }

    pub fn render_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "horizontal motif families: {} ({} unresolved)",
            self.horizontal.len(),
            self.unresolved_horizontal().len()
        ));
        for family in &self.horizontal {
            lines.push(format!(
                "  [{}] {} --{}--> {} x{}",
                if family.calibrated { "ok" } else { "todo" },
                family.left_spec,
                family.connector_name,
                family.right_spec,
                family.count
            ));
        }
        lines.push(format!(
            "vertical motif families: {} ({} unresolved)",
            self.vertical.len(),
            self.unresolved_vertical().len()
        ));
        for family in &self.vertical {
            lines.push(format!(
                "  [{}] {} / {} x{}",
                if family.calibrated { "ok" } else { "todo" },
                family.top_spec,
                family.bottom_spec,
                family.count
            ));
        }
        lines.join("\n")
    }
}

pub fn audit_published_board_motifs(
    construction: &CompiledConstruction,
) -> Result<BoardMotifAuditReport> {
    let root = published_root();
    let library = PublishedLibrary::load(&root)?;
    BoardRouter::new(construction, &library).audit_motifs()
}

struct PublishedLibrary {
    specs: HashMap<String, PatternInfo>,
    connectors: HashMap<(i8, i8), String>,
}

struct FabricAssembler<'a> {
    construction: &'a CompiledConstruction,
    library: &'a PublishedLibrary,
    positions: HashMap<InstanceId, (i32, i32)>,
    external_rows: HashMap<String, i32>,
    channel_margin: i32,
    route_cells: HashMap<(i32, i32), BTreeSet<Dir>>,
    instance_links: Vec<((i32, i32), (i32, i32))>,
}

impl<'a> FabricAssembler<'a> {
    fn new(construction: &'a CompiledConstruction, library: &'a PublishedLibrary) -> Self {
        let net_count = construction.nets.len().max(1) as i32;
        let macro_pitch_x = 240 + net_count * 12;
        let macro_pitch_y = 180 + net_count * 12;
        let channel_margin =  ninety_aligned_margin(net_count);
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

        Self {
            construction,
            library,
            positions,
            external_rows,
            channel_margin,
            route_cells: HashMap::new(),
            instance_links: Vec::new(),
        }
    }

    fn build(mut self) -> Result<PublishedBoard> {
        let layout = self.prepare_layout()?;
        let mut solved = PlacementSolver::new(self.library);
        for (&cell, spec_name) in &layout.pieces {
            solved.add_piece(cell, spec_name)?;
        }
        for &(left, right) in &layout.horizontal_relations {
            solved.add_horizontal_adjacency(left, right)?;
        }
        for &(top, bottom) in &layout.vertical_relations {
            solved.add_vertical_adjacency(top, bottom)?;
        }
        solved.materialize()
    }

    fn prepare_layout(&mut self) -> Result<AbstractBoardLayout> {
        let mut pieces = HashMap::new();
        for instance in &self.construction.instances {
            let spec = instance
                .kind
                .published_spec_name()
                .with_context(|| format!("No published spec for {:?}", instance.kind))?;
            pieces.insert(self.positions[&instance.id], spec.to_string());
        }

        for (net_index, net) in self.construction.nets.iter().enumerate() {
            self.route_net(net, net_index)?;
        }

        for (&cell, dirs) in &self.route_cells {
            pieces.insert(cell, routing_spec_for_dirs(dirs)?.to_string());
        }

        let mut horizontal_relations = Vec::new();
        let mut vertical_relations = Vec::new();
        for (&cell, dirs) in &self.route_cells {
            for &dir in dirs {
                let neighbor = step(cell, dir);
                let connected = self
                    .route_cells
                    .get(&neighbor)
                    .is_some_and(|other| other.contains(&dir.opposite()));
                if !connected {
                    continue;
                }
                match dir {
                    Dir::E => horizontal_relations.push((cell, neighbor)),
                    Dir::S => vertical_relations.push((cell, neighbor)),
                    Dir::N | Dir::W => {}
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

        Ok(AbstractBoardLayout {
            pieces,
            horizontal_relations,
            vertical_relations,
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
                back_dir: Dir::E,
            },
            Endpoint::InstancePort(port) => {
                let stub = self.instance_endpoint_stub(&net.from)?;
                self.instance_links.push((stub.coord, self.positions[&port.instance]));
                stub
            }
        };
        let path = self.route_path(source, target, net_index)?;
        self.install_path(source, target, net_index, &path)
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
        let source_local_y = self.fabric_local_row(source_escape, source_out, net_index, EndpointRole::Source);
        let target_local_y = self.fabric_local_row(target_escape, target_out, net_index, EndpointRole::Target);
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
        dirs: BTreeSet<Dir>,
        net_index: usize,
    ) -> Result<()> {
        match self.route_cells.get(&cell).cloned() {
            None => {
                self.route_cells.insert(cell, dirs);
                Ok(())
            }
            Some(existing) => {
                let horizontal = btreeset2(Dir::E, Dir::W);
                let vertical = btreeset2(Dir::N, Dir::S);
                if existing == horizontal && dirs == vertical
                    || existing == vertical && dirs == horizontal
                {
                    self.route_cells
                        .insert(cell, btreeset4(Dir::N, Dir::E, Dir::S, Dir::W));
                    Ok(())
                } else if existing == dirs {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Fabric router conflict at {:?} for net {}: existing {:?}, new {:?}",
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
                let dir = instance_port_dir(instance, port.port)?;
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
            Dir::E => (center.0 + 3, center.1),
            Dir::W => (center.0 - 3, center.1),
            Dir::N => (center.0, center.1 - 3),
            Dir::S => (center.0, center.1 + 3),
        }
    }

    fn fabric_bus_x(
        &self,
        min_x: i32,
        max_x: i32,
        outward: Dir,
        slot: i32,
        role: EndpointRole,
    ) -> i32 {
        let role_offset = match role {
            EndpointRole::Source => 0,
            EndpointRole::Target => 3,
        };
        match outward {
            Dir::W => min_x - self.channel_margin - slot - role_offset,
            Dir::E | Dir::N | Dir::S => max_x + self.channel_margin + slot + role_offset,
        }
    }

    fn fabric_local_row(
        &self,
        escape: (i32, i32),
        outward: Dir,
        net_index: usize,
        role: EndpointRole,
    ) -> i32 {
        let role_offset = match role {
            EndpointRole::Source => 3,
            EndpointRole::Target => 6,
        };
        let delta = role_offset + net_index as i32 * 6;
        match outward {
            Dir::S => escape.1 + delta,
            Dir::N | Dir::E | Dir::W => escape.1 - delta,
        }
    }
}

fn extend_manhattan_segment(path: &mut Vec<(i32, i32)>, target: (i32, i32)) -> Result<()> {
    let mut current = *path.last().context("Path is unexpectedly empty")?;
    if current.0 != target.0 && current.1 != target.1 {
        anyhow::bail!(
            "Fabric router produced non-Manhattan waypoints {:?} -> {:?}",
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

fn btreeset2(a: Dir, b: Dir) -> BTreeSet<Dir> {
    let mut set = BTreeSet::new();
    set.insert(a);
    set.insert(b);
    set
}

fn btreeset4(a: Dir, b: Dir, c: Dir, d: Dir) -> BTreeSet<Dir> {
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

impl PublishedLibrary {
    fn load(root: &Path) -> Result<Self> {
        let mut specs = HashMap::new();
        let mut connectors = HashMap::new();

        for spec in published_part1_specs() {
            let path = root.join(spec.path);
            let pattern = PublishedPattern::from_csv_file(&path)?;
            let anchors = pattern.find_wires();
            if spec.path.starts_with("connectors/") {
                let align = spec.align.unwrap_or_else(|| pattern.phase_alignment());
                let west = align.2.context("Connector missing west alignment")?;
                let east = align.0.context("Connector missing east alignment")?;
                connectors.insert((west, east), spec.name.to_string());
            }
            specs.insert(
                spec.name.to_string(),
                PatternInfo {
                    pattern,
                    anchors,
                },
            );
        }

        Ok(Self { specs, connectors })
    }

    fn pattern(&self, spec_name: &str) -> Result<&PatternInfo> {
        self.specs
            .get(spec_name)
            .with_context(|| format!("Unknown published spec '{spec_name}'"))
    }

    fn connector_name(&self, west_align: i8, east_align: i8) -> Result<&str> {
        self.connectors
            .get(&(west_align, east_align))
            .map(String::as_str)
            .with_context(|| format!("Missing connector for alignment {west_align} -> {east_align}"))
    }
}

struct BoardRouter<'a> {
    construction: &'a CompiledConstruction,
    library: &'a PublishedLibrary,
    positions: HashMap<InstanceId, (i32, i32)>,
    external_rows: HashMap<String, i32>,
    channel_margin: i32,
    cells: HashMap<(i32, i32), String>,
    dirs: HashMap<(i32, i32), BTreeSet<Dir>>,
    route_owner: HashMap<(i32, i32), RouteOwnership>,
    instance_links: Vec<((i32, i32), (i32, i32))>,
}

#[derive(Debug, Clone, Copy, Default)]
struct RouteOwnership {
    horizontal: Option<usize>,
    vertical: Option<usize>,
}

#[derive(Debug, Clone)]
struct AbstractBoardLayout {
    pieces: HashMap<(i32, i32), String>,
    horizontal_relations: Vec<((i32, i32), (i32, i32))>,
    vertical_relations: Vec<((i32, i32), (i32, i32))>,
}

impl<'a> BoardRouter<'a> {
    fn new(construction: &'a CompiledConstruction, library: &'a PublishedLibrary) -> Self {
        let net_count = construction.nets.len().max(1) as i32;
        let side_span = 4 + net_count * 2;
        let macro_pitch_x = side_span * 2 + 10;
        let macro_pitch_y = side_span * 2 + 10;
        let channel_margin = side_span + 4;
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
            .map(|(idx, variable)| (variable.clone(), idx as i32 * macro_pitch_y + macro_pitch_y / 2))
            .collect();

        Self {
            construction,
            library,
            positions,
            external_rows,
            channel_margin,
            cells: HashMap::new(),
            dirs: HashMap::new(),
            route_owner: HashMap::new(),
            instance_links: Vec::new(),
        }
    }

    fn build(mut self) -> Result<PublishedBoard> {
        let layout = self.prepare_layout()?;

        let mut solved = PlacementSolver::new(self.library);
        for (&cell, spec_name) in &layout.pieces {
            solved.add_piece(cell, spec_name)?;
        }
        for &(left, right) in &layout.horizontal_relations {
            solved.add_horizontal_adjacency(left, right)?;
        }
        for &(top, bottom) in &layout.vertical_relations {
            solved.add_vertical_adjacency(top, bottom)?;
        }

        solved.materialize()
    }

    fn audit_motifs(mut self) -> Result<BoardMotifAuditReport> {
        let layout = self.prepare_layout()?;
        let mut horizontal = BTreeMap::<(String, String, String), usize>::new();
        let mut vertical = BTreeMap::<(String, String), usize>::new();

        for &(left, right) in &layout.horizontal_relations {
            let left_spec = layout
                .pieces
                .get(&left)
                .with_context(|| format!("Missing left piece at {:?}", left))?;
            let right_spec = layout
                .pieces
                .get(&right)
                .with_context(|| format!("Missing right piece at {:?}", right))?;
            let left_info = self.library.pattern(left_spec)?;
            let right_info = self.library.pattern(right_spec)?;
            let left_align = left_info.pattern.phase_alignment().0.context("Missing east phase")?;
            let right_align = right_info.pattern.phase_alignment().2.context("Missing west phase")?;
            let connector_name = self.library.connector_name(left_align, right_align)?.to_string();
            *horizontal
                .entry((left_spec.clone(), connector_name, right_spec.clone()))
                .or_default() += 1;
        }

        for &(top, bottom) in &layout.vertical_relations {
            let top_spec = layout
                .pieces
                .get(&top)
                .with_context(|| format!("Missing top piece at {:?}", top))?;
            let bottom_spec = layout
                .pieces
                .get(&bottom)
                .with_context(|| format!("Missing bottom piece at {:?}", bottom))?;
            *vertical
                .entry((top_spec.clone(), bottom_spec.clone()))
                .or_default() += 1;
        }

        Ok(BoardMotifAuditReport {
            horizontal: horizontal
                .into_iter()
                .map(|((left_spec, connector_name, right_spec), count)| HorizontalMotifFamily {
                    calibrated: !known_horizontal_placement_candidates(
                        &left_spec,
                        &connector_name,
                        &right_spec,
                    )
                    .is_empty(),
                    left_spec,
                    connector_name,
                    right_spec,
                    count,
                })
                .collect(),
            vertical: vertical
                .into_iter()
                .map(|((top_spec, bottom_spec), count)| VerticalMotifFamily {
                    calibrated: !known_vertical_placement_candidates(&top_spec, &bottom_spec)
                        .is_empty(),
                    top_spec,
                    bottom_spec,
                    count,
                })
                .collect(),
        })
    }

    fn prepare_layout(&mut self) -> Result<AbstractBoardLayout> {
        for instance in &self.construction.instances {
            let spec = instance
                .kind
                .published_spec_name()
                .with_context(|| format!("No published spec for {:?}", instance.kind))?;
            let pos = self.positions[&instance.id];
            self.cells.insert(pos, spec.to_string());
        }

        for (net_index, net) in self.construction.nets.iter().enumerate() {
            self.route_net(net, net_index)?;
        }

        let mut pieces = self.cells.clone();
        for (&cell, dirs) in &self.dirs {
            if pieces.contains_key(&cell) {
                continue;
            }
            let spec_name = routing_spec_for_dirs(dirs)?;
            pieces.insert(cell, spec_name.to_string());
        }

        let mut horizontal_relations = Vec::new();
        let mut vertical_relations = Vec::new();
        for (&cell, dirs) in &self.dirs {
            for &dir in dirs {
                let neighbor = step(cell, dir);
                if !self.dirs.get(&neighbor).is_some_and(|d| d.contains(&dir.opposite())) {
                    continue;
                }
                match dir {
                    Dir::N | Dir::S => {
                        if dir == Dir::S {
                            vertical_relations.push((cell, neighbor));
                        }
                    }
                    Dir::E => {
                        horizontal_relations.push((cell, neighbor));
                    }
                    Dir::W => {}
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

        Ok(AbstractBoardLayout {
            pieces,
            horizontal_relations,
            vertical_relations,
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
                coord: (0, self.external_rows[variable]),
                back_dir: Dir::E,
            },
            Endpoint::InstancePort(port) => {
                let stub = self.instance_endpoint_stub(&net.from)?;
                self.instance_links.push((stub.coord, self.positions[&port.instance]));
                stub
            }
        };
        let path = self.find_path(source, target, net_index)?;

        self.dirs.entry(source.coord).or_default().insert(source.back_dir);
        self.dirs.entry(target.coord).or_default().insert(target.back_dir);
        for pair in path.windows(2) {
            self.draw_segment(pair[0], pair[1], net_index)?;
        }

        Ok(())
    }

    fn instance_endpoint_stub(&self, endpoint: &Endpoint) -> Result<Stub> {
        match endpoint {
            Endpoint::ExternalInput { .. } => {
                anyhow::bail!("External inputs do not have fixed instance stubs")
            }
            Endpoint::InstancePort(port) => {
                let instance = self.instance(port.instance)?;
                let (x, y) = self.positions[&port.instance];
                let dir = instance_port_dir(instance, port.port)?;
                let coord = step((x, y), dir);
                Ok(Stub {
                    coord,
                    back_dir: dir.opposite(),
                })
            }
        }
    }

    fn find_path(&self, source: Stub, target: Stub, net_index: usize) -> Result<Vec<(i32, i32)>> {
        let min_y = self.positions.values().map(|(_, y)| *y).min().unwrap_or(0);
        let min_x = self.positions.values().map(|(x, _)| *x).min().unwrap_or(0);
        let max_x = self.positions.values().map(|(x, _)| *x).max().unwrap_or(0);
        let slot = net_index as i32 * 6;
        let top_lane_y = min_y - self.channel_margin - slot;
        let source_out = source.back_dir.opposite();
        let target_out = target.back_dir.opposite();
        let source_escape = self.escape_point(source);
        let target_escape = self.escape_point(target);
        let source_bus_x =
            self.endpoint_bus_x(min_x, max_x, source_out, slot, EndpointRole::Source);
        let target_bus_x =
            self.endpoint_bus_x(min_x, max_x, target_out, slot, EndpointRole::Target);
        let lane_y = top_lane_y;
        let source_row_y = lane_y + 1;
        let target_row_y = lane_y + 2;

        let mut path = vec![source.coord];
        let mut waypoints = Vec::new();
        waypoints.push(source_escape);
        match source_out {
            Dir::E | Dir::W => {
                waypoints.push((source_escape.0, source_row_y));
                waypoints.push((source_bus_x, source_row_y));
                waypoints.push((source_bus_x, lane_y));
            }
            Dir::N | Dir::S => {
                waypoints.push((source_bus_x, source_escape.1));
                waypoints.push((source_bus_x, source_row_y));
                waypoints.push((source_bus_x, lane_y));
            }
        }
        match target_out {
            Dir::E | Dir::W => {
                waypoints.push((target_bus_x, lane_y));
                waypoints.push((target_bus_x, target_row_y));
                waypoints.push((target_escape.0, target_row_y));
            }
            Dir::N | Dir::S => {
                waypoints.push((target_bus_x, lane_y));
                waypoints.push((target_bus_x, target_row_y));
                waypoints.push((target_bus_x, target_escape.1));
            }
        }
        waypoints.push(target_escape);
        waypoints.push(target.coord);
        waypoints.dedup();
        for waypoint in waypoints {
            self.extend_manhattan_segment(&mut path, waypoint)?;
        }

        if path.len() >= 2 {
            let source_next = path[1];
            let source_dir = direction_between(source.coord, source_next)?;
            if source_dir == source.back_dir {
                anyhow::bail!("Source path leaves through the gadget interior");
            }

            let target_prev = path[path.len() - 2];
            let target_incoming = direction_between(target_prev, target.coord)?.opposite();
            if target_incoming == target.back_dir {
                anyhow::bail!("Target path approaches through the gadget interior");
            }
        }

        Ok(path)
    }

    fn extend_manhattan_segment(
        &self,
        path: &mut Vec<(i32, i32)>,
        target: (i32, i32),
    ) -> Result<()> {
        let mut current = *path.last().context("Path is unexpectedly empty")?;
        if current.0 != target.0 && current.1 != target.1 {
            anyhow::bail!(
                "Deterministic router produced non-Manhattan waypoints {:?} -> {:?}",
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

    fn draw_segment(&mut self, from: (i32, i32), to: (i32, i32), net_index: usize) -> Result<()> {
        if from.0 != to.0 && from.1 != to.1 {
            anyhow::bail!("Non-Manhattan segment from {:?} to {:?}", from, to);
        }

        let dx = (to.0 - from.0).signum();
        let dy = (to.1 - from.1).signum();
        let dir = match (dx, dy) {
            (1, 0) => Dir::E,
            (-1, 0) => Dir::W,
            (0, 1) => Dir::S,
            (0, -1) => Dir::N,
            _ => return Ok(()),
        };

        let mut current = from;
        while current != to {
            let next = (current.0 + dx, current.1 + dy);
            self.check_route_cell(current, dir, net_index)?;
            self.check_route_cell(next, dir.opposite(), net_index)?;
            self.dirs.entry(current).or_default().insert(dir);
            self.dirs.entry(next).or_default().insert(dir.opposite());
            current = next;
        }
        Ok(())
    }

    fn check_route_cell(&mut self, cell: (i32, i32), dir: Dir, net_index: usize) -> Result<()> {
        if self.cells.contains_key(&cell) {
            return Ok(());
        }
        let ownership = self.route_owner.entry(cell).or_default();
        let axis_owner = match dir {
            Dir::E | Dir::W => &mut ownership.horizontal,
            Dir::N | Dir::S => &mut ownership.vertical,
        };
        match *axis_owner {
            None => {
                *axis_owner = Some(net_index);
                Ok(())
            }
            Some(owner) if owner == net_index => Ok(()),
            Some(_) => anyhow::bail!(
                "Deterministic router attempted to merge distinct nets through {:?}",
                cell
            ),
        }
    }

    fn instance(&self, id: InstanceId) -> Result<&MacroInstance> {
        self.construction
            .instances
            .iter()
            .find(|instance| instance.id == id)
            .with_context(|| format!("Unknown macro instance {}", id.0))
    }

    fn escape_point(&self, stub: Stub) -> (i32, i32) {
        let center = step(stub.coord, stub.back_dir);
        let outward = stub.back_dir.opposite();
        match outward {
            Dir::E => (center.0 + 3, center.1),
            Dir::W => (center.0 - 3, center.1),
            Dir::N => (center.0, center.1 - 3),
            Dir::S => (center.0, center.1 + 3),
        }
    }

    fn endpoint_bus_x(
        &self,
        min_x: i32,
        max_x: i32,
        outward: Dir,
        slot: i32,
        role: EndpointRole,
    ) -> i32 {
        let role_offset = match role {
            EndpointRole::Source => 0,
            EndpointRole::Target => 2,
        };
        match outward {
            Dir::W => min_x - self.channel_margin - slot - role_offset,
            Dir::E | Dir::N | Dir::S => max_x + self.channel_margin + slot + role_offset,
        }
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
    back_dir: Dir,
}

fn direction_between(from: (i32, i32), to: (i32, i32)) -> Result<Dir> {
    match (to.0 - from.0, to.1 - from.1) {
        (0, -1) => Ok(Dir::N),
        (1, 0) => Ok(Dir::E),
        (0, 1) => Ok(Dir::S),
        (-1, 0) => Ok(Dir::W),
        _ => anyhow::bail!("Cells {:?} and {:?} are not adjacent", from, to),
    }
}

fn step((x, y): (i32, i32), dir: Dir) -> (i32, i32) {
    match dir {
        Dir::N => (x, y - 1),
        Dir::E => (x + 1, y),
        Dir::S => (x, y + 1),
        Dir::W => (x - 1, y),
    }
}

fn instance_port_dir(instance: &MacroInstance, port: &str) -> Result<Dir> {
    match (&instance.kind, port) {
        (MacroKind::NotGate, "in") => Ok(Dir::W),
        (MacroKind::NotGate, "out") => Ok(Dir::E),
        (MacroKind::OrGate, "lhs") => Ok(Dir::N),
        (MacroKind::OrGate, "rhs") => Ok(Dir::S),
        (MacroKind::OrGate, "out") => Ok(Dir::E),
        (MacroKind::Splitter, "in") => Ok(Dir::S),
        (MacroKind::Splitter, "out0") => Ok(Dir::E),
        (MacroKind::Splitter, "out1") => Ok(Dir::N),
        (MacroKind::Enforcer, "in") => Ok(Dir::W),
        _ => anyhow::bail!("Unsupported port '{}' on {:?}", port, instance.kind),
    }
}

fn routing_spec_for_dirs(dirs: &BTreeSet<Dir>) -> Result<&'static str> {
    match dirs.iter().copied().collect::<Vec<_>>().as_slice() {
        [Dir::N, Dir::S] => Ok("vertical wire tile"),
        [Dir::E, Dir::W] => Ok("horizontal wire tile"),
        [Dir::N, Dir::E] => Ok("NE turn tile"),
        [Dir::N, Dir::W] => Ok("NW turn tile"),
        [Dir::S, Dir::W] => Ok("SW turn tile"),
        [Dir::E, Dir::S] => Ok("SE turn tile"),
        [Dir::N, Dir::E, Dir::S, Dir::W] => Ok("crossing tile"),
        _ => anyhow::bail!("No routing tile for direction set {:?}", dirs),
    }
}

#[derive(Debug, Clone, Copy)]
struct HorizontalPlacementCandidate {
    connector_delta: (i32, i32),
    right_delta: (i32, i32),
}

#[derive(Debug, Clone, Copy)]
struct VerticalPlacementCandidate {
    bottom_delta: (i32, i32),
}

fn known_horizontal_placement_candidates(
    left_spec: &str,
    connector_name: &str,
    right_spec: &str,
) -> &'static [HorizontalPlacementCandidate] {
    match (left_spec, connector_name, right_spec) {
        ("horizontal wire tile", "connector 0 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (258, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
        ],
        ("horizontal wire tile", "connector 0 to -1", "SW turn tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (258, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
        ],
        ("NOT gate tile", "connector 0 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("NOT gate tile", "connector 0 to 1", "NW turn tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 0),
            },
        ],
        ("OR gate tile", "connector 0 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("OR gate tile", "connector 0 to 0", "crossing tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("SE turn tile", "connector -1 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("SE turn tile", "connector -1 to 1", "NW turn tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 0),
            },
        ],
        ("NE turn tile", "connector 1 to 1", "NW turn tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 0),
            },
        ],
        ("crossing tile", "connector -1 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("horizontal wire tile", "connector 0 to 0", "crossing tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        ("horizontal wire tile", "connector 0 to 1", "NW turn tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 0),
            },
        ],
        ("horizontal wire tile", "connector 0 to 1", "NOT gate tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 0),
            },
        ],
        ("horizontal wire tile", "connector 0 to 1", "enforcer gadget") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 18),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 19),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 18),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 19),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 18),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 19),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 20),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (270, 21),
            },
        ],
        ("splitter tile", "connector -1 to 0", "horizontal wire tile") => &[
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (261, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (264, 0),
            },
            HorizontalPlacementCandidate {
                connector_delta: (90, 0),
                right_delta: (267, 0),
            },
        ],
        _ => &[],
    }
}

fn known_vertical_placement_candidates(
    top_spec: &str,
    bottom_spec: &str,
) -> &'static [VerticalPlacementCandidate] {
    match (top_spec, bottom_spec) {
        ("SW turn tile", "OR gate tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("OR gate tile", "NW turn tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 82),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 85),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 88),
            },
        ],
        ("SE turn tile", "NW turn tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 82),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 85),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 88),
            },
        ],
        ("SE turn tile", "vertical wire tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("SE turn tile", "OR gate tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("SE turn tile", "crossing tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("SW turn tile", "NE turn tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 82),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 85),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 88),
            },
        ],
        ("SW turn tile", "vertical wire tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("crossing tile", "vertical wire tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("crossing tile", "splitter tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 81),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 84),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 87),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 90),
            },
        ],
        ("splitter tile", "NW turn tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 82),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 85),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 88),
            },
        ],
        ("vertical wire tile", "vertical wire tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 78),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 81),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 84),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 87),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 90),
            },
        ],
        ("vertical wire tile", "NW turn tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 80),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 83),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 86),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 89),
            },
        ],
        ("vertical wire tile", "crossing tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 78),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 81),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 84),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 87),
            },
            VerticalPlacementCandidate {
                bottom_delta: (0, 90),
            },
        ],
        _ => &[],
    }
}

fn default_horizontal_placement_candidate(
    left_origin: (i32, i32),
    left_anchor: CellCoord,
    right_anchor: CellCoord,
    connector_west: CellCoord,
    connector_east: CellCoord,
) -> HorizontalPlacementCandidate {
    let connector_origin = (
        left_origin.0 + left_anchor.x as i32 - connector_west.x as i32,
        left_origin.1 + left_anchor.y as i32 - connector_west.y as i32,
    );
    let right_origin = (
        connector_origin.0 + connector_east.x as i32 - right_anchor.x as i32,
        connector_origin.1 + connector_east.y as i32 - right_anchor.y as i32,
    );

    HorizontalPlacementCandidate {
        connector_delta: (
            connector_origin.0 - left_origin.0,
            connector_origin.1 - left_origin.1,
        ),
        right_delta: (right_origin.0 - left_origin.0, right_origin.1 - left_origin.1),
    }
}

fn default_vertical_placement_candidate(
    top_origin: (i32, i32),
    top_anchor: CellCoord,
    bottom_anchor: CellCoord,
) -> VerticalPlacementCandidate {
    let bottom_origin = (
        top_origin.0 + top_anchor.x as i32 - bottom_anchor.x as i32,
        top_origin.1 + top_anchor.y as i32 - bottom_anchor.y as i32,
    );

    VerticalPlacementCandidate {
        bottom_delta: (
            bottom_origin.0 - top_origin.0,
            bottom_origin.1 - top_origin.1,
        ),
    }
}

struct PlacementSolver<'a> {
    library: &'a PublishedLibrary,
    pieces: HashMap<(i32, i32), PiecePlacement>,
    vertical_links: Vec<((i32, i32), (i32, i32))>,
    horizontal_links: Vec<((i32, i32), (i32, i32))>,
}

#[derive(Debug, Clone)]
struct PiecePlacement {
    spec_name: String,
    origin: Option<(i32, i32)>,
}

#[derive(Debug, Default)]
struct OriginSearchStats {
    visited_states: usize,
    reported_states: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct HorizontalFamilyKey {
    left_spec: String,
    connector_name: String,
    right_spec: String,
    lane_y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct VerticalFamilyKey {
    top_spec: String,
    bottom_spec: String,
    lane_x: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VerticalOrHorizontalFamily {
    Horizontal(HorizontalFamilyKey),
    Vertical(VerticalFamilyKey),
}

impl<'a> PlacementSolver<'a> {
    fn new(library: &'a PublishedLibrary) -> Self {
        Self {
            library,
            pieces: HashMap::new(),
            vertical_links: Vec::new(),
            horizontal_links: Vec::new(),
        }
    }

    fn add_piece(&mut self, cell: (i32, i32), spec_name: &str) -> Result<()> {
        if let Some(existing) = self.pieces.get(&cell) {
            if existing.spec_name != spec_name {
                anyhow::bail!(
                    "Conflicting piece assignment at {:?}: '{}' vs '{}'",
                    cell,
                    existing.spec_name,
                    spec_name
                );
            }
            return Ok(());
        }
        self.pieces.insert(
            cell,
            PiecePlacement {
                spec_name: spec_name.to_string(),
                origin: None,
            },
        );
        Ok(())
    }

    fn add_vertical_adjacency(&mut self, top: (i32, i32), bottom: (i32, i32)) -> Result<()> {
        if !self.pieces.contains_key(&top) || !self.pieces.contains_key(&bottom) {
            return Ok(());
        }
        self.vertical_links.push((top, bottom));
        Ok(())
    }

    fn add_horizontal_adjacency(&mut self, left: (i32, i32), right: (i32, i32)) -> Result<()> {
        if !self.pieces.contains_key(&left) || !self.pieces.contains_key(&right) {
            return Ok(());
        }
        self.horizontal_links.push((left, right));
        Ok(())
    }

    fn materialize(mut self) -> Result<PublishedBoard> {
        let solved_origins = self.solve_origins()?;
        for (cell, origin) in solved_origins {
            self.pieces.get_mut(&cell).unwrap().origin = Some(origin);
        }

        let min_x = self
            .pieces
            .values()
            .filter_map(|piece| piece.origin.map(|(x, _)| x))
            .min()
            .unwrap_or(0);
        let min_y = self
            .pieces
            .values()
            .filter_map(|piece| piece.origin.map(|(_, y)| y))
            .min()
            .unwrap_or(0);

        let mut board_pieces = Vec::new();
        let mut max_x = 0i32;
        let mut max_y = 0i32;
        for piece in self.pieces.values() {
            let info = self.library.pattern(&piece.spec_name)?;
            let (origin_x, origin_y) = piece.origin.context("Unplaced board piece")?;
            let ox = origin_x - min_x;
            let oy = origin_y - min_y;
            max_x = max_x.max(ox + info.pattern.width as i32);
            max_y = max_y.max(oy + info.pattern.height as i32);
            board_pieces.push(BoardPiece {
                spec_name: piece.spec_name.clone(),
                origin_x: ox as usize,
                origin_y: oy as usize,
                width: info.pattern.width,
                height: info.pattern.height,
            });
        }

        for &(left, right) in &self.horizontal_links {
            let connector = self.compute_connector_piece(left, right, min_x, min_y)?;
            max_x = max_x.max(connector.origin_x as i32 + connector.width as i32);
            max_y = max_y.max(connector.origin_y as i32 + connector.height as i32);
            board_pieces.push(connector);
        }

        let mut target = Grid::new(max_x as usize, max_y as usize, BoundaryCondition::Dead);
        for piece in &board_pieces {
            let info = self.library.pattern(&piece.spec_name)?;
            for (y, row) in info.pattern.cells.iter().enumerate() {
                for (x, &cell) in row.iter().enumerate() {
                    if cell > 0 {
                        target.set(piece.origin_y + y, piece.origin_x + x, true)?;
                    }
                }
            }
        }

        Ok(PublishedBoard {
            pieces: board_pieces,
            target,
        })
    }

    fn solve_origins(&self) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        match self.solve_family_choices() {
            Ok(origins) => Ok(origins),
            Err(_) => self.solve_origins_exact(),
        }
    }

    fn solve_origins_exact(&self) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let mut piece_cells = self.pieces.keys().copied().collect::<Vec<_>>();
        piece_cells.sort_unstable();
        let root = *piece_cells
            .first()
            .context("No pieces available for placement solving")?;
        let mut assigned = HashMap::new();
        assigned.insert(root, (0, 0));
        let mut dead_states = HashSet::new();
        let mut stats = OriginSearchStats::default();
        self.search_origins(&piece_cells, &mut assigned, &mut dead_states, &mut stats)?
            .context("Exact placement search did not find a consistent board embedding")
    }

    fn solve_family_choices(&self) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let mut horizontal_domains = BTreeMap::new();
        for &(left, right) in &self.horizontal_links {
            let (family, candidates) = self.horizontal_family_candidates(left, right)?;
            horizontal_domains.entry(family).or_insert(candidates);
        }

        let mut vertical_domains = BTreeMap::new();
        for &(top, bottom) in &self.vertical_links {
            let (family, candidates) = self.vertical_family_candidates(top, bottom)?;
            vertical_domains.entry(family).or_insert(candidates);
        }

        let mut horizontal_choices = BTreeMap::new();
        let mut vertical_choices = BTreeMap::new();
        let mut dead_states = HashSet::new();
        self.search_family_choices(
            &horizontal_domains,
            &vertical_domains,
            &mut horizontal_choices,
            &mut vertical_choices,
            &mut dead_states,
        )?
        .context("No globally coherent placement-family selection found")
    }

    fn search_family_choices(
        &self,
        horizontal_domains: &BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
        vertical_domains: &BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
        dead_states: &mut HashSet<String>,
    ) -> Result<Option<HashMap<(i32, i32), (i32, i32)>>> {
        if self
            .propagate_partial_family_choices(horizontal_choices, vertical_choices)
            .is_err()
        {
            return Ok(None);
        }

        if !self.propagate_forced_family_choices(
            horizontal_domains,
            vertical_domains,
            horizontal_choices,
            vertical_choices,
        )? {
            return Ok(None);
        }

        let state_key = self.family_choice_state_key(horizontal_choices, vertical_choices);
        if dead_states.contains(&state_key) {
            return Ok(None);
        }

        if horizontal_choices.len() == horizontal_domains.len()
            && vertical_choices.len() == vertical_domains.len()
        {
            return Ok(self
                .solve_origins_with_choices(horizontal_choices, vertical_choices)
                .ok());
        }

        let next_horizontal = horizontal_domains
            .iter()
            .filter(|(family, _)| !horizontal_choices.contains_key(*family))
            .filter_map(|(family, _)| {
                let viable = self
                    .viable_horizontal_candidates(
                        family,
                        horizontal_domains,
                        vertical_domains,
                        horizontal_choices,
                        vertical_choices,
                    )
                    .ok()?;
                if viable.is_empty() {
                    None
                } else {
                    Some((
                        true,
                        VerticalOrHorizontalFamily::Horizontal(family.clone()),
                        viable,
                    ))
                }
            });
        let next_vertical = vertical_domains
            .iter()
            .filter(|(family, _)| !vertical_choices.contains_key(*family))
            .filter_map(|(family, _)| {
                let viable = self
                    .viable_vertical_candidates(
                        family,
                        horizontal_domains,
                        vertical_domains,
                        horizontal_choices,
                        vertical_choices,
                    )
                    .ok()?;
                if viable.is_empty() {
                    None
                } else {
                    Some((
                        false,
                        VerticalOrHorizontalFamily::Vertical(family.clone()),
                        viable,
                    ))
                }
            });

        let next_family = next_horizontal
            .chain(next_vertical)
            .min_by(|a, b| a.2.len().cmp(&b.2.len()).then_with(|| a.1.cmp(&b.1)));

        let Some((is_horizontal, family, viable_candidates)) = next_family else {
            return Ok(Some(self.solve_origins_with_choices(
                horizontal_choices,
                vertical_choices,
            )?));
        };

        match (is_horizontal, family) {
            (true, VerticalOrHorizontalFamily::Horizontal(family)) => {
                for candidate in viable_candidates {
                    horizontal_choices.insert(family.clone(), candidate);
                    if let Some(solution) = self.search_family_choices(
                        horizontal_domains,
                        vertical_domains,
                        horizontal_choices,
                        vertical_choices,
                        dead_states,
                    )? {
                        return Ok(Some(solution));
                    }
                    horizontal_choices.remove(&family);
                }
            }
            (false, VerticalOrHorizontalFamily::Vertical(family)) => {
                for candidate in viable_candidates {
                    vertical_choices.insert(family.clone(), candidate);
                    if let Some(solution) = self.search_family_choices(
                        horizontal_domains,
                        vertical_domains,
                        horizontal_choices,
                        vertical_choices,
                        dead_states,
                    )? {
                        return Ok(Some(solution));
                    }
                    vertical_choices.remove(&family);
                }
            }
            _ => unreachable!("Family kind mismatch in canonical placement search"),
        }

        dead_states.insert(state_key);
        Ok(None)
    }

    fn family_choice_state_key(
        &self,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> String {
        let mut key = String::new();
        for (family, delta) in horizontal_choices {
            key.push_str("H:");
            key.push_str(&family.left_spec);
            key.push('|');
            key.push_str(&family.connector_name);
            key.push('|');
            key.push_str(&family.right_spec);
            key.push('|');
            key.push_str(&family.lane_y.to_string());
            key.push('|');
            key.push_str(&delta.0.to_string());
            key.push('|');
            key.push_str(&delta.1.to_string());
            key.push(';');
        }
        for (family, delta) in vertical_choices {
            key.push_str("V:");
            key.push_str(&family.top_spec);
            key.push('|');
            key.push_str(&family.bottom_spec);
            key.push('|');
            key.push_str(&family.lane_x.to_string());
            key.push('|');
            key.push_str(&delta.0.to_string());
            key.push('|');
            key.push_str(&delta.1.to_string());
            key.push(';');
        }
        key
    }

    fn propagate_forced_family_choices(
        &self,
        horizontal_domains: &BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
        vertical_domains: &BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<bool> {
        loop {
            let mut progress = false;

            for family in horizontal_domains.keys() {
                if horizontal_choices.contains_key(family) {
                    continue;
                }
                let viable = self.viable_horizontal_candidates(
                    family,
                    horizontal_domains,
                    vertical_domains,
                    horizontal_choices,
                    vertical_choices,
                )?;
                if viable.is_empty() {
                    return Ok(false);
                }
                if viable.len() == 1 {
                    horizontal_choices.insert(family.clone(), viable[0]);
                    progress = true;
                }
            }

            for family in vertical_domains.keys() {
                if vertical_choices.contains_key(family) {
                    continue;
                }
                let viable = self.viable_vertical_candidates(
                    family,
                    horizontal_domains,
                    vertical_domains,
                    horizontal_choices,
                    vertical_choices,
                )?;
                if viable.is_empty() {
                    return Ok(false);
                }
                if viable.len() == 1 {
                    vertical_choices.insert(family.clone(), viable[0]);
                    progress = true;
                }
            }

            if !progress {
                return Ok(true);
            }

            if self
                .propagate_partial_family_choices(horizontal_choices, vertical_choices)
                .is_err()
            {
                return Ok(false);
            }
        }
    }

    fn viable_horizontal_candidates(
        &self,
        family: &HorizontalFamilyKey,
        _horizontal_domains: &BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
        _vertical_domains: &BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<Vec<(i32, i32)>> {
        let candidates = known_horizontal_placement_candidates(
            &family.left_spec,
            &family.connector_name,
            &family.right_spec,
        )
        .iter()
        .map(|candidate| candidate.right_delta)
        .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let mut viable = Vec::new();
        for candidate in candidates {
            let mut trial_h = horizontal_choices.clone();
            trial_h.insert(family.clone(), candidate);
            if self
                .propagate_partial_family_choices(&trial_h, vertical_choices)
                .is_ok()
            {
                viable.push(candidate);
            }
        }
        Ok(viable)
    }

    fn viable_vertical_candidates(
        &self,
        family: &VerticalFamilyKey,
        _horizontal_domains: &BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
        _vertical_domains: &BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<Vec<(i32, i32)>> {
        let candidates = known_vertical_placement_candidates(&family.top_spec, &family.bottom_spec)
            .iter()
            .map(|candidate| candidate.bottom_delta)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let mut viable = Vec::new();
        for candidate in candidates {
            let mut trial_v = vertical_choices.clone();
            trial_v.insert(family.clone(), candidate);
            if self
                .propagate_partial_family_choices(horizontal_choices, &trial_v)
                .is_ok()
            {
                viable.push(candidate);
            }
        }
        Ok(viable)
    }

    fn propagate_partial_family_choices(
        &self,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<()> {
        let mut piece_cells = self.pieces.keys().copied().collect::<Vec<_>>();
        piece_cells.sort_unstable();
        let root = *piece_cells
            .first()
            .context("No pieces available for placement solving")?;
        let mut origins = HashMap::new();
        origins.insert(root, (0, 0));

        let mut queue = VecDeque::new();
        for &(left, right) in &self.horizontal_links {
            queue.push_back((true, left, right));
        }
        for &(top, bottom) in &self.vertical_links {
            queue.push_back((false, top, bottom));
        }

        let mut stalled = 0usize;
        while let Some((horizontal, a, b)) = queue.pop_front() {
            let maybe_delta = if horizontal {
                let (family, _) = self.horizontal_family_candidates(a, b)?;
                horizontal_choices.get(&family).copied()
            } else {
                let (family, _) = self.vertical_family_candidates(a, b)?;
                vertical_choices.get(&family).copied()
            };

            let Some(delta) = maybe_delta else {
                queue.push_back((horizontal, a, b));
                stalled += 1;
                if stalled > queue.len().saturating_add(1) {
                    break;
                }
                continue;
            };

            match (origins.get(&a).copied(), origins.get(&b).copied()) {
                (Some((ax, ay)), None) => {
                    origins.insert(b, (ax + delta.0, ay + delta.1));
                    stalled = 0;
                }
                (None, Some((bx, by))) => {
                    origins.insert(a, (bx - delta.0, by - delta.1));
                    stalled = 0;
                }
                (Some((ax, ay)), Some((bx, by))) => {
                    if (ax + delta.0, ay + delta.1) != (bx, by) {
                        let a_name = &self.pieces[&a].spec_name;
                        let b_name = &self.pieces[&b].spec_name;
                        anyhow::bail!(
                            "Deterministic placement conflict for {:?} [{}] and {:?} [{}]: expected {:?}, found {:?}",
                            a,
                            a_name,
                            b,
                            b_name,
                            (ax + delta.0, ay + delta.1),
                            (bx, by)
                        );
                    }
                }
                (None, None) => {
                    queue.push_back((horizontal, a, b));
                    stalled += 1;
                    if stalled > queue.len().saturating_add(1) {
                        break;
                    }
                }
            }
        }

        for &(left, right) in &self.horizontal_links {
            let (family, candidates) = self.horizontal_family_candidates(left, right)?;
            if let (Some(left_origin), Some(right_origin)) =
                (origins.get(&left).copied(), origins.get(&right).copied())
            {
                let valid = match horizontal_choices.get(&family).copied() {
                    Some(delta) => (left_origin.0 + delta.0, left_origin.1 + delta.1) == right_origin,
                    None => candidates
                        .iter()
                        .any(|delta| (left_origin.0 + delta.0, left_origin.1 + delta.1) == right_origin),
                };
                if !valid {
                    anyhow::bail!(
                        "Partial horizontal placement choices are inconsistent for {:?} [{}] -> {:?} [{}]",
                        left,
                        self.pieces[&left].spec_name,
                        right,
                        self.pieces[&right].spec_name
                    );
                }
            }
        }

        for &(top, bottom) in &self.vertical_links {
            let (family, candidates) = self.vertical_family_candidates(top, bottom)?;
            if let (Some(top_origin), Some(bottom_origin)) =
                (origins.get(&top).copied(), origins.get(&bottom).copied())
            {
                let valid = match vertical_choices.get(&family).copied() {
                    Some(delta) => {
                        (top_origin.0 + delta.0, top_origin.1 + delta.1) == bottom_origin
                    }
                    None => candidates
                        .iter()
                        .any(|delta| (top_origin.0 + delta.0, top_origin.1 + delta.1) == bottom_origin),
                };
                if !valid {
                    anyhow::bail!(
                        "Partial vertical placement choices are inconsistent for {:?} [{}] -> {:?} [{}]",
                        top,
                        self.pieces[&top].spec_name,
                        bottom,
                        self.pieces[&bottom].spec_name
                    );
                }
            }
        }

        Ok(())
    }

    fn solve_origins_with_choices(
        &self,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let mut piece_cells = self.pieces.keys().copied().collect::<Vec<_>>();
        piece_cells.sort_unstable();
        let root = *piece_cells
            .first()
            .context("No pieces available for placement solving")?;
        let mut origins = HashMap::new();
        origins.insert(root, (0, 0));

        let mut queue = VecDeque::new();
        for &(left, right) in &self.horizontal_links {
            queue.push_back((true, left, right));
        }
        for &(top, bottom) in &self.vertical_links {
            queue.push_back((false, top, bottom));
        }

        let mut stalled = 0usize;
        while let Some((horizontal, a, b)) = queue.pop_front() {
            let delta = if horizontal {
                let (family, _) = self.horizontal_family_candidates(a, b)?;
                horizontal_choices[&family]
            } else {
                let (family, _) = self.vertical_family_candidates(a, b)?;
                vertical_choices[&family]
            };

            match (origins.get(&a).copied(), origins.get(&b).copied()) {
                (Some((ax, ay)), None) => {
                    origins.insert(b, (ax + delta.0, ay + delta.1));
                    stalled = 0;
                }
                (None, Some((bx, by))) => {
                    origins.insert(a, (bx - delta.0, by - delta.1));
                    stalled = 0;
                }
                (Some((ax, ay)), Some((bx, by))) => {
                    if (ax + delta.0, ay + delta.1) != (bx, by) {
                        let a_name = &self.pieces[&a].spec_name;
                        let b_name = &self.pieces[&b].spec_name;
                        anyhow::bail!(
                            "Deterministic placement conflict for {:?} [{}] and {:?} [{}]: expected {:?}, found {:?}",
                            a,
                            a_name,
                            b,
                            b_name,
                            (ax + delta.0, ay + delta.1),
                            (bx, by)
                        );
                    }
                }
                (None, None) => {
                    queue.push_back((horizontal, a, b));
                    stalled += 1;
                    if stalled > queue.len().saturating_add(1) {
                        anyhow::bail!(
                            "Deterministic placement propagation stalled before all pieces were placed"
                        );
                    }
                }
            }
        }

        if origins.len() != piece_cells.len() {
            anyhow::bail!("Deterministic placement left some board pieces unassigned");
        }

        Ok(origins)
    }

    fn horizontal_family_candidates(
        &self,
        left: (i32, i32),
        right: (i32, i32),
    ) -> Result<(HorizontalFamilyKey, Vec<(i32, i32)>)> {
        let left_piece = self.pieces.get(&left).unwrap();
        let right_piece = self.pieces.get(&right).unwrap();
        let left_info = self.library.pattern(&left_piece.spec_name)?;
        let right_info = self.library.pattern(&right_piece.spec_name)?;
        let left_anchor = left_info.anchors.east.context("Missing east anchor")?;
        let right_anchor = right_info.anchors.west.context("Missing west anchor")?;
        let left_align = left_info.pattern.phase_alignment().0.context("Missing east phase")?;
        let right_align = right_info.pattern.phase_alignment().2.context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info.anchors.west.context("Missing connector west anchor")?;
        let connector_east = connector_info.anchors.east.context("Missing connector east anchor")?;
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        if candidates.is_empty() {
            candidates.push(default_horizontal_placement_candidate(
                (0, 0),
                left_anchor,
                right_anchor,
                connector_west,
                connector_east,
            ));
        }
        Ok((
            HorizontalFamilyKey {
                left_spec: left_piece.spec_name.clone(),
                connector_name: connector_name.to_string(),
                right_spec: right_piece.spec_name.clone(),
                lane_y: left.1,
            },
            candidates.into_iter().map(|candidate| candidate.right_delta).collect(),
        ))
    }

    fn vertical_family_candidates(
        &self,
        top: (i32, i32),
        bottom: (i32, i32),
    ) -> Result<(VerticalFamilyKey, Vec<(i32, i32)>)> {
        let top_piece = self.pieces.get(&top).unwrap();
        let bottom_piece = self.pieces.get(&bottom).unwrap();
        let top_info = self.library.pattern(&top_piece.spec_name)?;
        let bottom_info = self.library.pattern(&bottom_piece.spec_name)?;
        let top_anchor = top_info.anchors.south.context("Missing south anchor")?;
        let bottom_anchor = bottom_info.anchors.north.context("Missing north anchor")?;
        let mut candidates =
            known_vertical_placement_candidates(&top_piece.spec_name, &bottom_piece.spec_name)
                .to_vec();
        if candidates.is_empty() {
            candidates.push(default_vertical_placement_candidate(
                (0, 0),
                top_anchor,
                bottom_anchor,
            ));
        }
        Ok((
            VerticalFamilyKey {
                top_spec: top_piece.spec_name.clone(),
                bottom_spec: bottom_piece.spec_name.clone(),
                lane_x: top.0,
            },
            candidates.into_iter().map(|candidate| candidate.bottom_delta).collect(),
        ))
    }

    fn search_origins(
        &self,
        piece_cells: &[(i32, i32)],
        assigned: &mut HashMap<(i32, i32), (i32, i32)>,
        dead_states: &mut HashSet<Vec<(i32, i32, i32, i32)>>,
        stats: &mut OriginSearchStats,
    ) -> Result<Option<HashMap<(i32, i32), (i32, i32)>>> {
        stats.visited_states += 1;
        if stats.visited_states >= stats.reported_states + 10_000 {
            stats.reported_states = stats.visited_states;
            eprintln!(
                "[rev_gol_proof] exact placement search: visited={} assigned={} dead_states={}",
                stats.visited_states,
                assigned.len(),
                dead_states.len()
            );
        }
        let mut implied = Vec::new();
        if !self.propagate_forced(piece_cells, assigned, &mut implied)? {
            self.undo_implied_assignments(assigned, &implied);
            return Ok(None);
        }

        if assigned.len() == piece_cells.len() {
            return Ok(Some(assigned.clone()));
        }

        let state_key = self.assignment_state_key(assigned);
        if dead_states.contains(&state_key) {
            self.undo_implied_assignments(assigned, &implied);
            return Ok(None);
        }

        let mut next_piece = None;
        let mut next_candidates = Vec::new();
        let mut next_assigned_neighbors = 0usize;
        let mut next_degree = 0usize;

        for &cell in piece_cells {
            if assigned.contains_key(&cell) {
                continue;
            }
            let constraints = self.neighbor_constraints(cell)?;
            let has_assigned_neighbor = constraints
                .iter()
                .any(|(neighbor, _)| assigned.contains_key(neighbor));
            let candidates = self.candidate_origins_from_constraints(&constraints, assigned);
            if has_assigned_neighbor && candidates.is_empty() {
                dead_states.insert(state_key);
                self.undo_implied_assignments(assigned, &implied);
                return Ok(None);
            }
            if candidates.is_empty() {
                continue;
            }
            let assigned_neighbor_count = constraints
                .iter()
                .filter(|(neighbor, _)| assigned.contains_key(neighbor))
                .count();
            let degree = constraints.len();
            let better = next_piece.is_none()
                || candidates.len() < next_candidates.len()
                || (candidates.len() == next_candidates.len()
                    && assigned_neighbor_count > next_assigned_neighbors)
                || (candidates.len() == next_candidates.len()
                    && assigned_neighbor_count == next_assigned_neighbors
                    && degree > next_degree);
            if better {
                next_piece = Some(cell);
                next_candidates = candidates;
                next_assigned_neighbors = assigned_neighbor_count;
                next_degree = degree;
            }
        }

        let Some(piece) = next_piece else {
            self.undo_implied_assignments(assigned, &implied);
            anyhow::bail!("Disconnected placement graph is not yet supported");
        };

        let ordered_candidates = self.order_candidate_origins(piece, next_candidates, assigned)?;
        for candidate in ordered_candidates {
            assigned.insert(piece, candidate);
            if self.is_consistent_assignment(piece, assigned)?
                && self.forward_check(piece_cells, assigned)?
            {
                if let Some(solution) =
                    self.search_origins(piece_cells, assigned, dead_states, stats)?
                {
                    return Ok(Some(solution));
                }
            }
            assigned.remove(&piece);
        }
        dead_states.insert(state_key);
        self.undo_implied_assignments(assigned, &implied);
        Ok(None)
    }

    fn candidate_origins_from_constraints(
        &self,
        constraints: &[((i32, i32), Vec<(i32, i32)>)],
        assigned: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Vec<(i32, i32)> {
        let mut candidate_sets = Vec::<BTreeSet<(i32, i32)>>::new();
        for (neighbor, deltas) in constraints {
            if let Some(&neighbor_origin) = assigned.get(neighbor) {
                let set = deltas
                    .iter()
                    .map(|delta| (neighbor_origin.0 + delta.0, neighbor_origin.1 + delta.1))
                    .collect::<BTreeSet<_>>();
                candidate_sets.push(set);
            }
        }

        if candidate_sets.is_empty() {
            return Vec::new();
        }

        let mut intersection = candidate_sets.remove(0);
        for set in candidate_sets {
            intersection = intersection
                .intersection(&set)
                .copied()
                .collect::<BTreeSet<_>>();
        }

        intersection.into_iter().collect()
    }

    fn order_candidate_origins(
        &self,
        piece: (i32, i32),
        mut candidates: Vec<(i32, i32)>,
        assigned: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Result<Vec<(i32, i32)>> {
        let mut scored = Vec::with_capacity(candidates.len());
        for candidate in candidates.drain(..) {
            let mut trial = assigned.clone();
            trial.insert(piece, candidate);
            let mut future_options = 0usize;
            for (neighbor, _) in self.neighbor_constraints(piece)? {
                if trial.contains_key(&neighbor) {
                    continue;
                }
                let constraints = self.neighbor_constraints(neighbor)?;
                future_options += self
                    .candidate_origins_from_constraints(&constraints, &trial)
                    .len();
            }
            scored.push((future_options, candidate));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(scored.into_iter().map(|(_, candidate)| candidate).collect())
    }

    fn propagate_forced(
        &self,
        piece_cells: &[(i32, i32)],
        assigned: &mut HashMap<(i32, i32), (i32, i32)>,
        implied: &mut Vec<(i32, i32)>,
    ) -> Result<bool> {
        loop {
            let mut progress = false;
            for &cell in piece_cells {
                if assigned.contains_key(&cell) {
                    continue;
                }
                let constraints = self.neighbor_constraints(cell)?;
                let has_assigned_neighbor = constraints
                    .iter()
                    .any(|(neighbor, _)| assigned.contains_key(neighbor));
                if !has_assigned_neighbor {
                    continue;
                }
                let candidates = self.candidate_origins_from_constraints(&constraints, assigned);
                if candidates.is_empty() {
                    return Ok(false);
                }
                if candidates.len() == 1 {
                    assigned.insert(cell, candidates[0]);
                    implied.push(cell);
                    progress = true;
                }
            }
            if !progress {
                return Ok(true);
            }
        }
    }

    fn undo_implied_assignments(
        &self,
        assigned: &mut HashMap<(i32, i32), (i32, i32)>,
        implied: &[(i32, i32)],
    ) {
        for cell in implied.iter().rev() {
            assigned.remove(cell);
        }
    }

    fn forward_check(
        &self,
        piece_cells: &[(i32, i32)],
        assigned: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Result<bool> {
        for &cell in piece_cells {
            if assigned.contains_key(&cell) {
                continue;
            }
            let constraints = self.neighbor_constraints(cell)?;
            let has_assigned_neighbor = constraints
                .iter()
                .any(|(neighbor, _)| assigned.contains_key(neighbor));
            if has_assigned_neighbor
                && self
                    .candidate_origins_from_constraints(&constraints, assigned)
                    .is_empty()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn assignment_state_key(
        &self,
        assigned: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Vec<(i32, i32, i32, i32)> {
        let mut items = assigned
            .iter()
            .map(|(&(cell_x, cell_y), &(origin_x, origin_y))| {
                (cell_x, cell_y, origin_x, origin_y)
            })
            .collect::<Vec<_>>();
        items.sort_unstable();
        items
    }

    fn is_consistent_assignment(
        &self,
        piece: (i32, i32),
        assigned: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Result<bool> {
        let piece_origin = assigned[&piece];
        for (neighbor, deltas) in self.neighbor_constraints(piece)? {
            if let Some(&neighbor_origin) = assigned.get(&neighbor) {
                if !deltas.iter().any(|delta| {
                    (neighbor_origin.0 + delta.0, neighbor_origin.1 + delta.1) == piece_origin
                }) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn neighbor_constraints(
        &self,
        piece: (i32, i32),
    ) -> Result<Vec<((i32, i32), Vec<(i32, i32)>)>> {
        let mut neighbors = Vec::new();

        for &(left, right) in &self.horizontal_links {
            if right == piece {
                neighbors.push((left, self.horizontal_deltas(left, right)?));
            } else if left == piece {
                neighbors.push((
                    right,
                    self.horizontal_deltas(left, right)?
                        .into_iter()
                        .map(|delta| (-delta.0, -delta.1))
                        .collect(),
                ));
            }
        }

        for &(top, bottom) in &self.vertical_links {
            if bottom == piece {
                neighbors.push((top, self.vertical_deltas(top, bottom)?));
            } else if top == piece {
                neighbors.push((
                    bottom,
                    self.vertical_deltas(top, bottom)?
                        .into_iter()
                        .map(|delta| (-delta.0, -delta.1))
                        .collect(),
                ));
            }
        }

        Ok(neighbors)
    }

    fn horizontal_deltas(&self, left: (i32, i32), right: (i32, i32)) -> Result<Vec<(i32, i32)>> {
        let left_piece = self.pieces.get(&left).unwrap();
        let right_piece = self.pieces.get(&right).unwrap();
        let left_info = self.library.pattern(&left_piece.spec_name)?;
        let right_info = self.library.pattern(&right_piece.spec_name)?;
        let left_anchor = left_info.anchors.east.context("Missing east anchor")?;
        let right_anchor = right_info.anchors.west.context("Missing west anchor")?;
        let left_align = left_info.pattern.phase_alignment().0.context("Missing east phase")?;
        let right_align = right_info.pattern.phase_alignment().2.context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info.anchors.west.context("Missing connector west anchor")?;
        let connector_east = connector_info.anchors.east.context("Missing connector east anchor")?;
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        if candidates.is_empty() {
            candidates.push(default_horizontal_placement_candidate(
                (0, 0),
                left_anchor,
                right_anchor,
                connector_west,
                connector_east,
            ));
        }
        Ok(candidates
            .into_iter()
            .map(|candidate| candidate.right_delta)
            .collect())
    }

    fn vertical_deltas(&self, top: (i32, i32), bottom: (i32, i32)) -> Result<Vec<(i32, i32)>> {
        let top_piece = self.pieces.get(&top).unwrap();
        let bottom_piece = self.pieces.get(&bottom).unwrap();
        let top_info = self.library.pattern(&top_piece.spec_name)?;
        let bottom_info = self.library.pattern(&bottom_piece.spec_name)?;
        let top_anchor = top_info.anchors.south.context("Missing south anchor")?;
        let bottom_anchor = bottom_info.anchors.north.context("Missing north anchor")?;
        let mut candidates =
            known_vertical_placement_candidates(&top_piece.spec_name, &bottom_piece.spec_name)
                .to_vec();
        if candidates.is_empty() {
            candidates.push(default_vertical_placement_candidate(
                (0, 0),
                top_anchor,
                bottom_anchor,
            ));
        }
        Ok(candidates
            .into_iter()
            .map(|candidate| candidate.bottom_delta)
            .collect())
    }

    fn compute_connector_piece(
        &self,
        left: (i32, i32),
        right: (i32, i32),
        min_x: i32,
        min_y: i32,
    ) -> Result<BoardPiece> {
        let left_piece = self.pieces.get(&left).unwrap();
        let right_piece = self.pieces.get(&right).unwrap();
        let left_info = self.library.pattern(&left_piece.spec_name)?;
        let right_info = self.library.pattern(&right_piece.spec_name)?;
        let left_origin = left_piece.origin.context("Unplaced left piece")?;
        let right_origin = right_piece.origin.context("Unplaced right piece")?;
        let left_anchor = left_info.anchors.east.context("Missing east anchor")?;
        let right_anchor = right_info.anchors.west.context("Missing west anchor")?;
        let left_align = left_info.pattern.phase_alignment().0.context("Missing east phase")?;
        let right_align = right_info.pattern.phase_alignment().2.context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info.anchors.west.context("Missing connector west anchor")?;
        let connector_east = connector_info.anchors.east.context("Missing connector east anchor")?;
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        candidates.push(default_horizontal_placement_candidate(
            left_origin,
            left_anchor,
            right_anchor,
            connector_west,
            connector_east,
        ));

        let Some(candidate) = candidates.into_iter().find(|candidate| {
            (
                left_origin.0 + candidate.right_delta.0,
                left_origin.1 + candidate.right_delta.1,
            ) == right_origin
        }) else {
            anyhow::bail!(
                "Connector '{}' placement between {:?} [{} @ {:?}] and {:?} [{} @ {:?}] is inconsistent: expected right origin {:?}, actual {:?}",
                connector_name,
                left,
                left_piece.spec_name,
                left_origin,
                right,
                right_piece.spec_name,
                right_origin,
                (
                    left_origin.0 + default_horizontal_placement_candidate(
                        left_origin,
                        left_anchor,
                        right_anchor,
                        connector_west,
                        connector_east,
                    )
                    .right_delta
                    .0,
                    left_origin.1 + default_horizontal_placement_candidate(
                        left_origin,
                        left_anchor,
                        right_anchor,
                        connector_west,
                        connector_east,
                    )
                    .right_delta
                    .1,
                ),
                right_origin
            );
        };

        let connector_origin = (
            left_origin.0 + candidate.connector_delta.0,
            left_origin.1 + candidate.connector_delta.1,
        );

        Ok(BoardPiece {
            spec_name: connector_name.to_string(),
            origin_x: (connector_origin.0 - min_x) as usize,
            origin_y: (connector_origin.1 - min_y) as usize,
            width: connector_info.pattern.width,
            height: connector_info.pattern.height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;
    use rev_gol::game_of_life::io::parse_grid_from_string;

    #[test]
    fn test_published_board_grid_string_round_trips_through_rev_gol_io() {
        let target = Grid::from_cells(
            vec![
                vec![false, true, false],
                vec![true, false, true],
                vec![false, true, false],
            ],
            BoundaryCondition::Dead,
        )
        .unwrap();
        let board = PublishedBoard {
            pieces: Vec::new(),
            target,
        };

        let text = board.to_grid_string();
        let reparsed = parse_grid_from_string(&text, BoundaryCondition::Dead).unwrap();

        assert_eq!(reparsed.width, board.target.width);
        assert_eq!(reparsed.height, board.target.height);
        assert_eq!(reparsed.living_count(), board.target.living_count());
        assert_eq!(grid_to_string(&reparsed), text);
    }

    #[test]
    #[ignore = "published-board stamping is still experimental for mixed turn/connector layouts"]
    fn test_build_published_board_for_small_formula_when_sources_exist() {
        if !published_root().exists() {
            return;
        }

        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let board = build_published_board(&construction).unwrap();

        assert!(!board.pieces.is_empty());
        assert!(board.target.width > 0);
        assert!(board.target.height > 0);
        assert!(board.target.living_count() > 0);
    }
}
