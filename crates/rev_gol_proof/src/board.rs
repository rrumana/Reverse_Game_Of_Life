//! Routed board emission for macro-level `SAT -> Rev_GOL` constructions.
//!
//! The current implementation can turn a macro construction into a routed
//! published-pattern board candidate, but the placement solver is still
//! incomplete for some mixed turn/connector layouts.

use crate::compiler::{CompiledConstruction, Endpoint, InstanceId, MacroInstance, MacroKind};
use crate::published::{published_part1_specs, published_root, PublishedPattern, WireAnchors};
use crate::verifier::CellCoord;
use anyhow::{Context, Result};
use rev_gol::config::BoundaryCondition;
use rev_gol::game_of_life::{
    io::{grid_to_string, save_grid_to_file},
    Grid,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::Instant;

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

fn board_timing_enabled() -> bool {
    std::env::var_os("REV_GOL_PROOF_TIMING").is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardBuildOptions {
    pub allow_exact_placement_search: bool,
    pub exact_search_state_limit: Option<usize>,
    pub exact_search_progress_interval: usize,
}

impl Default for BoardBuildOptions {
    fn default() -> Self {
        Self {
            allow_exact_placement_search: false,
            exact_search_state_limit: Some(100_000),
            exact_search_progress_interval: 10_000,
        }
    }
}

pub fn build_published_board(construction: &CompiledConstruction) -> Result<PublishedBoard> {
    build_published_board_with_options(construction, BoardBuildOptions::default())
}

pub fn build_published_board_with_options(
    construction: &CompiledConstruction,
    options: BoardBuildOptions,
) -> Result<PublishedBoard> {
    let root = published_root();
    let library = PublishedLibrary::load(&root)?;
    FabricAssembler::new(construction, &library, options).build()
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
        self.horizontal
            .iter()
            .filter(|item| !item.calibrated)
            .collect()
    }

    pub fn unresolved_vertical(&self) -> Vec<&VerticalMotifFamily> {
        self.vertical
            .iter()
            .filter(|item| !item.calibrated)
            .collect()
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
    let mut assembler = FabricAssembler::new(construction, &library, BoardBuildOptions::default());
    let layout = assembler.prepare_layout()?;
    audit_layout_motifs(&layout, &library)
}

struct PublishedLibrary {
    specs: HashMap<String, PatternInfo>,
    connectors: HashMap<(i8, i8), String>,
}

struct FabricAssembler<'a> {
    construction: &'a CompiledConstruction,
    library: &'a PublishedLibrary,
    build_options: BoardBuildOptions,
    positions: HashMap<InstanceId, (i32, i32)>,
    external_rows: HashMap<String, i32>,
    channel_margin: i32,
    route_cells: HashMap<(i32, i32), BTreeSet<Dir>>,
    instance_links: Vec<((i32, i32), (i32, i32))>,
}

impl<'a> FabricAssembler<'a> {
    fn new(
        construction: &'a CompiledConstruction,
        library: &'a PublishedLibrary,
        build_options: BoardBuildOptions,
    ) -> Self {
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

        Self {
            construction,
            library,
            build_options,
            positions,
            external_rows,
            channel_margin,
            route_cells: HashMap::new(),
            instance_links: Vec::new(),
        }
    }

    fn build(mut self) -> Result<PublishedBoard> {
        let timing = board_timing_enabled();
        let started = Instant::now();
        let layout = self.prepare_layout()?;
        if timing {
            eprintln!(
                "[rev_gol_proof] board build: layout prepared in {:?} (pieces={}, h_links={}, v_links={})",
                started.elapsed(),
                layout.pieces.len(),
                layout.horizontal_relations.len(),
                layout.vertical_relations.len()
            );
        }
        let mut solved = PlacementSolver::new(self.library, self.build_options);
        for (&cell, spec_name) in &layout.pieces {
            solved.add_piece(cell, spec_name)?;
        }
        for &(left, right) in &layout.horizontal_relations {
            solved.add_horizontal_adjacency(left, right)?;
        }
        for &(top, bottom) in &layout.vertical_relations {
            solved.add_vertical_adjacency(top, bottom)?;
        }
        let materialize_started = Instant::now();
        let board = solved.materialize()?;
        if timing {
            eprintln!(
                "[rev_gol_proof] board build: materialized in {:?} (total {:?})",
                materialize_started.elapsed(),
                started.elapsed()
            );
        }
        Ok(board)
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
                self.instance_links
                    .push((stub.coord, self.positions[&port.instance]));
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
            specs.insert(spec.name.to_string(), PatternInfo { pattern, anchors });
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
            .with_context(|| {
                format!("Missing connector for alignment {west_align} -> {east_align}")
            })
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
            cells: HashMap::new(),
            dirs: HashMap::new(),
            route_owner: HashMap::new(),
            instance_links: Vec::new(),
        }
    }

    fn build(mut self) -> Result<PublishedBoard> {
        let layout = self.prepare_layout()?;

        let mut solved = PlacementSolver::new(self.library, BoardBuildOptions::default());
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
        audit_layout_motifs(&layout, self.library)
    }
}

fn audit_layout_motifs(
    layout: &AbstractBoardLayout,
    library: &PublishedLibrary,
) -> Result<BoardMotifAuditReport> {
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
        let left_info = library.pattern(left_spec)?;
        let right_info = library.pattern(right_spec)?;
        let left_align = left_info
            .pattern
            .phase_alignment()
            .0
            .context("Missing east phase")?;
        let right_align = right_info
            .pattern
            .phase_alignment()
            .2
            .context("Missing west phase")?;
        let connector_name = library.connector_name(left_align, right_align)?.to_string();
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
            .map(
                |((left_spec, connector_name, right_spec), count)| HorizontalMotifFamily {
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
                },
            )
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

#[allow(dead_code)]
impl<'a> BoardRouter<'a> {
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
                if !self
                    .dirs
                    .get(&neighbor)
                    .is_some_and(|d| d.contains(&dir.opposite()))
                {
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
        let (source, source_is_external) = match &net.from {
            Endpoint::ExternalInput { variable } => (
                Stub {
                    coord: (0, self.external_rows[variable]),
                    back_dir: Dir::E,
                },
                true,
            ),
            Endpoint::InstancePort(port) => {
                let stub = self.instance_endpoint_stub(&net.from)?;
                self.instance_links
                    .push((stub.coord, self.positions[&port.instance]));
                (stub, false)
            }
        };
        let path = self.find_path(source, source_is_external, target, net_index)?;

        self.dirs
            .entry(source.coord)
            .or_default()
            .insert(source.back_dir);
        self.dirs
            .entry(target.coord)
            .or_default()
            .insert(target.back_dir);
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

    fn find_path(
        &self,
        source: Stub,
        source_is_external: bool,
        target: Stub,
        net_index: usize,
    ) -> Result<Vec<(i32, i32)>> {
        let min_y = self.positions.values().map(|(_, y)| *y).min().unwrap_or(0);
        let min_x = self.positions.values().map(|(x, _)| *x).min().unwrap_or(0);
        let max_x = self.positions.values().map(|(x, _)| *x).max().unwrap_or(0);
        let slot = net_index as i32 * 6;
        let top_lane_y = min_y - self.channel_margin - slot;
        let source_out = source.back_dir.opposite();
        let target_out = target.back_dir.opposite();
        let source_escape = if source_is_external {
            self.external_source_escape_point(source, slot)?
        } else {
            self.escape_point(source)
        };
        let target_escape = self.escape_point(target);
        let source_local_y =
            self.local_row(source_escape, source_out, net_index, EndpointRole::Source);
        let target_local_y =
            self.local_row(target_escape, target_out, net_index, EndpointRole::Target);
        let source_bus_x =
            self.endpoint_bus_x(min_x, max_x, source_out, slot, EndpointRole::Source);
        let target_bus_x =
            self.endpoint_bus_x(min_x, max_x, target_out, slot, EndpointRole::Target);

        let mut path = vec![source.coord];
        let mut waypoints = vec![
            source_escape,
            (source_escape.0, source_local_y),
            (source_bus_x, source_local_y),
            (source_bus_x, top_lane_y),
            (target_bus_x, top_lane_y),
            (target_bus_x, target_local_y),
            (target_escape.0, target_local_y),
            target_escape,
            target.coord,
        ];
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

    fn external_source_escape_point(&self, source: Stub, slot: i32) -> Result<(i32, i32)> {
        if source.back_dir != Dir::E {
            anyhow::bail!(
                "Expected external source stub to face east into the fabric, got {:?}",
                source.back_dir
            );
        }

        let center = step(source.coord, source.back_dir);
        Ok((center.0 - 3 - slot, center.1))
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
            EndpointRole::Target => 3,
        };
        match outward {
            Dir::W => min_x - self.channel_margin - slot - role_offset,
            Dir::E | Dir::N | Dir::S => max_x + self.channel_margin + slot + role_offset,
        }
    }

    fn local_row(
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
        ("NE turn tile", "connector 1 to 0", "horizontal wire tile") => &[
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

pub(crate) fn curated_horizontal_candidate_deltas(
    left_spec: &str,
    connector_name: &str,
    right_spec: &str,
) -> Vec<(i32, i32)> {
    known_horizontal_placement_candidates(left_spec, connector_name, right_spec)
        .iter()
        .map(|candidate| candidate.right_delta)
        .collect()
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
        ("OR gate tile", "vertical wire tile") => &[
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
        ("splitter tile", "vertical wire tile") => &[
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
        ("vertical wire tile", "NE turn tile") => &[
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
        ("vertical wire tile", "OR gate tile") => &[
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
        ("vertical wire tile", "splitter tile") => &[
            VerticalPlacementCandidate {
                bottom_delta: (0, 79),
            },
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
        _ => &[],
    }
}

pub(crate) fn curated_vertical_candidate_deltas(
    top_spec: &str,
    bottom_spec: &str,
) -> Vec<(i32, i32)> {
    known_vertical_placement_candidates(top_spec, bottom_spec)
        .iter()
        .map(|candidate| candidate.bottom_delta)
        .collect()
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
        right_delta: (
            right_origin.0 - left_origin.0,
            right_origin.1 - left_origin.1,
        ),
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

fn manhattan_delta_distance(lhs: (i32, i32), rhs: (i32, i32)) -> i32 {
    (lhs.0 - rhs.0).abs() + (lhs.1 - rhs.1).abs()
}

fn synthesized_horizontal_fallback_candidates(
    default_candidate: HorizontalPlacementCandidate,
) -> Vec<HorizontalPlacementCandidate> {
    [-6, -3, 0, 3, 6]
        .into_iter()
        .map(|dx| HorizontalPlacementCandidate {
            connector_delta: default_candidate.connector_delta,
            right_delta: (
                default_candidate.right_delta.0 + dx,
                default_candidate.right_delta.1,
            ),
        })
        .collect()
}

fn synthesized_vertical_fallback_candidates(
    default_candidate: VerticalPlacementCandidate,
) -> Vec<VerticalPlacementCandidate> {
    [-6, -3, 0, 3, 6]
        .into_iter()
        .map(|dy| VerticalPlacementCandidate {
            bottom_delta: (
                default_candidate.bottom_delta.0,
                default_candidate.bottom_delta.1 + dy,
            ),
        })
        .collect()
}

fn push_unique_horizontal_candidate(
    candidates: &mut Vec<HorizontalPlacementCandidate>,
    candidate: HorizontalPlacementCandidate,
) {
    if !candidates
        .iter()
        .any(|existing| existing.right_delta == candidate.right_delta)
    {
        candidates.push(candidate);
    }
}

fn push_unique_vertical_candidate(
    candidates: &mut Vec<VerticalPlacementCandidate>,
    candidate: VerticalPlacementCandidate,
) {
    if !candidates
        .iter()
        .any(|existing| existing.bottom_delta == candidate.bottom_delta)
    {
        candidates.push(candidate);
    }
}

struct PlacementSolver<'a> {
    library: &'a PublishedLibrary,
    options: BoardBuildOptions,
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
    phase_x: i32,
    start_x: i32,
    end_x: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct VerticalFamilyKey {
    top_spec: String,
    bottom_spec: String,
    lane_x: i32,
    phase_y: i32,
    start_y: i32,
    end_y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum VerticalOrHorizontalFamily {
    Horizontal(HorizontalFamilyKey),
    Vertical(VerticalFamilyKey),
}

#[derive(Debug, Clone)]
struct PreparedHorizontalLink {
    left: (i32, i32),
    right: (i32, i32),
    family: HorizontalFamilyKey,
    segments: Vec<((i32, i32), (i32, i32))>,
}

#[derive(Debug, Clone)]
struct PreparedVerticalLink {
    top: (i32, i32),
    bottom: (i32, i32),
    family: VerticalFamilyKey,
    segments: Vec<((i32, i32), (i32, i32))>,
}

#[derive(Debug, Clone)]
struct PlacementModel {
    horizontal_domains: BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
    vertical_domains: BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
    horizontal_links: Vec<PreparedHorizontalLink>,
    vertical_links: Vec<PreparedVerticalLink>,
    plaquettes: Vec<PlaquetteConstraint>,
    cycle_constraints: Vec<CycleConstraint>,
}

#[derive(Debug, Clone)]
struct PlaquetteConstraint {
    northwest: (i32, i32),
    top: HorizontalFamilyKey,
    bottom: HorizontalFamilyKey,
    left: VerticalFamilyKey,
    right: VerticalFamilyKey,
}

#[derive(Debug, Clone)]
struct ReducedPlacementDomains {
    horizontal: BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
    vertical: BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
}

#[derive(Debug, Clone)]
struct CycleConstraint {
    closing_edge: ((i32, i32), (i32, i32)),
    terms: Vec<(VerticalOrHorizontalFamily, i32)>,
}

#[derive(Debug, Clone)]
struct PlacementGraphEdge {
    id: usize,
    from: (i32, i32),
    to: (i32, i32),
    family: VerticalOrHorizontalFamily,
}

#[derive(Debug, Clone)]
struct TreeParentStep {
    parent: (i32, i32),
    family: VerticalOrHorizontalFamily,
    sign: i32,
}

#[derive(Debug, Clone)]
struct OriginEdge {
    neighbor: (i32, i32),
    delta: (i32, i32),
    family: VerticalOrHorizontalFamily,
    sign: i32,
}

#[derive(Debug, Clone)]
struct OriginDerivation {
    parent: (i32, i32),
    family: VerticalOrHorizontalFamily,
    sign: i32,
}

#[derive(Debug, Clone)]
struct PlacementConflict {
    cell: (i32, i32),
    cell_spec: String,
    neighbor: (i32, i32),
    neighbor_spec: String,
    expected: (i32, i32),
    found: (i32, i32),
    cycle_terms: Vec<(VerticalOrHorizontalFamily, i32)>,
    implicated_families: Vec<VerticalOrHorizontalFamily>,
}

impl PlacementConflict {
    fn render(&self) -> String {
        format!(
            "Deterministic placement conflict for {:?} [{}] and {:?} [{}]: expected {:?}, found {:?}; cycle terms={:?}; implicated families={:?}",
            self.cell,
            self.cell_spec,
            self.neighbor,
            self.neighbor_spec,
            self.expected,
            self.found,
            self.cycle_terms,
            self.implicated_families
        )
    }
}

impl<'a> PlacementSolver<'a> {
    fn new(library: &'a PublishedLibrary, options: BoardBuildOptions) -> Self {
        Self {
            library,
            options,
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
        let timing = board_timing_enabled();
        let solve_started = Instant::now();
        let solved_origins = self.solve_origins()?;
        if timing {
            eprintln!(
                "[rev_gol_proof] board build: solved origins in {:?}",
                solve_started.elapsed()
            );
        }
        for (cell, origin) in solved_origins {
            self.pieces.get_mut(&cell).unwrap().origin = Some(origin);
        }

        let stamp_started = Instant::now();
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

        let board = PublishedBoard {
            pieces: board_pieces,
            target,
        };
        if timing {
            eprintln!(
                "[rev_gol_proof] board build: stamped board in {:?}",
                stamp_started.elapsed()
            );
        }
        Ok(board)
    }

    fn solve_origins(&self) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let timing = board_timing_enabled();
        let model = self.build_placement_model()?;
        if timing {
            let max_cycle_terms = model
                .cycle_constraints
                .iter()
                .map(|constraint| constraint.terms.len())
                .max()
                .unwrap_or(0);
            eprintln!(
                "[rev_gol_proof] board build: placement model has {} horizontal families, {} vertical families, {} local plaquettes, {} cycle constraints (max {} terms)",
                model.horizontal_domains.len(),
                model.vertical_domains.len(),
                model.plaquettes.len(),
                model.cycle_constraints.len(),
                max_cycle_terms
            );
        }
        let canonical_choices = self.canonical_family_choices(&model)?;
        let canonical_started = Instant::now();
        match self.try_solve_origins_with_choices(
            &model,
            &canonical_choices.0,
            &canonical_choices.1,
        ) {
            Ok(origins) => {
                if timing {
                    eprintln!(
                        "[rev_gol_proof] board build: canonical family choices solved origins in {:?}",
                        canonical_started.elapsed()
                    );
                }
                return Ok(origins);
            }
            Err(err) => {
                if timing {
                    eprintln!(
                        "[rev_gol_proof] board build: canonical family choices failed in {:?}: {err}",
                        canonical_started.elapsed(),
                        err = err.render()
                    );
                }
            }
        }

        let repair_started = Instant::now();
        let mut repair_horizontal_choices = canonical_choices.0.clone();
        let mut repair_vertical_choices = canonical_choices.1.clone();
        let mut repair_seen = HashSet::new();
        if let Some(origins) = self.search_conflict_guided_repairs(
            &model,
            &mut repair_horizontal_choices,
            &mut repair_vertical_choices,
            &mut repair_seen,
        )? {
            if timing {
                eprintln!(
                    "[rev_gol_proof] board build: conflict-guided repair solved origins in {:?} after {} states",
                    repair_started.elapsed(),
                    repair_seen.len()
                );
            }
            return Ok(origins);
        }
        if timing {
            eprintln!(
                "[rev_gol_proof] board build: conflict-guided repair exhausted {} states in {:?}",
                repair_seen.len(),
                repair_started.elapsed()
            );
        }

        let family_started = Instant::now();
        match self.solve_family_choices_with_model(&model) {
            Ok(origins) => {
                if timing {
                    eprintln!(
                        "[rev_gol_proof] board build: searched family choices solved origins in {:?}",
                        family_started.elapsed()
                    );
                }
                Ok(origins)
            }
            Err(family_err) if self.options.allow_exact_placement_search => {
                if timing {
                    eprintln!(
                        "[rev_gol_proof] board build: family-choice search failed in {:?}: {family_err}",
                        family_started.elapsed()
                    );
                }
                self.solve_origins_exact().with_context(|| {
                    format!("Family-based placement failed before exact fallback: {family_err}")
                })
            }
            Err(family_err) => {
                if timing {
                    eprintln!(
                        "[rev_gol_proof] board build: family-choice search failed in {:?}: {family_err}",
                        family_started.elapsed()
                    );
                }
                Err(family_err).context(
                    "Family-based placement failed and exact placement fallback is disabled",
                )
            }
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

    #[allow(dead_code)]
    fn solve_family_choices(&self) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let model = self.build_placement_model()?;
        self.solve_family_choices_with_model(&model)
    }

    fn solve_family_choices_with_model(
        &self,
        model: &PlacementModel,
    ) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let mut horizontal_choices = BTreeMap::new();
        let mut vertical_choices = BTreeMap::new();
        let mut dead_states = HashSet::new();
        self.search_family_choices(
            &model,
            &mut horizontal_choices,
            &mut vertical_choices,
            &mut dead_states,
        )?
        .context("No globally coherent placement-family selection found")
    }

    fn canonical_family_choices(
        &self,
        model: &PlacementModel,
    ) -> Result<(
        BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        BTreeMap<VerticalFamilyKey, (i32, i32)>,
    )> {
        let horizontal_choices = model
            .horizontal_domains
            .iter()
            .map(|(family, candidates)| {
                let candidate = candidates.first().copied().with_context(|| {
                    format!(
                        "Horizontal family {:?} unexpectedly has no candidates",
                        family
                    )
                })?;
                Ok((family.clone(), candidate))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let vertical_choices = model
            .vertical_domains
            .iter()
            .map(|(family, candidates)| {
                let candidate = candidates.first().copied().with_context(|| {
                    format!(
                        "Vertical family {:?} unexpectedly has no candidates",
                        family
                    )
                })?;
                Ok((family.clone(), candidate))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok((horizontal_choices, vertical_choices))
    }

    fn search_conflict_guided_repairs(
        &self,
        model: &PlacementModel,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
        seen_states: &mut HashSet<String>,
    ) -> Result<Option<HashMap<(i32, i32), (i32, i32)>>> {
        // Corridor aggregation makes conflict-guided repair local enough to be useful again.
        const MAX_CONFLICT_GUIDED_REPAIR_STATES: usize = 2048;

        if seen_states.len() >= MAX_CONFLICT_GUIDED_REPAIR_STATES {
            return Ok(None);
        }
        let state_key = self.family_choice_state_key(horizontal_choices, vertical_choices);
        if !seen_states.insert(state_key) {
            return Ok(None);
        }
        if board_timing_enabled() && seen_states.len().is_power_of_two() {
            eprintln!(
                "[rev_gol_proof] board build: conflict-guided repair visited {} states",
                seen_states.len()
            );
        }

        let conflict = match self.try_solve_origins_with_choices(
            model,
            horizontal_choices,
            vertical_choices,
        ) {
            Ok(origins) => return Ok(Some(origins)),
            Err(conflict) => conflict,
        };

        let branch_families = self.conflict_family_branch_order(
            model,
            horizontal_choices,
            vertical_choices,
            &conflict,
        );
        let repair_assignments =
            self.conflict_repair_states(model, horizontal_choices, vertical_choices, &conflict)?;
        for assignment in repair_assignments {
            let mut previous = Vec::with_capacity(assignment.len());
            for (family, candidate) in &assignment {
                previous.push((
                    family.clone(),
                    self.set_family_choice(
                        horizontal_choices,
                        vertical_choices,
                        family,
                        *candidate,
                    )?,
                ));
            }
            if let Some(origins) = self.search_conflict_guided_repairs(
                model,
                horizontal_choices,
                vertical_choices,
                seen_states,
            )? {
                return Ok(Some(origins));
            }
            for (family, candidate) in previous {
                self.set_family_choice(horizontal_choices, vertical_choices, &family, candidate)?;
            }
        }
        if branch_families.is_empty() {
            return Ok(None);
        }

        for family in branch_families {
            let alternatives =
                self.family_alternatives(model, horizontal_choices, vertical_choices, &family)?;
            for candidate in alternatives {
                let previous = self.set_family_choice(
                    horizontal_choices,
                    vertical_choices,
                    &family,
                    candidate,
                )?;
                if let Some(origins) = self.search_conflict_guided_repairs(
                    model,
                    horizontal_choices,
                    vertical_choices,
                    seen_states,
                )? {
                    return Ok(Some(origins));
                }
                self.set_family_choice(horizontal_choices, vertical_choices, &family, previous)?;
            }
        }

        Ok(None)
    }

    fn search_family_choices(
        &self,
        model: &PlacementModel,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
        dead_states: &mut HashSet<String>,
    ) -> Result<Option<HashMap<(i32, i32), (i32, i32)>>> {
        if self
            .propagate_partial_family_choices(model, horizontal_choices, vertical_choices)
            .is_err()
        {
            return Ok(None);
        }

        if !self.propagate_forced_family_choices(model, horizontal_choices, vertical_choices)? {
            return Ok(None);
        }

        let reduced = match self.reduce_family_domains(model, horizontal_choices, vertical_choices)
        {
            Ok(reduced) => reduced,
            Err(_) => return Ok(None),
        };

        let state_key = self.family_choice_state_key(horizontal_choices, vertical_choices);
        if dead_states.contains(&state_key) {
            return Ok(None);
        }

        if horizontal_choices.len() == model.horizontal_domains.len()
            && vertical_choices.len() == model.vertical_domains.len()
        {
            return Ok(self
                .solve_origins_with_choices(model, horizontal_choices, vertical_choices)
                .ok());
        }

        let next_horizontal = reduced
            .horizontal
            .iter()
            .filter(|(family, _)| !horizontal_choices.contains_key(*family))
            .filter(|(_, viable)| !viable.is_empty())
            .map(|(family, viable)| {
                (
                    true,
                    VerticalOrHorizontalFamily::Horizontal(family.clone()),
                    viable.clone(),
                )
            });
        let next_vertical = reduced
            .vertical
            .iter()
            .filter(|(family, _)| !vertical_choices.contains_key(*family))
            .filter(|(_, viable)| !viable.is_empty())
            .map(|(family, viable)| {
                (
                    false,
                    VerticalOrHorizontalFamily::Vertical(family.clone()),
                    viable.clone(),
                )
            });

        let next_family = next_horizontal
            .chain(next_vertical)
            .min_by(|a, b| a.2.len().cmp(&b.2.len()).then_with(|| a.1.cmp(&b.1)));

        let Some((is_horizontal, family, viable_candidates)) = next_family else {
            return Ok(Some(self.solve_origins_with_choices(
                model,
                horizontal_choices,
                vertical_choices,
            )?));
        };

        match (is_horizontal, family) {
            (true, VerticalOrHorizontalFamily::Horizontal(family)) => {
                for candidate in viable_candidates {
                    horizontal_choices.insert(family.clone(), candidate);
                    if let Some(solution) = self.search_family_choices(
                        model,
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
                        model,
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
            key.push_str(&family.phase_x.to_string());
            key.push('|');
            key.push_str(&family.start_x.to_string());
            key.push('|');
            key.push_str(&family.end_x.to_string());
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
            key.push_str(&family.phase_y.to_string());
            key.push('|');
            key.push_str(&family.start_y.to_string());
            key.push('|');
            key.push_str(&family.end_y.to_string());
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
        model: &PlacementModel,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<bool> {
        loop {
            let reduced =
                match self.reduce_family_domains(model, horizontal_choices, vertical_choices) {
                    Ok(reduced) => reduced,
                    Err(_) => return Ok(false),
                };
            let mut progress = false;

            for (family, viable) in &reduced.horizontal {
                if viable.is_empty() {
                    return Ok(false);
                }
                if viable.len() == 1 {
                    match horizontal_choices.get(family).copied() {
                        Some(existing) if existing == viable[0] => {}
                        Some(_) => return Ok(false),
                        None => {
                            horizontal_choices.insert(family.clone(), viable[0]);
                            progress = true;
                        }
                    }
                }
            }

            for (family, viable) in &reduced.vertical {
                if viable.is_empty() {
                    return Ok(false);
                }
                if viable.len() == 1 {
                    match vertical_choices.get(family).copied() {
                        Some(existing) if existing == viable[0] => {}
                        Some(_) => return Ok(false),
                        None => {
                            vertical_choices.insert(family.clone(), viable[0]);
                            progress = true;
                        }
                    }
                }
            }

            if !progress {
                return Ok(true);
            }

            if self
                .propagate_partial_family_choices(model, horizontal_choices, vertical_choices)
                .is_err()
            {
                return Ok(false);
            }
        }
    }

    fn propagate_partial_family_choices(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<()> {
        let reduced = self.reduce_family_domains(model, horizontal_choices, vertical_choices)?;
        let root = self.placement_root_cell(model)?;
        let adjacency = self.build_selected_adjacency(model, horizontal_choices, vertical_choices);
        let origins = self
            .propagate_origins_from_adjacency(root, &adjacency)
            .map_err(|conflict| anyhow::anyhow!(conflict.render()))?;

        for link in &model.horizontal_links {
            let left = link.left;
            let right = link.right;
            let candidates = reduced
                .horizontal
                .get(&link.family)
                .context("Missing horizontal family domain")?;
            if let (Some(left_origin), Some(right_origin)) =
                (origins.get(&left).copied(), origins.get(&right).copied())
            {
                let valid = match horizontal_choices.get(&link.family).copied() {
                    Some(delta) => {
                        (left_origin.0 + delta.0, left_origin.1 + delta.1) == right_origin
                    }
                    None => candidates.iter().any(|delta| {
                        (left_origin.0 + delta.0, left_origin.1 + delta.1) == right_origin
                    }),
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

        for link in &model.vertical_links {
            let top = link.top;
            let bottom = link.bottom;
            let candidates = reduced
                .vertical
                .get(&link.family)
                .context("Missing vertical family domain")?;
            if let (Some(top_origin), Some(bottom_origin)) =
                (origins.get(&top).copied(), origins.get(&bottom).copied())
            {
                let valid = match vertical_choices.get(&link.family).copied() {
                    Some(delta) => {
                        (top_origin.0 + delta.0, top_origin.1 + delta.1) == bottom_origin
                    }
                    None => candidates.iter().any(|delta| {
                        (top_origin.0 + delta.0, top_origin.1 + delta.1) == bottom_origin
                    }),
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

    fn reduce_family_domains(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<ReducedPlacementDomains> {
        let mut horizontal = model.horizontal_domains.clone();
        let mut vertical = model.vertical_domains.clone();

        for (family, &choice) in horizontal_choices {
            let domain = horizontal
                .get_mut(family)
                .with_context(|| format!("Missing horizontal domain for family {:?}", family))?;
            if !domain.contains(&choice) {
                anyhow::bail!(
                    "Horizontal family {:?} was assigned unsupported candidate {:?}",
                    family,
                    choice
                );
            }
            domain.clear();
            domain.push(choice);
        }

        for (family, &choice) in vertical_choices {
            let domain = vertical
                .get_mut(family)
                .with_context(|| format!("Missing vertical domain for family {:?}", family))?;
            if !domain.contains(&choice) {
                anyhow::bail!(
                    "Vertical family {:?} was assigned unsupported candidate {:?}",
                    family,
                    choice
                );
            }
            domain.clear();
            domain.push(choice);
        }

        let mut changed = true;
        while changed {
            changed = false;
            for plaquette in &model.plaquettes {
                let top_domain = horizontal
                    .get(&plaquette.top)
                    .with_context(|| format!("Missing top domain for plaquette {:?}", plaquette))?
                    .clone();
                let bottom_domain = horizontal
                    .get(&plaquette.bottom)
                    .with_context(|| {
                        format!("Missing bottom domain for plaquette {:?}", plaquette)
                    })?
                    .clone();
                let left_domain = vertical
                    .get(&plaquette.left)
                    .with_context(|| format!("Missing left domain for plaquette {:?}", plaquette))?
                    .clone();
                let right_domain = vertical
                    .get(&plaquette.right)
                    .with_context(|| format!("Missing right domain for plaquette {:?}", plaquette))?
                    .clone();

                let mut supported_top = BTreeSet::new();
                let mut supported_bottom = BTreeSet::new();
                let mut supported_left = BTreeSet::new();
                let mut supported_right = BTreeSet::new();

                for &top_delta in &top_domain {
                    for &right_delta in &right_domain {
                        let top_right = (top_delta.0 + right_delta.0, top_delta.1 + right_delta.1);
                        for &left_delta in &left_domain {
                            for &bottom_delta in &bottom_domain {
                                if top_right
                                    == (
                                        left_delta.0 + bottom_delta.0,
                                        left_delta.1 + bottom_delta.1,
                                    )
                                {
                                    supported_top.insert(top_delta);
                                    supported_bottom.insert(bottom_delta);
                                    supported_left.insert(left_delta);
                                    supported_right.insert(right_delta);
                                }
                            }
                        }
                    }
                }

                if supported_top.is_empty()
                    || supported_bottom.is_empty()
                    || supported_left.is_empty()
                    || supported_right.is_empty()
                {
                    anyhow::bail!(
                        "Local plaquette {:?} around {:?} has no supported family-delta combination",
                        plaquette,
                        plaquette.northwest
                    );
                }

                changed |= Self::retain_supported_candidates(
                    horizontal
                        .get_mut(&plaquette.top)
                        .context("Missing top domain after support computation")?,
                    &supported_top,
                );
                changed |= Self::retain_supported_candidates(
                    horizontal
                        .get_mut(&plaquette.bottom)
                        .context("Missing bottom domain after support computation")?,
                    &supported_bottom,
                );
                changed |= Self::retain_supported_candidates(
                    vertical
                        .get_mut(&plaquette.left)
                        .context("Missing left domain after support computation")?,
                    &supported_left,
                );
                changed |= Self::retain_supported_candidates(
                    vertical
                        .get_mut(&plaquette.right)
                        .context("Missing right domain after support computation")?,
                    &supported_right,
                );
            }
            for constraint in &model.cycle_constraints {
                changed |=
                    self.apply_cycle_constraint(&mut horizontal, &mut vertical, constraint)?;
            }
        }

        Ok(ReducedPlacementDomains {
            horizontal,
            vertical,
        })
    }

    fn retain_supported_candidates(
        domain: &mut Vec<(i32, i32)>,
        supported: &BTreeSet<(i32, i32)>,
    ) -> bool {
        let original_len = domain.len();
        domain.retain(|candidate| supported.contains(candidate));
        domain.len() != original_len
    }

    fn apply_cycle_constraint(
        &self,
        horizontal: &mut BTreeMap<HorizontalFamilyKey, Vec<(i32, i32)>>,
        vertical: &mut BTreeMap<VerticalFamilyKey, Vec<(i32, i32)>>,
        constraint: &CycleConstraint,
    ) -> Result<bool> {
        let mut domains = Vec::with_capacity(constraint.terms.len());
        for (family, _) in &constraint.terms {
            let domain = match family {
                VerticalOrHorizontalFamily::Horizontal(family) => horizontal
                    .get(family)
                    .with_context(|| format!("Missing horizontal domain for family {:?}", family))?
                    .clone(),
                VerticalOrHorizontalFamily::Vertical(family) => vertical
                    .get(family)
                    .with_context(|| format!("Missing vertical domain for family {:?}", family))?
                    .clone(),
            };
            domains.push(domain);
        }

        let mut prefix = vec![HashSet::<(i32, i32)>::new(); constraint.terms.len() + 1];
        prefix[0].insert((0, 0));
        for (idx, (_, coefficient)) in constraint.terms.iter().enumerate() {
            let current_prefix = prefix[idx].iter().copied().collect::<Vec<_>>();
            for sum in current_prefix {
                for &candidate in &domains[idx] {
                    prefix[idx + 1].insert((
                        sum.0 + coefficient * candidate.0,
                        sum.1 + coefficient * candidate.1,
                    ));
                }
            }
        }

        let mut suffix = vec![HashSet::<(i32, i32)>::new(); constraint.terms.len() + 1];
        suffix[constraint.terms.len()].insert((0, 0));
        for idx in (0..constraint.terms.len()).rev() {
            let coefficient = constraint.terms[idx].1;
            let current_suffix = suffix[idx + 1].iter().copied().collect::<Vec<_>>();
            for sum in current_suffix {
                for &candidate in &domains[idx] {
                    suffix[idx].insert((
                        sum.0 + coefficient * candidate.0,
                        sum.1 + coefficient * candidate.1,
                    ));
                }
            }
        }

        let mut supported = vec![BTreeSet::new(); constraint.terms.len()];
        for (idx, (_, coefficient)) in constraint.terms.iter().enumerate() {
            for &candidate in &domains[idx] {
                let supported_here = prefix[idx].iter().any(|sum| {
                    suffix[idx + 1].contains(&(
                        -sum.0 - coefficient * candidate.0,
                        -sum.1 - coefficient * candidate.1,
                    ))
                });
                if supported_here {
                    supported[idx].insert(candidate);
                }
            }
        }

        if supported.iter().any(BTreeSet::is_empty) {
            anyhow::bail!(
                "Cycle constraint {:?} on closing edge {:?} has no supported family-delta combination",
                constraint.terms,
                constraint.closing_edge
            );
        }

        let mut changed = false;
        for ((family, _), supported_candidates) in constraint.terms.iter().zip(&supported) {
            match family {
                VerticalOrHorizontalFamily::Horizontal(family) => {
                    changed |= Self::retain_supported_candidates(
                        horizontal
                            .get_mut(family)
                            .context("Missing horizontal domain after cycle support computation")?,
                        supported_candidates,
                    );
                }
                VerticalOrHorizontalFamily::Vertical(family) => {
                    changed |= Self::retain_supported_candidates(
                        vertical
                            .get_mut(family)
                            .context("Missing vertical domain after cycle support computation")?,
                        supported_candidates,
                    );
                }
            }
        }

        Ok(changed)
    }

    fn solve_origins_with_choices(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        self.try_solve_origins_with_choices(model, horizontal_choices, vertical_choices)
            .map_err(|conflict| anyhow::anyhow!(conflict.render()))
    }

    fn try_solve_origins_with_choices(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> std::result::Result<HashMap<(i32, i32), (i32, i32)>, PlacementConflict> {
        let model_nodes = self.placement_model_nodes(model);
        let root = *model_nodes
            .first()
            .expect("No pieces available for placement solving");
        let adjacency = self.build_complete_adjacency(model, horizontal_choices, vertical_choices);
        let endpoint_origins = self.propagate_origins_from_adjacency(root, &adjacency)?;

        if endpoint_origins.len() != model_nodes.len() {
            return Err(PlacementConflict {
                cell: root,
                cell_spec: self.pieces[&root].spec_name.clone(),
                neighbor: root,
                neighbor_spec: self.pieces[&root].spec_name.clone(),
                expected: (0, 0),
                found: (0, 0),
                cycle_terms: Vec::new(),
                implicated_families: Vec::new(),
            });
        }

        let origins = self
            .reconstruct_corridor_origins(
                model,
                &endpoint_origins,
                horizontal_choices,
                vertical_choices,
            )
            .map_err(|_| PlacementConflict {
                cell: root,
                cell_spec: self.pieces[&root].spec_name.clone(),
                neighbor: root,
                neighbor_spec: self.pieces[&root].spec_name.clone(),
                expected: (0, 0),
                found: (0, 0),
                cycle_terms: Vec::new(),
                implicated_families: Vec::new(),
            })?;

        if origins.len() != self.pieces.len() {
            return Err(PlacementConflict {
                cell: root,
                cell_spec: self.pieces[&root].spec_name.clone(),
                neighbor: root,
                neighbor_spec: self.pieces[&root].spec_name.clone(),
                expected: (0, 0),
                found: (0, 0),
                cycle_terms: Vec::new(),
                implicated_families: Vec::new(),
            });
        }

        Ok(origins)
    }

    fn placement_model_nodes(&self, model: &PlacementModel) -> Vec<(i32, i32)> {
        let mut nodes = BTreeSet::new();
        for link in &model.horizontal_links {
            nodes.insert(link.left);
            nodes.insert(link.right);
        }
        for link in &model.vertical_links {
            nodes.insert(link.top);
            nodes.insert(link.bottom);
        }
        if nodes.is_empty() {
            nodes.extend(self.pieces.keys().copied());
        }
        nodes.into_iter().collect()
    }

    fn placement_root_cell(&self, model: &PlacementModel) -> Result<(i32, i32)> {
        self.placement_model_nodes(model)
            .into_iter()
            .next()
            .context("No pieces available for placement solving")
    }

    fn build_selected_adjacency(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> HashMap<(i32, i32), Vec<OriginEdge>> {
        let mut adjacency = HashMap::<(i32, i32), Vec<OriginEdge>>::new();
        for link in &model.horizontal_links {
            if let Some(delta) = horizontal_choices.get(&link.family).copied() {
                Self::add_origin_edge(
                    &mut adjacency,
                    link.left,
                    link.right,
                    delta,
                    VerticalOrHorizontalFamily::Horizontal(link.family.clone()),
                );
            }
        }
        for link in &model.vertical_links {
            if let Some(delta) = vertical_choices.get(&link.family).copied() {
                Self::add_origin_edge(
                    &mut adjacency,
                    link.top,
                    link.bottom,
                    delta,
                    VerticalOrHorizontalFamily::Vertical(link.family.clone()),
                );
            }
        }
        adjacency
    }

    fn build_complete_adjacency(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> HashMap<(i32, i32), Vec<OriginEdge>> {
        let mut adjacency = HashMap::<(i32, i32), Vec<OriginEdge>>::new();
        for link in &model.horizontal_links {
            Self::add_origin_edge(
                &mut adjacency,
                link.left,
                link.right,
                horizontal_choices[&link.family],
                VerticalOrHorizontalFamily::Horizontal(link.family.clone()),
            );
        }
        for link in &model.vertical_links {
            Self::add_origin_edge(
                &mut adjacency,
                link.top,
                link.bottom,
                vertical_choices[&link.family],
                VerticalOrHorizontalFamily::Vertical(link.family.clone()),
            );
        }
        adjacency
    }

    fn reconstruct_corridor_origins(
        &self,
        model: &PlacementModel,
        endpoint_origins: &HashMap<(i32, i32), (i32, i32)>,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
    ) -> Result<HashMap<(i32, i32), (i32, i32)>> {
        let mut origins = endpoint_origins.clone();

        for link in &model.horizontal_links {
            let total_delta = horizontal_choices
                .get(&link.family)
                .copied()
                .with_context(|| {
                    format!("Missing horizontal choice for family {:?}", link.family)
                })?;
            self.reconstruct_horizontal_corridor(link, total_delta, &mut origins)?;
        }

        for link in &model.vertical_links {
            let total_delta = vertical_choices
                .get(&link.family)
                .copied()
                .with_context(|| format!("Missing vertical choice for family {:?}", link.family))?;
            self.reconstruct_vertical_corridor(link, total_delta, &mut origins)?;
        }

        Ok(origins)
    }

    fn reconstruct_horizontal_corridor(
        &self,
        link: &PreparedHorizontalLink,
        total_delta: (i32, i32),
        origins: &mut HashMap<(i32, i32), (i32, i32)>,
    ) -> Result<()> {
        let left_origin = origins.get(&link.left).copied().with_context(|| {
            format!(
                "Missing origin for horizontal corridor start {:?}",
                link.left
            )
        })?;
        let right_origin = origins.get(&link.right).copied().with_context(|| {
            format!(
                "Missing origin for horizontal corridor end {:?}",
                link.right
            )
        })?;
        let domains = link
            .segments
            .iter()
            .map(|&(left, right)| self.horizontal_deltas(left, right))
            .collect::<Result<Vec<_>>>()?;
        let suffix = Self::corridor_suffix_sums(&domains);
        anyhow::ensure!(
            suffix
                .first()
                .is_some_and(|supported| supported.contains(&total_delta)),
            "Horizontal corridor {:?} -> {:?} does not support aggregate delta {:?}",
            link.left,
            link.right,
            total_delta
        );

        let mut remaining = total_delta;
        let mut current_origin = left_origin;
        for (idx, (&(_, next_cell), domain)) in link.segments.iter().zip(&domains).enumerate() {
            let chosen = domain
                .iter()
                .copied()
                .find(|candidate| {
                    suffix[idx + 1]
                        .contains(&(remaining.0 - candidate.0, remaining.1 - candidate.1))
                })
                .with_context(|| {
                    format!(
                        "Failed to reconstruct horizontal corridor {:?} -> {:?} at segment {}",
                        link.left, link.right, idx
                    )
                })?;
            let next_origin = (current_origin.0 + chosen.0, current_origin.1 + chosen.1);
            if idx + 1 == link.segments.len() {
                anyhow::ensure!(
                    next_origin == right_origin,
                    "Horizontal corridor {:?} -> {:?} reconstructed inconsistent endpoint {:?} vs {:?}",
                    link.left,
                    link.right,
                    next_origin,
                    right_origin
                );
            } else {
                Self::insert_reconstructed_origin(origins, next_cell, next_origin)?;
            }
            current_origin = next_origin;
            remaining = (remaining.0 - chosen.0, remaining.1 - chosen.1);
        }

        anyhow::ensure!(
            remaining == (0, 0),
            "Horizontal corridor {:?} -> {:?} left residual delta {:?}",
            link.left,
            link.right,
            remaining
        );
        Ok(())
    }

    fn reconstruct_vertical_corridor(
        &self,
        link: &PreparedVerticalLink,
        total_delta: (i32, i32),
        origins: &mut HashMap<(i32, i32), (i32, i32)>,
    ) -> Result<()> {
        let top_origin = origins.get(&link.top).copied().with_context(|| {
            format!("Missing origin for vertical corridor start {:?}", link.top)
        })?;
        let bottom_origin = origins.get(&link.bottom).copied().with_context(|| {
            format!("Missing origin for vertical corridor end {:?}", link.bottom)
        })?;
        let domains = link
            .segments
            .iter()
            .map(|&(top, bottom)| self.vertical_deltas(top, bottom))
            .collect::<Result<Vec<_>>>()?;
        let suffix = Self::corridor_suffix_sums(&domains);
        anyhow::ensure!(
            suffix
                .first()
                .is_some_and(|supported| supported.contains(&total_delta)),
            "Vertical corridor {:?} -> {:?} does not support aggregate delta {:?}",
            link.top,
            link.bottom,
            total_delta
        );

        let mut remaining = total_delta;
        let mut current_origin = top_origin;
        for (idx, (&(_, next_cell), domain)) in link.segments.iter().zip(&domains).enumerate() {
            let chosen = domain
                .iter()
                .copied()
                .find(|candidate| {
                    suffix[idx + 1]
                        .contains(&(remaining.0 - candidate.0, remaining.1 - candidate.1))
                })
                .with_context(|| {
                    format!(
                        "Failed to reconstruct vertical corridor {:?} -> {:?} at segment {}",
                        link.top, link.bottom, idx
                    )
                })?;
            let next_origin = (current_origin.0 + chosen.0, current_origin.1 + chosen.1);
            if idx + 1 == link.segments.len() {
                anyhow::ensure!(
                    next_origin == bottom_origin,
                    "Vertical corridor {:?} -> {:?} reconstructed inconsistent endpoint {:?} vs {:?}",
                    link.top,
                    link.bottom,
                    next_origin,
                    bottom_origin
                );
            } else {
                Self::insert_reconstructed_origin(origins, next_cell, next_origin)?;
            }
            current_origin = next_origin;
            remaining = (remaining.0 - chosen.0, remaining.1 - chosen.1);
        }

        anyhow::ensure!(
            remaining == (0, 0),
            "Vertical corridor {:?} -> {:?} left residual delta {:?}",
            link.top,
            link.bottom,
            remaining
        );
        Ok(())
    }

    fn corridor_suffix_sums(domains: &[Vec<(i32, i32)>]) -> Vec<HashSet<(i32, i32)>> {
        let mut suffix = vec![HashSet::<(i32, i32)>::new(); domains.len() + 1];
        suffix[domains.len()].insert((0, 0));
        for idx in (0..domains.len()).rev() {
            let tail = suffix[idx + 1].iter().copied().collect::<Vec<_>>();
            for sum in tail {
                for &candidate in &domains[idx] {
                    suffix[idx].insert((sum.0 + candidate.0, sum.1 + candidate.1));
                }
            }
        }
        suffix
    }

    fn insert_reconstructed_origin(
        origins: &mut HashMap<(i32, i32), (i32, i32)>,
        cell: (i32, i32),
        origin: (i32, i32),
    ) -> Result<()> {
        if let Some(existing) = origins.insert(cell, origin) {
            anyhow::ensure!(
                existing == origin,
                "Conflicting reconstructed origin for {:?}: {:?} vs {:?}",
                cell,
                existing,
                origin
            );
        }
        Ok(())
    }

    fn add_origin_edge(
        adjacency: &mut HashMap<(i32, i32), Vec<OriginEdge>>,
        from: (i32, i32),
        to: (i32, i32),
        delta: (i32, i32),
        family: VerticalOrHorizontalFamily,
    ) {
        adjacency.entry(from).or_default().push(OriginEdge {
            neighbor: to,
            delta,
            family: family.clone(),
            sign: 1,
        });
        adjacency.entry(to).or_default().push(OriginEdge {
            neighbor: from,
            delta: (-delta.0, -delta.1),
            family,
            sign: -1,
        });
    }

    fn propagate_origins_from_adjacency(
        &self,
        root: (i32, i32),
        adjacency: &HashMap<(i32, i32), Vec<OriginEdge>>,
    ) -> std::result::Result<HashMap<(i32, i32), (i32, i32)>, PlacementConflict> {
        let mut origins = HashMap::new();
        let mut derivations = HashMap::<(i32, i32), OriginDerivation>::new();
        origins.insert(root, (0, 0));
        let mut frontier = VecDeque::from([root]);

        while let Some(cell) = frontier.pop_front() {
            let origin = origins[&cell];
            for edge in adjacency.get(&cell).into_iter().flatten() {
                let expected = (origin.0 + edge.delta.0, origin.1 + edge.delta.1);
                match origins.get(&edge.neighbor).copied() {
                    None => {
                        origins.insert(edge.neighbor, expected);
                        derivations.insert(
                            edge.neighbor,
                            OriginDerivation {
                                parent: cell,
                                family: edge.family.clone(),
                                sign: edge.sign,
                            },
                        );
                        frontier.push_back(edge.neighbor);
                    }
                    Some(existing) if existing == expected => {}
                    Some(existing) => {
                        let cycle_terms =
                            self.conflict_cycle_terms(cell, edge.neighbor, edge, &derivations);
                        return Err(PlacementConflict {
                            cell,
                            cell_spec: self.pieces[&cell].spec_name.clone(),
                            neighbor: edge.neighbor,
                            neighbor_spec: self.pieces[&edge.neighbor].spec_name.clone(),
                            expected,
                            found: existing,
                            cycle_terms: cycle_terms.clone(),
                            implicated_families: cycle_terms
                                .iter()
                                .map(|(family, _)| family.clone())
                                .collect(),
                        });
                    }
                }
            }
        }

        Ok(origins)
    }

    fn conflict_cycle_terms(
        &self,
        cell: (i32, i32),
        neighbor: (i32, i32),
        edge: &OriginEdge,
        derivations: &HashMap<(i32, i32), OriginDerivation>,
    ) -> Vec<(VerticalOrHorizontalFamily, i32)> {
        let mut coefficients = Self::derivation_path_terms(cell, neighbor, derivations);
        *coefficients.entry(edge.family.clone()).or_insert(0) -= edge.sign;
        coefficients.retain(|_, coefficient| *coefficient != 0);
        coefficients.into_iter().collect()
    }

    fn derivation_path_terms(
        from: (i32, i32),
        to: (i32, i32),
        derivations: &HashMap<(i32, i32), OriginDerivation>,
    ) -> BTreeMap<VerticalOrHorizontalFamily, i32> {
        let mut ancestors = HashSet::new();
        let mut cursor = from;
        ancestors.insert(cursor);
        while let Some(step) = derivations.get(&cursor) {
            cursor = step.parent;
            ancestors.insert(cursor);
        }

        let mut lca = to;
        let mut descent = Vec::new();
        while !ancestors.contains(&lca) {
            let Some(step) = derivations.get(&lca) else {
                return BTreeMap::new();
            };
            descent.push(step.clone());
            lca = step.parent;
        }

        let mut coefficients = BTreeMap::new();
        cursor = from;
        while cursor != lca {
            let Some(step) = derivations.get(&cursor) else {
                return BTreeMap::new();
            };
            *coefficients.entry(step.family.clone()).or_insert(0) -= step.sign;
            cursor = step.parent;
        }

        for step in descent.into_iter().rev() {
            *coefficients.entry(step.family).or_insert(0) += step.sign;
        }

        coefficients
    }

    fn conflict_family_branch_order(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
        conflict: &PlacementConflict,
    ) -> Vec<VerticalOrHorizontalFamily> {
        let mut families = conflict
            .implicated_families
            .iter()
            .filter_map(|family| {
                let alternatives = self
                    .family_alternatives(model, horizontal_choices, vertical_choices, family)
                    .ok()?;
                if alternatives.is_empty() {
                    None
                } else {
                    Some((alternatives.len(), family.clone()))
                }
            })
            .collect::<Vec<_>>();
        families.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        families.into_iter().map(|(_, family)| family).collect()
    }

    fn family_alternatives(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
        family: &VerticalOrHorizontalFamily,
    ) -> Result<Vec<(i32, i32)>> {
        let current = self.current_family_choice(horizontal_choices, vertical_choices, family)?;
        let alternatives = self
            .family_domain(model, family)?
            .iter()
            .copied()
            .filter(|candidate| *candidate != current)
            .collect();
        Ok(alternatives)
    }

    fn conflict_repair_states(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
        conflict: &PlacementConflict,
    ) -> Result<Vec<Vec<(VerticalOrHorizontalFamily, (i32, i32))>>> {
        const MAX_REPAIR_TERMS: usize = 8;
        const MAX_EXACT_REPAIR_PRODUCT: usize = 100_000;

        if conflict.cycle_terms.is_empty() {
            return Ok(Vec::new());
        }

        if conflict.cycle_terms.len() <= MAX_REPAIR_TERMS {
            let mut domains = Vec::with_capacity(conflict.cycle_terms.len());
            let mut current = Vec::with_capacity(conflict.cycle_terms.len());
            for (family, _) in &conflict.cycle_terms {
                domains.push(self.family_domain(model, family)?.to_vec());
                current.push(self.current_family_choice(
                    horizontal_choices,
                    vertical_choices,
                    family,
                )?);
            }

            let exact_product = domains.iter().fold(1usize, |product, domain| {
                product.saturating_mul(domain.len().max(1))
            });
            if exact_product <= MAX_EXACT_REPAIR_PRODUCT {
                let mut assignments = Vec::new();
                let mut partial = Vec::with_capacity(conflict.cycle_terms.len());
                Self::enumerate_cycle_assignments(
                    0,
                    &conflict.cycle_terms,
                    &domains,
                    &mut partial,
                    (0, 0),
                    &mut assignments,
                );

                let mut repairs = assignments
                    .into_iter()
                    .filter_map(|assignment| {
                        let changes = assignment
                            .iter()
                            .zip(current.iter())
                            .filter(|((_, candidate), current)| candidate != *current)
                            .count();
                        if changes == 0 {
                            None
                        } else {
                            Some((changes, assignment))
                        }
                    })
                    .collect::<Vec<_>>();
                repairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                return Ok(repairs
                    .into_iter()
                    .map(|(_, assignment)| assignment)
                    .collect());
            }
        }

        let Some(x_repairs) = self.axis_conflict_repair_states(
            model,
            horizontal_choices,
            vertical_choices,
            conflict,
            0,
        )?
        else {
            return Ok(Vec::new());
        };
        let Some(y_repairs) = self.axis_conflict_repair_states(
            model,
            horizontal_choices,
            vertical_choices,
            conflict,
            1,
        )?
        else {
            return Ok(Vec::new());
        };

        let mut repairs = Vec::new();
        for x_assignment in &x_repairs {
            for y_assignment in &y_repairs {
                let mut assignment = x_assignment.clone();
                assignment.extend(y_assignment.iter().cloned());
                assignment.sort();
                if !assignment.is_empty() {
                    repairs.push(assignment);
                }
            }
        }
        repairs.sort();
        repairs.dedup();
        Ok(repairs)
    }

    fn axis_conflict_repair_states(
        &self,
        model: &PlacementModel,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
        conflict: &PlacementConflict,
        axis: usize,
    ) -> Result<Option<Vec<Vec<(VerticalOrHorizontalFamily, (i32, i32))>>>> {
        let mut variable_terms = Vec::<(VerticalOrHorizontalFamily, i32)>::new();
        let mut variable_domains = Vec::<Vec<(i32, i32)>>::new();
        let mut constant_sum = 0i32;

        for (family, coefficient) in &conflict.cycle_terms {
            let domain = self.family_domain(model, family)?.to_vec();
            let distinct_x = domain.iter().map(|delta| delta.0).collect::<BTreeSet<_>>();
            let distinct_y = domain.iter().map(|delta| delta.1).collect::<BTreeSet<_>>();
            let varies_x = distinct_x.len() > 1;
            let varies_y = distinct_y.len() > 1;
            if varies_x && varies_y {
                return Ok(None);
            }

            let current =
                self.current_family_choice(horizontal_choices, vertical_choices, family)?;
            let varies_on_axis = if axis == 0 { varies_x } else { varies_y };
            if varies_on_axis {
                variable_terms.push((family.clone(), *coefficient));
                variable_domains.push(domain);
            } else {
                constant_sum += coefficient * if axis == 0 { current.0 } else { current.1 };
            }
        }

        if variable_terms.is_empty() {
            return Ok(Some(if constant_sum == 0 {
                vec![Vec::new()]
            } else {
                Vec::new()
            }));
        }

        let mut assignments = Vec::new();
        let mut partial = Vec::with_capacity(variable_terms.len());
        Self::enumerate_axis_cycle_assignments(
            0,
            axis,
            &variable_terms,
            &variable_domains,
            &mut partial,
            constant_sum,
            &mut assignments,
        );
        Ok(Some(assignments))
    }

    fn enumerate_cycle_assignments(
        idx: usize,
        terms: &[(VerticalOrHorizontalFamily, i32)],
        domains: &[Vec<(i32, i32)>],
        partial: &mut Vec<(VerticalOrHorizontalFamily, (i32, i32))>,
        sum: (i32, i32),
        out: &mut Vec<Vec<(VerticalOrHorizontalFamily, (i32, i32))>>,
    ) {
        if idx == terms.len() {
            if sum == (0, 0) {
                out.push(partial.clone());
            }
            return;
        }

        let (family, coefficient) = &terms[idx];
        for &candidate in &domains[idx] {
            partial.push((family.clone(), candidate));
            Self::enumerate_cycle_assignments(
                idx + 1,
                terms,
                domains,
                partial,
                (
                    sum.0 + coefficient * candidate.0,
                    sum.1 + coefficient * candidate.1,
                ),
                out,
            );
            partial.pop();
        }
    }

    fn enumerate_axis_cycle_assignments(
        idx: usize,
        axis: usize,
        terms: &[(VerticalOrHorizontalFamily, i32)],
        domains: &[Vec<(i32, i32)>],
        partial: &mut Vec<(VerticalOrHorizontalFamily, (i32, i32))>,
        sum: i32,
        out: &mut Vec<Vec<(VerticalOrHorizontalFamily, (i32, i32))>>,
    ) {
        if idx == terms.len() {
            if sum == 0 {
                out.push(partial.clone());
            }
            return;
        }

        let (family, coefficient) = &terms[idx];
        for &candidate in &domains[idx] {
            partial.push((family.clone(), candidate));
            Self::enumerate_axis_cycle_assignments(
                idx + 1,
                axis,
                terms,
                domains,
                partial,
                sum + coefficient * if axis == 0 { candidate.0 } else { candidate.1 },
                out,
            );
            partial.pop();
        }
    }

    fn current_family_choice(
        &self,
        horizontal_choices: &BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &BTreeMap<VerticalFamilyKey, (i32, i32)>,
        family: &VerticalOrHorizontalFamily,
    ) -> Result<(i32, i32)> {
        match family {
            VerticalOrHorizontalFamily::Horizontal(family) => horizontal_choices
                .get(family)
                .copied()
                .with_context(|| format!("Missing horizontal choice for family {:?}", family)),
            VerticalOrHorizontalFamily::Vertical(family) => vertical_choices
                .get(family)
                .copied()
                .with_context(|| format!("Missing vertical choice for family {:?}", family)),
        }
    }

    fn family_domain<'b>(
        &self,
        model: &'b PlacementModel,
        family: &VerticalOrHorizontalFamily,
    ) -> Result<&'b [(i32, i32)]> {
        match family {
            VerticalOrHorizontalFamily::Horizontal(family) => model
                .horizontal_domains
                .get(family)
                .map(Vec::as_slice)
                .with_context(|| format!("Missing horizontal domain for family {:?}", family)),
            VerticalOrHorizontalFamily::Vertical(family) => model
                .vertical_domains
                .get(family)
                .map(Vec::as_slice)
                .with_context(|| format!("Missing vertical domain for family {:?}", family)),
        }
    }

    fn set_family_choice(
        &self,
        horizontal_choices: &mut BTreeMap<HorizontalFamilyKey, (i32, i32)>,
        vertical_choices: &mut BTreeMap<VerticalFamilyKey, (i32, i32)>,
        family: &VerticalOrHorizontalFamily,
        candidate: (i32, i32),
    ) -> Result<(i32, i32)> {
        match family {
            VerticalOrHorizontalFamily::Horizontal(family) => horizontal_choices
                .insert(family.clone(), candidate)
                .with_context(|| format!("Missing horizontal choice for family {:?}", family)),
            VerticalOrHorizontalFamily::Vertical(family) => vertical_choices
                .insert(family.clone(), candidate)
                .with_context(|| format!("Missing vertical choice for family {:?}", family)),
        }
    }

    fn build_placement_model(&self) -> Result<PlacementModel> {
        let timing = board_timing_enabled();
        let mut horizontal_domains = BTreeMap::new();
        let horizontal_started = Instant::now();
        let horizontal_corridors = self.build_horizontal_corridors()?;
        if timing {
            let max_segments = horizontal_corridors
                .iter()
                .map(|(prepared, _)| prepared.segments.len())
                .max()
                .unwrap_or(0);
            let max_domain = horizontal_corridors
                .iter()
                .map(|(_, domain)| domain.len())
                .max()
                .unwrap_or(0);
            eprintln!(
                "[rev_gol_proof] board build: collapsed {} raw horizontal links into {} corridors in {:?} (max {} segments, max domain {})",
                self.horizontal_links.len(),
                horizontal_corridors.len(),
                horizontal_started.elapsed(),
                max_segments,
                max_domain
            );
        }
        let mut horizontal_links = Vec::with_capacity(horizontal_corridors.len());
        let mut horizontal_lookup = HashMap::with_capacity(horizontal_corridors.len());
        for (prepared, candidates) in horizontal_corridors {
            let family = prepared.family.clone();
            if let Some(existing) = horizontal_domains.get(&family) {
                if existing != &candidates {
                    anyhow::bail!(
                        "Inconsistent horizontal domain for family {:?}: {:?} vs {:?}",
                        family,
                        existing,
                        candidates
                    );
                }
            } else {
                horizontal_domains.insert(family.clone(), candidates);
            }
            horizontal_lookup.insert((prepared.left, prepared.right), family);
            horizontal_links.push(prepared);
        }

        let mut vertical_domains = BTreeMap::new();
        let vertical_started = Instant::now();
        let vertical_corridors = self.build_vertical_corridors()?;
        if timing {
            let max_segments = vertical_corridors
                .iter()
                .map(|(prepared, _)| prepared.segments.len())
                .max()
                .unwrap_or(0);
            let max_domain = vertical_corridors
                .iter()
                .map(|(_, domain)| domain.len())
                .max()
                .unwrap_or(0);
            eprintln!(
                "[rev_gol_proof] board build: collapsed {} raw vertical links into {} corridors in {:?} (max {} segments, max domain {})",
                self.vertical_links.len(),
                vertical_corridors.len(),
                vertical_started.elapsed(),
                max_segments,
                max_domain
            );
        }
        let mut vertical_links = Vec::with_capacity(vertical_corridors.len());
        let mut vertical_lookup = HashMap::with_capacity(vertical_corridors.len());
        for (prepared, candidates) in vertical_corridors {
            let family = prepared.family.clone();
            if let Some(existing) = vertical_domains.get(&family) {
                if existing != &candidates {
                    anyhow::bail!(
                        "Inconsistent vertical domain for family {:?}: {:?} vs {:?}",
                        family,
                        existing,
                        candidates
                    );
                }
            } else {
                vertical_domains.insert(family.clone(), candidates);
            }
            vertical_lookup.insert((prepared.top, prepared.bottom), family);
            vertical_links.push(prepared);
        }

        let mut plaquettes = Vec::new();
        for &northwest in self.pieces.keys() {
            let northeast = (northwest.0 + 1, northwest.1);
            let southwest = (northwest.0, northwest.1 + 1);
            let southeast = (northwest.0 + 1, northwest.1 + 1);
            if !self.pieces.contains_key(&northeast)
                || !self.pieces.contains_key(&southwest)
                || !self.pieces.contains_key(&southeast)
            {
                continue;
            }

            let Some(top) = horizontal_lookup.get(&(northwest, northeast)).cloned() else {
                continue;
            };
            let Some(bottom) = horizontal_lookup.get(&(southwest, southeast)).cloned() else {
                continue;
            };
            let Some(left) = vertical_lookup.get(&(northwest, southwest)).cloned() else {
                continue;
            };
            let Some(right) = vertical_lookup.get(&(northeast, southeast)).cloned() else {
                continue;
            };

            plaquettes.push(PlaquetteConstraint {
                northwest,
                top,
                bottom,
                left,
                right,
            });
        }

        let mut graph_edges = Vec::with_capacity(horizontal_links.len() + vertical_links.len());
        for link in &horizontal_links {
            let id = graph_edges.len();
            graph_edges.push(PlacementGraphEdge {
                id,
                from: link.left,
                to: link.right,
                family: VerticalOrHorizontalFamily::Horizontal(link.family.clone()),
            });
        }
        for link in &vertical_links {
            let id = graph_edges.len();
            graph_edges.push(PlacementGraphEdge {
                id,
                from: link.top,
                to: link.bottom,
                family: VerticalOrHorizontalFamily::Vertical(link.family.clone()),
            });
        }
        let mut cycle_nodes = BTreeSet::new();
        for link in &horizontal_links {
            cycle_nodes.insert(link.left);
            cycle_nodes.insert(link.right);
        }
        for link in &vertical_links {
            cycle_nodes.insert(link.top);
            cycle_nodes.insert(link.bottom);
        }
        if cycle_nodes.is_empty() {
            cycle_nodes.extend(self.pieces.keys().copied());
        }
        let cycle_constraints = Self::build_cycle_constraints(cycle_nodes, &graph_edges);

        Ok(PlacementModel {
            horizontal_domains,
            vertical_domains,
            horizontal_links,
            vertical_links,
            plaquettes,
            cycle_constraints,
        })
    }

    fn build_horizontal_corridors(&self) -> Result<Vec<(PreparedHorizontalLink, Vec<(i32, i32)>)>> {
        let mut horizontal_neighbors = HashMap::<(i32, i32), Vec<(i32, i32)>>::new();
        let mut vertical_degrees = HashMap::<(i32, i32), usize>::new();
        for &(left, right) in &self.horizontal_links {
            horizontal_neighbors.entry(left).or_default().push(right);
            horizontal_neighbors.entry(right).or_default().push(left);
        }
        for &(top, bottom) in &self.vertical_links {
            *vertical_degrees.entry(top).or_insert(0) += 1;
            *vertical_degrees.entry(bottom).or_insert(0) += 1;
        }

        let mut lanes = BTreeMap::<i32, BTreeSet<i32>>::new();
        let mut links = HashSet::<((i32, i32), (i32, i32))>::new();
        for &(left, right) in &self.horizontal_links {
            lanes.entry(left.1).or_default().insert(left.0);
            lanes.entry(right.1).or_default().insert(right.0);
            links.insert((left, right));
        }

        let mut corridors = Vec::new();
        for (lane_y, xs) in lanes {
            let xs = xs.into_iter().collect::<Vec<_>>();
            let mut component_start = 0usize;
            while component_start < xs.len() {
                let mut component_end = component_start;
                while component_end + 1 < xs.len()
                    && links
                        .contains(&((xs[component_end], lane_y), (xs[component_end + 1], lane_y)))
                {
                    component_end += 1;
                }

                let cells = xs[component_start..=component_end]
                    .iter()
                    .map(|&x| (x, lane_y))
                    .collect::<Vec<_>>();
                if cells.len() >= 2 {
                    let mut corridor_start = 0usize;
                    for idx in 1..cells.len() {
                        if !self.is_collapsible_horizontal_wire(
                            cells[idx],
                            &horizontal_neighbors,
                            &vertical_degrees,
                        ) {
                            corridors.push(
                                self.prepare_horizontal_corridor(&cells[corridor_start..=idx])?,
                            );
                            corridor_start = idx;
                        }
                    }
                }

                component_start = component_end + 1;
            }
        }

        Ok(corridors)
    }

    fn build_vertical_corridors(&self) -> Result<Vec<(PreparedVerticalLink, Vec<(i32, i32)>)>> {
        let mut vertical_neighbors = HashMap::<(i32, i32), Vec<(i32, i32)>>::new();
        let mut horizontal_degrees = HashMap::<(i32, i32), usize>::new();
        for &(top, bottom) in &self.vertical_links {
            vertical_neighbors.entry(top).or_default().push(bottom);
            vertical_neighbors.entry(bottom).or_default().push(top);
        }
        for &(left, right) in &self.horizontal_links {
            *horizontal_degrees.entry(left).or_insert(0) += 1;
            *horizontal_degrees.entry(right).or_insert(0) += 1;
        }

        let mut columns = BTreeMap::<i32, BTreeSet<i32>>::new();
        let mut links = HashSet::<((i32, i32), (i32, i32))>::new();
        for &(top, bottom) in &self.vertical_links {
            columns.entry(top.0).or_default().insert(top.1);
            columns.entry(bottom.0).or_default().insert(bottom.1);
            links.insert((top, bottom));
        }

        let mut corridors = Vec::new();
        for (lane_x, ys) in columns {
            let ys = ys.into_iter().collect::<Vec<_>>();
            let mut component_start = 0usize;
            while component_start < ys.len() {
                let mut component_end = component_start;
                while component_end + 1 < ys.len()
                    && links
                        .contains(&((lane_x, ys[component_end]), (lane_x, ys[component_end + 1])))
                {
                    component_end += 1;
                }

                let cells = ys[component_start..=component_end]
                    .iter()
                    .map(|&y| (lane_x, y))
                    .collect::<Vec<_>>();
                if cells.len() >= 2 {
                    let mut corridor_start = 0usize;
                    for idx in 1..cells.len() {
                        if !self.is_collapsible_vertical_wire(
                            cells[idx],
                            &vertical_neighbors,
                            &horizontal_degrees,
                        ) {
                            corridors.push(
                                self.prepare_vertical_corridor(&cells[corridor_start..=idx])?,
                            );
                            corridor_start = idx;
                        }
                    }
                }

                component_start = component_end + 1;
            }
        }

        Ok(corridors)
    }

    fn is_collapsible_horizontal_wire(
        &self,
        cell: (i32, i32),
        horizontal_neighbors: &HashMap<(i32, i32), Vec<(i32, i32)>>,
        vertical_degrees: &HashMap<(i32, i32), usize>,
    ) -> bool {
        self.pieces
            .get(&cell)
            .is_some_and(|piece| piece.spec_name == "horizontal wire tile")
            && horizontal_neighbors.get(&cell).map_or(0, Vec::len) == 2
            && vertical_degrees.get(&cell).copied().unwrap_or(0) == 0
    }

    fn is_collapsible_vertical_wire(
        &self,
        cell: (i32, i32),
        vertical_neighbors: &HashMap<(i32, i32), Vec<(i32, i32)>>,
        horizontal_degrees: &HashMap<(i32, i32), usize>,
    ) -> bool {
        self.pieces
            .get(&cell)
            .is_some_and(|piece| piece.spec_name == "vertical wire tile")
            && vertical_neighbors.get(&cell).map_or(0, Vec::len) == 2
            && horizontal_degrees.get(&cell).copied().unwrap_or(0) == 0
    }

    fn prepare_horizontal_corridor(
        &self,
        cells: &[(i32, i32)],
    ) -> Result<(PreparedHorizontalLink, Vec<(i32, i32)>)> {
        anyhow::ensure!(
            cells.len() >= 2,
            "Horizontal corridor requires at least two cells"
        );

        let start = cells[0];
        let end = *cells.last().unwrap();
        let segments = cells
            .windows(2)
            .map(|window| (window[0], window[1]))
            .collect::<Vec<_>>();

        let mut segment_domains = Vec::with_capacity(segments.len());
        let mut default_total = (0, 0);
        let mut first_family = None;
        for &(left, right) in &segments {
            let (family, candidates) = self.horizontal_family_candidates(left, right)?;
            default_total.0 += candidates
                .first()
                .copied()
                .context("Horizontal corridor segment unexpectedly has no candidates")?
                .0;
            default_total.1 += candidates
                .first()
                .copied()
                .context("Horizontal corridor segment unexpectedly has no candidates")?
                .1;
            if first_family.is_none() {
                first_family = Some(family);
            }
            segment_domains.push(candidates);
        }

        let mut candidates = Self::aggregate_corridor_candidates(&segment_domains, default_total);
        let corridor_name = if segments.len() == 1 {
            first_family
                .as_ref()
                .map(|family| family.connector_name.clone())
                .unwrap_or_else(|| "corridor".to_string())
        } else {
            format!("corridor({})", segments.len())
        };
        candidates.sort_by_key(|candidate| manhattan_delta_distance(*candidate, default_total));

        Ok((
            PreparedHorizontalLink {
                left: start,
                right: end,
                family: HorizontalFamilyKey {
                    left_spec: self.pieces[&start].spec_name.clone(),
                    connector_name: corridor_name,
                    right_spec: self.pieces[&end].spec_name.clone(),
                    lane_y: start.1,
                    phase_x: start.0.rem_euclid(3),
                    start_x: start.0,
                    end_x: end.0,
                },
                segments,
            },
            candidates,
        ))
    }

    fn prepare_vertical_corridor(
        &self,
        cells: &[(i32, i32)],
    ) -> Result<(PreparedVerticalLink, Vec<(i32, i32)>)> {
        anyhow::ensure!(
            cells.len() >= 2,
            "Vertical corridor requires at least two cells"
        );

        let start = cells[0];
        let end = *cells.last().unwrap();
        let segments = cells
            .windows(2)
            .map(|window| (window[0], window[1]))
            .collect::<Vec<_>>();

        let mut segment_domains = Vec::with_capacity(segments.len());
        let mut default_total = (0, 0);
        for &(top, bottom) in &segments {
            let (_, candidates) = self.vertical_family_candidates(top, bottom)?;
            default_total.0 += candidates
                .first()
                .copied()
                .context("Vertical corridor segment unexpectedly has no candidates")?
                .0;
            default_total.1 += candidates
                .first()
                .copied()
                .context("Vertical corridor segment unexpectedly has no candidates")?
                .1;
            segment_domains.push(candidates);
        }

        let mut candidates = Self::aggregate_corridor_candidates(&segment_domains, default_total);
        candidates.sort_by_key(|candidate| manhattan_delta_distance(*candidate, default_total));

        Ok((
            PreparedVerticalLink {
                top: start,
                bottom: end,
                family: VerticalFamilyKey {
                    top_spec: self.pieces[&start].spec_name.clone(),
                    bottom_spec: self.pieces[&end].spec_name.clone(),
                    lane_x: start.0,
                    phase_y: start.1.rem_euclid(3),
                    start_y: start.1,
                    end_y: end.1,
                },
                segments,
            },
            candidates,
        ))
    }

    fn aggregate_corridor_candidates(
        segment_domains: &[Vec<(i32, i32)>],
        default_total: (i32, i32),
    ) -> Vec<(i32, i32)> {
        if let Some(candidates) = Self::aggregate_axis_corridor_candidates(segment_domains, 0, 1) {
            return Self::sort_corridor_candidates(candidates, default_total);
        }
        if let Some(candidates) = Self::aggregate_axis_corridor_candidates(segment_domains, 1, 0) {
            return Self::sort_corridor_candidates(candidates, default_total);
        }

        let mut sums = BTreeSet::from([(0, 0)]);
        for domain in segment_domains {
            let mut next = BTreeSet::new();
            for sum in &sums {
                for &candidate in domain {
                    next.insert((sum.0 + candidate.0, sum.1 + candidate.1));
                }
            }
            sums = next;
        }
        Self::sort_corridor_candidates(sums.into_iter().collect(), default_total)
    }

    fn aggregate_axis_corridor_candidates(
        segment_domains: &[Vec<(i32, i32)>],
        varying_axis: usize,
        fixed_axis: usize,
    ) -> Option<Vec<(i32, i32)>> {
        let mut fixed_total = 0i32;
        let mut min_sum = 0i32;
        let mut max_sum = 0i32;
        let mut common_step = None::<i32>;
        let mut axis_domains = Vec::with_capacity(segment_domains.len());

        for domain in segment_domains {
            let fixed_value = match fixed_axis {
                0 => domain.first().map(|candidate| candidate.0)?,
                1 => domain.first().map(|candidate| candidate.1)?,
                _ => return None,
            };
            if !domain.iter().all(|candidate| match fixed_axis {
                0 => candidate.0 == fixed_value,
                1 => candidate.1 == fixed_value,
                _ => false,
            }) {
                return None;
            }
            fixed_total += fixed_value;

            let mut axis_values = domain
                .iter()
                .map(|candidate| match varying_axis {
                    0 => candidate.0,
                    1 => candidate.1,
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            axis_values.sort_unstable();
            axis_values.dedup();

            let first = *axis_values.first()?;
            let last = *axis_values.last()?;
            min_sum += first;
            max_sum += last;

            if axis_values.len() > 1 {
                let step = axis_values[1] - axis_values[0];
                if axis_values
                    .windows(2)
                    .any(|window| window[1] - window[0] != step)
                {
                    common_step = Some(-1);
                } else {
                    match common_step {
                        None => common_step = Some(step),
                        Some(existing) if existing == step => {}
                        Some(_) => common_step = Some(-1),
                    }
                }
            }
            axis_domains.push(axis_values);
        }

        if common_step != Some(-1) {
            let values = if min_sum == max_sum {
                vec![min_sum]
            } else {
                let step = common_step.unwrap_or(1);
                if step <= 0 {
                    return None;
                }
                let mut values = Vec::new();
                let mut current = min_sum;
                while current <= max_sum {
                    values.push(current);
                    current += step;
                }
                values
            };
            return Some(
                values
                    .into_iter()
                    .map(|sum| match varying_axis {
                        0 => (sum, fixed_total),
                        1 => (fixed_total, sum),
                        _ => unreachable!(),
                    })
                    .collect(),
            );
        }

        let mut sums = HashSet::from([0i32]);
        for axis_values in axis_domains {
            let current = sums.iter().copied().collect::<Vec<_>>();
            let mut next = HashSet::with_capacity(current.len() * axis_values.len());
            for sum in current {
                for value in &axis_values {
                    next.insert(sum + value);
                }
            }
            sums = next;
        }

        Some(
            sums.into_iter()
                .map(|sum| match varying_axis {
                    0 => (sum, fixed_total),
                    1 => (fixed_total, sum),
                    _ => unreachable!(),
                })
                .collect(),
        )
    }

    fn sort_corridor_candidates(
        mut candidates: Vec<(i32, i32)>,
        default_total: (i32, i32),
    ) -> Vec<(i32, i32)> {
        candidates.sort_by_key(|candidate| manhattan_delta_distance(*candidate, default_total));
        candidates
    }

    fn build_cycle_constraints(
        nodes: impl IntoIterator<Item = (i32, i32)>,
        edges: &[PlacementGraphEdge],
    ) -> Vec<CycleConstraint> {
        let mut adjacency = HashMap::<(i32, i32), Vec<(usize, (i32, i32), i32)>>::new();
        for edge in edges {
            adjacency
                .entry(edge.from)
                .or_default()
                .push((edge.id, edge.to, 1));
            adjacency
                .entry(edge.to)
                .or_default()
                .push((edge.id, edge.from, -1));
        }

        let mut visited = HashSet::new();
        let mut parent = HashMap::<(i32, i32), TreeParentStep>::new();
        let mut tree_edges = HashSet::new();

        for root in nodes {
            if !visited.insert(root) {
                continue;
            }
            let mut frontier = VecDeque::from([root]);
            while let Some(cell) = frontier.pop_front() {
                for &(edge_id, neighbor, sign) in adjacency.get(&cell).into_iter().flatten() {
                    if visited.insert(neighbor) {
                        let edge = &edges[edge_id];
                        parent.insert(
                            neighbor,
                            TreeParentStep {
                                parent: cell,
                                family: edge.family.clone(),
                                sign,
                            },
                        );
                        tree_edges.insert(edge_id);
                        frontier.push_back(neighbor);
                    }
                }
            }
        }

        let mut constraints = Vec::new();
        for edge in edges {
            if tree_edges.contains(&edge.id) {
                continue;
            }

            let mut coefficients = Self::tree_path_terms(edge.from, edge.to, &parent);
            *coefficients.entry(edge.family.clone()).or_insert(0) -= 1;
            coefficients.retain(|_, coefficient| *coefficient != 0);
            if coefficients.is_empty() {
                continue;
            }

            constraints.push(CycleConstraint {
                closing_edge: (edge.from, edge.to),
                terms: coefficients.into_iter().collect(),
            });
        }

        constraints
    }

    fn tree_path_terms(
        from: (i32, i32),
        to: (i32, i32),
        parent: &HashMap<(i32, i32), TreeParentStep>,
    ) -> BTreeMap<VerticalOrHorizontalFamily, i32> {
        let mut ancestors = HashSet::new();
        let mut cursor = from;
        ancestors.insert(cursor);
        while let Some(step) = parent.get(&cursor) {
            cursor = step.parent;
            ancestors.insert(cursor);
        }

        let mut lca = to;
        let mut descent = Vec::new();
        while !ancestors.contains(&lca) {
            let Some(step) = parent.get(&lca) else {
                return BTreeMap::new();
            };
            descent.push(step.clone());
            lca = step.parent;
        }

        let mut coefficients = BTreeMap::new();
        cursor = from;
        while cursor != lca {
            let Some(step) = parent.get(&cursor) else {
                return BTreeMap::new();
            };
            *coefficients.entry(step.family.clone()).or_insert(0) -= step.sign;
            cursor = step.parent;
        }

        for step in descent.into_iter().rev() {
            *coefficients.entry(step.family).or_insert(0) += step.sign;
        }

        coefficients
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
        let left_align = left_info
            .pattern
            .phase_alignment()
            .0
            .context("Missing east phase")?;
        let right_align = right_info
            .pattern
            .phase_alignment()
            .2
            .context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info
            .anchors
            .west
            .context("Missing connector west anchor")?;
        let connector_east = connector_info
            .anchors
            .east
            .context("Missing connector east anchor")?;
        let default_candidate = default_horizontal_placement_candidate(
            (0, 0),
            left_anchor,
            right_anchor,
            connector_west,
            connector_east,
        );
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        for candidate in synthesized_horizontal_fallback_candidates(default_candidate) {
            push_unique_horizontal_candidate(&mut candidates, candidate);
        }
        push_unique_horizontal_candidate(&mut candidates, default_candidate);
        candidates.sort_by_key(|candidate| {
            (
                manhattan_delta_distance(candidate.right_delta, default_candidate.right_delta),
                manhattan_delta_distance(
                    candidate.connector_delta,
                    default_candidate.connector_delta,
                ),
            )
        });
        Ok((
            HorizontalFamilyKey {
                left_spec: left_piece.spec_name.clone(),
                connector_name: connector_name.to_string(),
                right_spec: right_piece.spec_name.clone(),
                lane_y: left.1,
                phase_x: left.0.rem_euclid(3),
                start_x: left.0,
                end_x: right.0,
            },
            candidates
                .into_iter()
                .map(|candidate| candidate.right_delta)
                .collect(),
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
        let default_candidate =
            default_vertical_placement_candidate((0, 0), top_anchor, bottom_anchor);
        let mut candidates =
            known_vertical_placement_candidates(&top_piece.spec_name, &bottom_piece.spec_name)
                .to_vec();
        for candidate in synthesized_vertical_fallback_candidates(default_candidate) {
            push_unique_vertical_candidate(&mut candidates, candidate);
        }
        push_unique_vertical_candidate(&mut candidates, default_candidate);
        candidates.sort_by_key(|candidate| {
            manhattan_delta_distance(candidate.bottom_delta, default_candidate.bottom_delta)
        });
        Ok((
            VerticalFamilyKey {
                top_spec: top_piece.spec_name.clone(),
                bottom_spec: bottom_piece.spec_name.clone(),
                lane_x: top.0,
                phase_y: top.1.rem_euclid(3),
                start_y: top.1,
                end_y: bottom.1,
            },
            candidates
                .into_iter()
                .map(|candidate| candidate.bottom_delta)
                .collect(),
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
        if let Some(limit) = self.options.exact_search_state_limit {
            if stats.visited_states > limit {
                anyhow::bail!(
                    "Exact placement search exceeded the configured limit of {limit} visited states"
                );
            }
        }
        let progress_interval = self.options.exact_search_progress_interval.max(1);
        if stats.visited_states >= stats.reported_states + progress_interval {
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
            .map(|(&(cell_x, cell_y), &(origin_x, origin_y))| (cell_x, cell_y, origin_x, origin_y))
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
        let left_align = left_info
            .pattern
            .phase_alignment()
            .0
            .context("Missing east phase")?;
        let right_align = right_info
            .pattern
            .phase_alignment()
            .2
            .context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info
            .anchors
            .west
            .context("Missing connector west anchor")?;
        let connector_east = connector_info
            .anchors
            .east
            .context("Missing connector east anchor")?;
        let default_candidate = default_horizontal_placement_candidate(
            (0, 0),
            left_anchor,
            right_anchor,
            connector_west,
            connector_east,
        );
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        for candidate in synthesized_horizontal_fallback_candidates(default_candidate) {
            push_unique_horizontal_candidate(&mut candidates, candidate);
        }
        push_unique_horizontal_candidate(&mut candidates, default_candidate);
        candidates.sort_by_key(|candidate| {
            (
                manhattan_delta_distance(candidate.right_delta, default_candidate.right_delta),
                manhattan_delta_distance(
                    candidate.connector_delta,
                    default_candidate.connector_delta,
                ),
            )
        });
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
        let default_candidate =
            default_vertical_placement_candidate((0, 0), top_anchor, bottom_anchor);
        let mut candidates =
            known_vertical_placement_candidates(&top_piece.spec_name, &bottom_piece.spec_name)
                .to_vec();
        for candidate in synthesized_vertical_fallback_candidates(default_candidate) {
            push_unique_vertical_candidate(&mut candidates, candidate);
        }
        push_unique_vertical_candidate(&mut candidates, default_candidate);
        candidates.sort_by_key(|candidate| {
            manhattan_delta_distance(candidate.bottom_delta, default_candidate.bottom_delta)
        });
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
        let left_align = left_info
            .pattern
            .phase_alignment()
            .0
            .context("Missing east phase")?;
        let right_align = right_info
            .pattern
            .phase_alignment()
            .2
            .context("Missing west phase")?;
        let connector_name = self.library.connector_name(left_align, right_align)?;
        let connector_info = self.library.pattern(connector_name)?;
        let connector_west = connector_info
            .anchors
            .west
            .context("Missing connector west anchor")?;
        let connector_east = connector_info
            .anchors
            .east
            .context("Missing connector east anchor")?;
        let default_candidate = default_horizontal_placement_candidate(
            left_origin,
            left_anchor,
            right_anchor,
            connector_west,
            connector_east,
        );
        let mut candidates = known_horizontal_placement_candidates(
            &left_piece.spec_name,
            connector_name,
            &right_piece.spec_name,
        )
        .to_vec();
        for candidate in synthesized_horizontal_fallback_candidates(default_candidate) {
            push_unique_horizontal_candidate(&mut candidates, candidate);
        }
        push_unique_horizontal_candidate(&mut candidates, default_candidate);

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
    fn test_audit_published_board_motifs_for_small_formula() {
        if !published_root().exists() {
            return;
        }

        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let audit = audit_published_board_motifs(&construction).unwrap();

        assert!(!audit.horizontal.is_empty());
        assert!(!audit.vertical.is_empty());
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
