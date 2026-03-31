//! SAT-backed verifier scaffolding for candidate reverse-Life gadgets.

use anyhow::{Context, Result};
use rev_gol::config::SolverBackend;
use rev_gol::game_of_life::Grid;
use rev_gol::sat::constraints::Clause;
use rev_gol::sat::{SolverOptions, UnifiedSatSolver};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

/// A cell in the predecessor layer of a gadget instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellCoord {
    pub x: isize,
    pub y: isize,
}

impl CellCoord {
    pub const fn new(x: isize, y: isize) -> Self {
        Self { x, y }
    }
}

/// A forced predecessor literal used to encode a port state or boundary condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellLiteral {
    pub coord: CellCoord,
    pub alive: bool,
}

impl CellLiteral {
    pub const fn alive(x: isize, y: isize) -> Self {
        Self {
            coord: CellCoord::new(x, y),
            alive: true,
        }
    }

    pub const fn dead(x: isize, y: isize) -> Self {
        Self {
            coord: CellCoord::new(x, y),
            alive: false,
        }
    }
}

/// A named interface on the predecessor boundary of a gadget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub states: BTreeMap<String, Vec<CellLiteral>>,
}

impl Port {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            states: BTreeMap::new(),
        }
    }

    pub fn with_state(mut self, state_name: impl Into<String>, literals: Vec<CellLiteral>) -> Self {
        self.states.insert(state_name.into(), literals);
        self
    }

    pub fn literals_for_state(&self, state_name: &str) -> Result<&[CellLiteral]> {
        self.states
            .get(state_name)
            .map(Vec::as_slice)
            .with_context(|| format!("Port '{}' has no state '{}'", self.name, state_name))
    }

    pub fn state_names(&self) -> Vec<String> {
        self.states.keys().cloned().collect()
    }
}

/// A candidate gadget target pattern and its predecessor-side interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GadgetPattern {
    pub name: String,
    pub target: Grid,
    pub ports: Vec<Port>,
    /// Additional predecessor literals that should always hold, such as a dead border
    /// away from connector cells.
    pub base_predecessor_literals: Vec<CellLiteral>,
}

impl GadgetPattern {
    pub fn new(name: impl Into<String>, target: Grid) -> Self {
        Self {
            name: name.into(),
            target,
            ports: Vec::new(),
            base_predecessor_literals: Vec::new(),
        }
    }

    pub fn with_port(mut self, port: Port) -> Self {
        self.ports.push(port);
        self
    }

    pub fn with_base_predecessor_literals(mut self, literals: Vec<CellLiteral>) -> Self {
        self.base_predecessor_literals = literals;
        self
    }

    fn port(&self, port_name: &str) -> Result<&Port> {
        self.ports
            .iter()
            .find(|port| port.name == port_name)
            .with_context(|| format!("Unknown port '{}' on gadget '{}'", port_name, self.name))
    }

    fn validate_coords(&self, coord: CellCoord) -> Result<()> {
        if !self.preimage_domain().contains(&coord) {
            anyhow::bail!(
                "Coordinate ({}, {}) is outside gadget '{}' preimage domain",
                coord.x,
                coord.y,
                self.name
            );
        }

        Ok(())
    }

    fn preimage_domain(&self) -> HashSet<CellCoord> {
        let mut domain = HashSet::new();
        for y in 0..self.target.height {
            for x in 0..self.target.width {
                domain.extend(life_neighborhood(CellCoord::new(x as isize, y as isize)));
            }
        }
        domain
    }
}

/// An assignment of named port states to a gadget instance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortAssignment {
    pub states: BTreeMap<String, String>,
}

impl PortAssignment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_state(
        mut self,
        port_name: impl Into<String>,
        state_name: impl Into<String>,
    ) -> Self {
        self.states.insert(port_name.into(), state_name.into());
        self
    }
}

/// Configuration for the gadget verifier SAT backend.
#[derive(Debug, Clone)]
pub struct GadgetVerifierConfig {
    pub backend: SolverBackend,
    pub num_threads: Option<usize>,
    pub enable_preprocessing: bool,
    pub verbosity: u32,
    pub timeout: Option<Duration>,
}

impl Default for GadgetVerifierConfig {
    fn default() -> Self {
        Self {
            backend: SolverBackend::Parkissat,
            num_threads: None,
            enable_preprocessing: true,
            verbosity: 0,
            timeout: None,
        }
    }
}

/// Result of a SAT check for a concrete port assignment.
#[derive(Debug, Clone)]
pub struct AssignmentCheck {
    pub assignment: PortAssignment,
    pub satisfiable: bool,
}

/// Summary of allowed/forbidden assignment checks.
#[derive(Debug, Clone)]
pub struct RelationCheckReport {
    pub allowed_assignments_hold: bool,
    pub forbidden_assignments_hold: bool,
    pub allowed_results: Vec<AssignmentCheck>,
    pub forbidden_results: Vec<AssignmentCheck>,
}

/// Result of checking a charging rule against all reachable projected port states.
#[derive(Debug, Clone)]
pub struct ChargingCheck {
    pub fixed_assignment: PortAssignment,
    pub output_ports: Vec<String>,
    pub all_outputs_are_named_states: bool,
    pub observed_outputs: Vec<ProjectedPortState>,
}

/// A projected predecessor assignment observed on a set of ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPortState {
    pub states: BTreeMap<String, Option<String>>,
}

/// SAT-backed gadget verifier.
pub struct GadgetVerifier {
    config: GadgetVerifierConfig,
}

#[derive(Debug, Clone)]
struct ProofSatInstance {
    clauses: Vec<Clause>,
    vars: HashMap<CellCoord, i32>,
}

struct PreparedProofSolver {
    instance: ProofSatInstance,
    solver: UnifiedSatSolver,
}

pub(crate) fn life_neighborhood(coord: CellCoord) -> [CellCoord; 9] {
    let x = coord.x;
    let y = coord.y;
    if y % 2 != 0 {
        if x % 2 != 0 {
            [
                coord,
                CellCoord::new(x - 1, y),
                CellCoord::new(x - 1, y - 1),
                CellCoord::new(x + 1, y),
                CellCoord::new(x + 1, y - 1),
                CellCoord::new(x - 1, y + 1),
                CellCoord::new(x, y + 1),
                CellCoord::new(x + 1, y + 1),
                CellCoord::new(x, y - 1),
            ]
        } else {
            [
                coord,
                CellCoord::new(x - 1, y),
                CellCoord::new(x - 1, y - 1),
                CellCoord::new(x + 1, y),
                CellCoord::new(x + 1, y - 1),
                CellCoord::new(x + 1, y + 1),
                CellCoord::new(x, y + 1),
                CellCoord::new(x - 1, y + 1),
                CellCoord::new(x, y - 1),
            ]
        }
    } else if x % 2 != 0 {
        [
            coord,
            CellCoord::new(x - 1, y + 1),
            CellCoord::new(x - 1, y),
            CellCoord::new(x + 1, y + 1),
            CellCoord::new(x + 1, y),
            CellCoord::new(x - 1, y - 1),
            CellCoord::new(x, y - 1),
            CellCoord::new(x + 1, y - 1),
            CellCoord::new(x, y + 1),
        ]
    } else {
        [
            coord,
            CellCoord::new(x - 1, y + 1),
            CellCoord::new(x - 1, y),
            CellCoord::new(x + 1, y + 1),
            CellCoord::new(x + 1, y),
            CellCoord::new(x + 1, y - 1),
            CellCoord::new(x, y - 1),
            CellCoord::new(x - 1, y - 1),
            CellCoord::new(x, y + 1),
        ]
    }
}

fn life_next(center_alive: bool, live_neighbors: usize) -> bool {
    match (center_alive, live_neighbors) {
        (true, 2 | 3) => true,
        (false, 3) => true,
        _ => false,
    }
}

impl GadgetVerifier {
    pub fn new(config: GadgetVerifierConfig) -> Self {
        Self { config }
    }

    /// Check whether a candidate gadget admits a preimage matching the given port assignment.
    pub fn check_assignment(
        &self,
        gadget: &GadgetPattern,
        assignment: &PortAssignment,
    ) -> Result<bool> {
        let mut prepared = self.prepare_gadget(gadget)?;
        self.check_assignment_with_prepared(gadget, &mut prepared, assignment)
    }

    /// Check a relation claim by testing explicit allowed and forbidden assignments.
    pub fn verify_relation(
        &self,
        gadget: &GadgetPattern,
        allowed_assignments: &[PortAssignment],
        forbidden_assignments: &[PortAssignment],
    ) -> Result<RelationCheckReport> {
        let mut prepared = self.prepare_gadget(gadget)?;
        let mut allowed_results = Vec::new();
        for assignment in allowed_assignments {
            allowed_results.push(AssignmentCheck {
                assignment: assignment.clone(),
                satisfiable: self.check_assignment_with_prepared(
                    gadget,
                    &mut prepared,
                    assignment,
                )?,
            });
        }

        let mut forbidden_results = Vec::new();
        for assignment in forbidden_assignments {
            forbidden_results.push(AssignmentCheck {
                assignment: assignment.clone(),
                satisfiable: self.check_assignment_with_prepared(
                    gadget,
                    &mut prepared,
                    assignment,
                )?,
            });
        }

        let allowed_assignments_hold = allowed_results.iter().all(|result| result.satisfiable);
        let forbidden_assignments_hold = forbidden_results.iter().all(|result| !result.satisfiable);

        Ok(RelationCheckReport {
            allowed_assignments_hold,
            forbidden_assignments_hold,
            allowed_results,
            forbidden_results,
        })
    }

    /// Enumerate all distinct observed port states compatible with a fixed partial assignment.
    pub fn enumerate_projected_port_states(
        &self,
        gadget: &GadgetPattern,
        fixed_assignment: &PortAssignment,
        observed_ports: &[String],
    ) -> Result<Vec<ProjectedPortState>> {
        let mut prepared = self.prepare_gadget(gadget)?;
        self.enumerate_projected_port_states_with_prepared(
            gadget,
            &mut prepared,
            fixed_assignment,
            observed_ports,
        )
    }

    fn enumerate_projected_port_states_with_prepared(
        &self,
        gadget: &GadgetPattern,
        prepared: &mut PreparedProofSolver,
        fixed_assignment: &PortAssignment,
        observed_ports: &[String],
    ) -> Result<Vec<ProjectedPortState>> {
        let fixed_assumptions =
            self.collect_assumptions(gadget, &prepared.instance, fixed_assignment)?;

        let mut blocking_var_set = std::collections::BTreeSet::new();
        for port_name in observed_ports {
            let port = gadget.port(port_name)?;
            for literals in port.states.values() {
                for literal in literals {
                    let var = Self::var_for_coord(&prepared.instance, literal.coord)?;
                    blocking_var_set.insert(var);
                }
            }
        }
        let blocking_vars = blocking_var_set.into_iter().collect::<Vec<_>>();

        let mut projected = Vec::new();
        loop {
            let satisfiable = prepared
                .solver
                .solve_under_assumptions(&fixed_assumptions)
                .context("Failed to enumerate projected gadget states")?;
            if !satisfiable {
                break;
            }

            let observed_values = blocking_vars
                .iter()
                .map(|&var| Ok((var, self.model_value(&prepared.solver, var)?)))
                .collect::<Result<HashMap<_, _>>>()?;

            let mut states = BTreeMap::new();
            for port_name in observed_ports {
                let port = gadget.port(port_name)?;
                let mut matched_state = None;

                for (state_name, literals) in &port.states {
                    let matches = literals.iter().all(|literal| {
                        let Ok(var) = Self::var_for_coord(&prepared.instance, literal.coord) else {
                            return false;
                        };
                        observed_values.get(&var).copied().unwrap_or(false) == literal.alive
                    });

                    if matches {
                        matched_state = Some(state_name.clone());
                        break;
                    }
                }

                states.insert(port_name.clone(), matched_state);
            }

            projected.push(ProjectedPortState { states });

            if blocking_vars.is_empty() {
                break;
            }

            let blocking_clause = Clause::new(
                blocking_vars
                    .iter()
                    .map(|&var| {
                        if observed_values.get(&var).copied().unwrap_or(false) {
                            -var
                        } else {
                            var
                        }
                    })
                    .collect(),
            );
            prepared
                .solver
                .add_clause(&blocking_clause)
                .context("Failed to add projected-state blocking clause")?;
        }

        projected.sort_by(|a, b| a.states.cmp(&b.states));
        projected.dedup();
        Ok(projected)
    }

    /// Verify that every projected output under a fixed partial assignment lands in named port states.
    pub fn verify_charging_rule(
        &self,
        gadget: &GadgetPattern,
        fixed_assignment: &PortAssignment,
        output_ports: &[String],
    ) -> Result<ChargingCheck> {
        let mut prepared = self.prepare_gadget(gadget)?;
        let observed_outputs = self.enumerate_projected_port_states_with_prepared(
            gadget,
            &mut prepared,
            fixed_assignment,
            output_ports,
        )?;
        let all_outputs_are_named_states = observed_outputs.iter().all(|state| {
            output_ports
                .iter()
                .all(|port_name| state.states.get(port_name).is_some_and(Option::is_some))
        });

        Ok(ChargingCheck {
            fixed_assignment: fixed_assignment.clone(),
            output_ports: output_ports.to_vec(),
            all_outputs_are_named_states,
            observed_outputs,
        })
    }

    /// Enumerate all named assignments for a chosen subset of states on each port.
    pub fn enumerate_assignments(
        &self,
        gadget: &GadgetPattern,
        states_by_port: &BTreeMap<String, Vec<String>>,
    ) -> Result<Vec<PortAssignment>> {
        let mut normalized = Vec::new();

        for (port_name, states) in states_by_port {
            let port = gadget.port(port_name)?;
            let allowed_states = if states.is_empty() {
                port.state_names()
            } else {
                let mut collected = Vec::new();
                for state in states {
                    port.literals_for_state(state)?;
                    collected.push(state.clone());
                }
                collected
            };

            normalized.push((port_name.clone(), allowed_states));
        }

        let mut results = Vec::new();
        let mut current = PortAssignment::new();
        Self::enumerate_assignments_recursive(&normalized, 0, &mut current, &mut results);
        Ok(results)
    }

    fn enumerate_assignments_recursive(
        states_by_port: &[(String, Vec<String>)],
        index: usize,
        current: &mut PortAssignment,
        results: &mut Vec<PortAssignment>,
    ) {
        if index == states_by_port.len() {
            results.push(current.clone());
            return;
        }

        let (port_name, states) = &states_by_port[index];
        for state in states {
            current.states.insert(port_name.clone(), state.clone());
            Self::enumerate_assignments_recursive(states_by_port, index + 1, current, results);
        }
        current.states.remove(port_name);
    }

    fn collect_predecessor_literals(
        &self,
        gadget: &GadgetPattern,
        assignment: &PortAssignment,
    ) -> Result<Vec<CellLiteral>> {
        let mut merged = HashMap::<CellCoord, bool>::new();

        for &literal in &gadget.base_predecessor_literals {
            gadget.validate_coords(literal.coord)?;
            Self::insert_literal(&mut merged, literal, "base predecessor literal")?;
        }

        for (port_name, state_name) in &assignment.states {
            let port = gadget.port(port_name)?;
            for &literal in port.literals_for_state(state_name)? {
                gadget.validate_coords(literal.coord)?;
                Self::insert_literal(
                    &mut merged,
                    literal,
                    &format!("state '{}' on port '{}'", state_name, port_name),
                )?;
            }
        }

        let mut result: Vec<CellLiteral> = merged
            .into_iter()
            .map(|(coord, alive)| CellLiteral { coord, alive })
            .collect();
        result.sort_by_key(|literal| literal.coord);
        Ok(result)
    }

    fn insert_literal(
        merged: &mut HashMap<CellCoord, bool>,
        literal: CellLiteral,
        source: &str,
    ) -> Result<()> {
        if let Some(existing) = merged.insert(literal.coord, literal.alive) {
            if existing != literal.alive {
                anyhow::bail!(
                    "Conflicting predecessor requirements at ({}, {}) while processing {}",
                    literal.coord.x,
                    literal.coord.y,
                    source
                );
            }
        }

        Ok(())
    }

    fn build_sat_instance(&self, gadget: &GadgetPattern) -> Result<ProofSatInstance> {
        let mut domain: Vec<CellCoord> = gadget.preimage_domain().into_iter().collect();
        domain.sort_unstable();

        let vars = domain
            .into_iter()
            .enumerate()
            .map(|(idx, coord)| (coord, idx as i32 + 1))
            .collect::<HashMap<_, _>>();

        let mut clauses = Vec::new();
        for y in 0..gadget.target.height {
            for x in 0..gadget.target.width {
                let coord = CellCoord::new(x as isize, y as isize);
                let neighborhood = life_neighborhood(coord);
                let vars_for_cell = neighborhood
                    .iter()
                    .map(|coord| {
                        vars.get(coord).copied().with_context(|| {
                            format!("Missing neighborhood variable for {:?}", coord)
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let target_alive = gadget.target.get(y, x);

                for mask in 0..(1usize << vars_for_cell.len()) {
                    let center_alive = (mask & 1) != 0;
                    let live_neighbors = (1..vars_for_cell.len())
                        .filter(|bit| (mask & (1 << bit)) != 0)
                        .count();
                    if life_next(center_alive, live_neighbors) == target_alive {
                        continue;
                    }

                    let clause = vars_for_cell
                        .iter()
                        .enumerate()
                        .map(|(bit, var)| {
                            if (mask & (1 << bit)) != 0 {
                                -*var
                            } else {
                                *var
                            }
                        })
                        .collect();
                    clauses.push(Clause::new(clause));
                }
            }
        }

        Ok(ProofSatInstance { clauses, vars })
    }

    fn prepare_gadget(&self, gadget: &GadgetPattern) -> Result<PreparedProofSolver> {
        let instance = self
            .build_sat_instance(gadget)
            .with_context(|| format!("Failed to build SAT encoding for gadget '{}'", gadget.name))?;
        let mut solver = UnifiedSatSolver::new(self.config.backend)
            .context("Failed to create solver for gadget verification")?;
        solver
            .configure(&SolverOptions {
                num_threads: self.config.num_threads,
                enable_preprocessing: self.config.enable_preprocessing,
                verbosity: self.config.verbosity,
                timeout: self.config.timeout,
                random_seed: None,
            })
            .context("Failed to configure solver for gadget verification")?;
        solver
            .add_clauses(&instance.clauses)
            .context("Failed to load gadget clauses into SAT solver")?;

        Ok(PreparedProofSolver { instance, solver })
    }

    fn check_assignment_with_prepared(
        &self,
        gadget: &GadgetPattern,
        prepared: &mut PreparedProofSolver,
        assignment: &PortAssignment,
    ) -> Result<bool> {
        let assumptions = self.collect_assumptions(gadget, &prepared.instance, assignment)?;
        prepared.solver.solve_under_assumptions(&assumptions)
    }

    fn collect_assumptions(
        &self,
        gadget: &GadgetPattern,
        instance: &ProofSatInstance,
        assignment: &PortAssignment,
    ) -> Result<Vec<i32>> {
        self.collect_predecessor_literals(gadget, assignment)?
            .into_iter()
            .map(|literal| {
                let var = Self::var_for_coord(instance, literal.coord)?;
                Ok(if literal.alive { var } else { -var })
            })
            .collect()
    }

    fn var_for_coord(instance: &ProofSatInstance, coord: CellCoord) -> Result<i32> {
        instance
            .vars
            .get(&coord)
            .copied()
            .with_context(|| format!("No variable allocated for {:?}", coord))
    }

    fn model_value(&self, solver: &UnifiedSatSolver, variable: i32) -> Result<bool> {
        Ok(solver.model_value(variable)?.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rev_gol::config::BoundaryCondition;

    fn blinker_gadget() -> GadgetPattern {
        let target = Grid::from_cells(
            vec![
                vec![false, false, false],
                vec![true, true, true],
                vec![false, false, false],
            ],
            BoundaryCondition::Dead,
        )
        .unwrap();

        GadgetPattern::new("blinker", target)
            .with_port(
                Port::new("top")
                    .with_state("alive", vec![CellLiteral::alive(1, 0)])
                    .with_state("dead", vec![CellLiteral::dead(1, 0)]),
            )
            .with_port(
                Port::new("mid")
                    .with_state("alive", vec![CellLiteral::alive(1, 1)])
                    .with_state("dead", vec![CellLiteral::dead(1, 1)]),
            )
            .with_port(
                Port::new("bot")
                    .with_state("alive", vec![CellLiteral::alive(1, 2)])
                    .with_state("dead", vec![CellLiteral::dead(1, 2)]),
            )
            .with_base_predecessor_literals(vec![
                CellLiteral::dead(0, 0),
                CellLiteral::dead(2, 0),
                CellLiteral::dead(0, 1),
                CellLiteral::dead(2, 1),
                CellLiteral::dead(0, 2),
                CellLiteral::dead(2, 2),
            ])
    }

    #[test]
    fn test_check_assignment_accepts_known_blinker_predecessor() {
        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let gadget = blinker_gadget();
        let assignment = PortAssignment::new()
            .with_state("top", "alive")
            .with_state("mid", "alive")
            .with_state("bot", "alive");

        assert!(verifier.check_assignment(&gadget, &assignment).unwrap());
    }

    #[test]
    fn test_check_assignment_rejects_incompatible_predecessor() {
        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let gadget = blinker_gadget();
        let assignment = PortAssignment::new()
            .with_state("top", "dead")
            .with_state("mid", "dead")
            .with_state("bot", "dead");

        assert!(!verifier.check_assignment(&gadget, &assignment).unwrap());
    }

    #[test]
    fn test_verify_relation_reports_allowed_and_forbidden_cases() {
        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let gadget = blinker_gadget();
        let allowed = vec![PortAssignment::new()
            .with_state("top", "alive")
            .with_state("mid", "alive")
            .with_state("bot", "alive")];
        let forbidden = vec![PortAssignment::new()
            .with_state("top", "dead")
            .with_state("mid", "dead")
            .with_state("bot", "dead")];

        let report = verifier
            .verify_relation(&gadget, &allowed, &forbidden)
            .unwrap();

        assert!(report.allowed_assignments_hold);
        assert!(report.forbidden_assignments_hold);
    }

    #[test]
    fn test_enumerate_assignments_builds_cartesian_product() {
        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let gadget = blinker_gadget();
        let assignments = verifier
            .enumerate_assignments(
                &gadget,
                &BTreeMap::from([
                    (
                        "top".to_string(),
                        vec!["alive".to_string(), "dead".to_string()],
                    ),
                    (
                        "mid".to_string(),
                        vec!["alive".to_string(), "dead".to_string()],
                    ),
                ]),
            )
            .unwrap();

        assert_eq!(assignments.len(), 4);
    }

    #[test]
    fn test_projected_port_states_cover_known_blinker_options() {
        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let gadget = blinker_gadget();
        let projected = verifier
            .enumerate_projected_port_states(
                &gadget,
                &PortAssignment::new(),
                &["top".to_string(), "mid".to_string(), "bot".to_string()],
            )
            .unwrap();

        assert!(projected.iter().any(|state| {
            state.states.get("top") == Some(&Some("alive".to_string()))
                && state.states.get("mid") == Some(&Some("alive".to_string()))
                && state.states.get("bot") == Some(&Some("alive".to_string()))
        }));
    }
}
