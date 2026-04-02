//! Macro-level `SAT -> Rev_GOL` construction compiler.
//!
//! This module does not yet route and stamp a final published-pattern board.
//! Instead it builds the end-to-end composition object we need first:
//! a Boolean circuit, explicit splitter expansion, concrete gadget placements,
//! and the netlist between gadget ports.

use crate::circuit::{Circuit, CircuitNodeKind, CnfFormula, NodeId};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroKind {
    InputPort { variable: String },
    NotGate,
    OrGate,
    Splitter,
    Enforcer,
}

impl MacroKind {
    pub fn published_spec_name(&self) -> Option<&'static str> {
        match self {
            MacroKind::InputPort { .. } => None,
            MacroKind::NotGate => Some("NOT gate tile"),
            MacroKind::OrGate => Some("OR gate tile"),
            MacroKind::Splitter => Some("splitter tile"),
            MacroKind::Enforcer => Some("enforcer gadget"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroInstance {
    pub id: InstanceId,
    pub kind: MacroKind,
    pub column: usize,
    pub row: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortRef {
    pub instance: InstanceId,
    pub port: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    ExternalInput { variable: String },
    InstancePort(PortRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub from: Endpoint,
    pub to: Endpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledConstruction {
    pub instances: Vec<MacroInstance>,
    pub nets: Vec<Net>,
    pub output_sink: InstanceId,
    pub variable_inputs: BTreeMap<String, Endpoint>,
}

impl CompiledConstruction {
    pub fn bounds(&self) -> (usize, usize) {
        let width = self
            .instances
            .iter()
            .map(|item| item.column)
            .max()
            .unwrap_or(0)
            + 1;
        let height = self
            .instances
            .iter()
            .map(|item| item.row)
            .max()
            .unwrap_or(0)
            + 1;
        (width, height)
    }

    pub fn render_blueprint(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Instances:".to_string());
        for instance in &self.instances {
            let spec = instance.kind.published_spec_name().unwrap_or("<external>");
            lines.push(format!(
                "  #{:02} {:?} [{}] @ ({}, {})",
                instance.id.0, instance.kind, spec, instance.column, instance.row
            ));
        }
        lines.push("Nets:".to_string());
        for net in &self.nets {
            lines.push(format!("  {:?} -> {:?}", net.from, net.to));
        }
        lines.join("\n")
    }

    pub fn input_variables(&self) -> Vec<String> {
        self.variable_inputs.keys().cloned().collect()
    }

    pub fn evaluate(&self, assignment: &BTreeMap<String, bool>) -> Result<bool> {
        let mut values = HashMap::<Endpoint, bool>::new();

        for variable in self.input_variables() {
            let value = assignment
                .get(&variable)
                .copied()
                .with_context(|| format!("Missing assignment for input '{variable}'"))?;
            values.insert(Endpoint::ExternalInput { variable }, value);
        }

        let mut changed = true;
        while changed {
            changed = false;

            for net in &self.nets {
                if let Some(value) = values.get(&net.from).copied() {
                    match values.get(&net.to).copied() {
                        Some(existing) if existing != value => {
                            anyhow::bail!(
                                "Conflicting values on {:?}: {} vs {}",
                                net.to,
                                existing,
                                value
                            );
                        }
                        Some(_) => {}
                        None => {
                            values.insert(net.to.clone(), value);
                            changed = true;
                        }
                    }
                }
            }

            for instance in &self.instances {
                changed |= self.derive_instance_outputs(instance, &mut values)?;
            }
        }

        let sink_input = Endpoint::InstancePort(PortRef {
            instance: self.output_sink,
            port: "in",
        });
        Ok(values.get(&sink_input).copied().unwrap_or(false))
    }

    fn derive_instance_outputs(
        &self,
        instance: &MacroInstance,
        values: &mut HashMap<Endpoint, bool>,
    ) -> Result<bool> {
        let mut changed = false;

        match &instance.kind {
            MacroKind::InputPort { variable } => {
                if let Some(value) = values
                    .get(&Endpoint::ExternalInput {
                        variable: variable.clone(),
                    })
                    .copied()
                {
                    changed |= Self::set_instance_port(values, instance.id, "out", value)?;
                }
            }
            MacroKind::NotGate => {
                if let Some(value) = Self::get_instance_port(values, instance.id, "in") {
                    changed |= Self::set_instance_port(values, instance.id, "out", !value)?;
                }
            }
            MacroKind::OrGate => {
                if let (Some(lhs), Some(rhs)) = (
                    Self::get_instance_port(values, instance.id, "lhs"),
                    Self::get_instance_port(values, instance.id, "rhs"),
                ) {
                    changed |= Self::set_instance_port(values, instance.id, "out", lhs || rhs)?;
                }
            }
            MacroKind::Splitter => {
                if let Some(value) = Self::get_instance_port(values, instance.id, "in") {
                    changed |= Self::set_instance_port(values, instance.id, "out0", value)?;
                    changed |= Self::set_instance_port(values, instance.id, "out1", value)?;
                }
            }
            MacroKind::Enforcer => {}
        }

        Ok(changed)
    }

    fn get_instance_port(
        values: &HashMap<Endpoint, bool>,
        instance: InstanceId,
        port: &'static str,
    ) -> Option<bool> {
        values
            .get(&Endpoint::InstancePort(PortRef { instance, port }))
            .copied()
    }

    fn set_instance_port(
        values: &mut HashMap<Endpoint, bool>,
        instance: InstanceId,
        port: &'static str,
        value: bool,
    ) -> Result<bool> {
        let endpoint = Endpoint::InstancePort(PortRef { instance, port });
        match values.get(&endpoint).copied() {
            Some(existing) if existing != value => {
                anyhow::bail!(
                    "Conflicting derived values on {:?}: {} vs {}",
                    endpoint,
                    existing,
                    value
                );
            }
            Some(_) => Ok(false),
            None => {
                values.insert(endpoint, value);
                Ok(true)
            }
        }
    }
}

pub struct ConstructionCompiler;

impl ConstructionCompiler {
    pub fn compile_cnf(formula: &CnfFormula) -> Result<CompiledConstruction> {
        let circuit = Circuit::from_cnf(formula);
        Self::compile_circuit(&circuit)
    }

    pub fn compile_circuit(circuit: &Circuit) -> Result<CompiledConstruction> {
        let depths = circuit.depths();
        let mut placements = Vec::new();
        let mut next_instance = 0usize;
        let mut rows_by_depth = HashMap::<usize, usize>::new();
        let mut instance_by_node = HashMap::<NodeId, InstanceId>::new();
        let mut variable_inputs = BTreeMap::new();

        for (variable, &node_id) in circuit.inputs() {
            variable_inputs.insert(
                variable.clone(),
                Endpoint::ExternalInput {
                    variable: variable.clone(),
                },
            );
            // Reserve an initial row so later gate placement remains stable.
            rows_by_depth.entry(depths[&node_id]).or_insert(0);
        }

        for node in circuit.nodes() {
            let depth = depths[&node.id];
            let row = rows_by_depth.entry(depth).or_insert(0);

            let kind = match &node.kind {
                CircuitNodeKind::Input { .. } => continue,
                CircuitNodeKind::Const { .. } => continue,
                CircuitNodeKind::Not { .. } => MacroKind::NotGate,
                CircuitNodeKind::Or { .. } => MacroKind::OrGate,
            };

            let id = InstanceId(next_instance);
            next_instance += 1;
            placements.push(MacroInstance {
                id,
                kind,
                column: depth + 1,
                row: *row,
            });
            instance_by_node.insert(node.id, id);
            *row += 1;
        }

        let output_row = placements
            .iter()
            .find(|instance| instance.id == instance_by_node[&circuit.output()])
            .map(|instance| instance.row)
            .unwrap_or(0);
        let output_sink = InstanceId(next_instance);
        placements.push(MacroInstance {
            id: output_sink,
            kind: MacroKind::Enforcer,
            column: depths[&circuit.output()] + 2,
            row: output_row,
        });
        next_instance += 1;

        let mut compiler = LayoutExpansion {
            placements,
            nets: Vec::new(),
            next_instance,
            next_aux_row: circuit.nodes().len() + circuit.inputs().len(),
            instance_by_node,
        };

        let fanout = circuit.fanout_counts();
        let consumer_ports = Self::collect_consumers(circuit, &compiler.instance_by_node);

        for node in circuit.nodes() {
            let mut targets = consumer_ports.get(&node.id).cloned().unwrap_or_default();
            if node.id == circuit.output() {
                targets.push(TargetEndpoint::InputPort(PortRef {
                    instance: output_sink,
                    port: "in",
                }));
            }
            if targets.is_empty() {
                continue;
            }

            let source = match &node.kind {
                CircuitNodeKind::Input { name } => Endpoint::ExternalInput {
                    variable: name.clone(),
                },
                CircuitNodeKind::Const { value } => Endpoint::ExternalInput {
                    variable: format!("const_{}", u8::from(*value)),
                },
                CircuitNodeKind::Not { .. } | CircuitNodeKind::Or { .. } => {
                    Endpoint::InstancePort(PortRef {
                        instance: compiler.instance_by_node[&node.id],
                        port: "out",
                    })
                }
            };
            let splitter_start_column = match node.kind {
                CircuitNodeKind::Input { .. } | CircuitNodeKind::Const { .. } => 1,
                CircuitNodeKind::Not { .. } | CircuitNodeKind::Or { .. } => depths[&node.id] + 2,
            };

            if fanout.get(&node.id).copied().unwrap_or(0) <= 1 && targets.len() == 1 {
                compiler.connect(source, targets.remove(0));
            } else {
                compiler.expand_splitter_tree(source, targets, splitter_start_column);
            }
        }

        Ok(CompiledConstruction {
            instances: compiler.placements,
            nets: compiler.nets,
            output_sink,
            variable_inputs,
        })
    }

    fn collect_consumers(
        circuit: &Circuit,
        instance_by_node: &HashMap<NodeId, InstanceId>,
    ) -> HashMap<NodeId, Vec<TargetEndpoint>> {
        let mut out = HashMap::<NodeId, Vec<TargetEndpoint>>::new();
        for node in circuit.nodes() {
            match node.kind {
                CircuitNodeKind::Input { .. } | CircuitNodeKind::Const { .. } => {}
                CircuitNodeKind::Not { input } => {
                    out.entry(input)
                        .or_default()
                        .push(TargetEndpoint::InputPort(PortRef {
                            instance: instance_by_node[&node.id],
                            port: "in",
                        }));
                }
                CircuitNodeKind::Or { lhs, rhs } => {
                    out.entry(lhs)
                        .or_default()
                        .push(TargetEndpoint::InputPort(PortRef {
                            instance: instance_by_node[&node.id],
                            port: "lhs",
                        }));
                    out.entry(rhs)
                        .or_default()
                        .push(TargetEndpoint::InputPort(PortRef {
                            instance: instance_by_node[&node.id],
                            port: "rhs",
                        }));
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetEndpoint {
    InputPort(PortRef),
}

struct LayoutExpansion {
    placements: Vec<MacroInstance>,
    nets: Vec<Net>,
    next_instance: usize,
    next_aux_row: usize,
    instance_by_node: HashMap<NodeId, InstanceId>,
}

impl LayoutExpansion {
    fn connect(&mut self, from: Endpoint, to: TargetEndpoint) {
        match to {
            TargetEndpoint::InputPort(port) => self.nets.push(Net {
                from,
                to: Endpoint::InstancePort(port),
            }),
        }
    }

    fn expand_splitter_tree(
        &mut self,
        source: Endpoint,
        mut targets: Vec<TargetEndpoint>,
        start_column: usize,
    ) {
        if targets.len() == 1 {
            self.connect(source, targets.remove(0));
            return;
        }

        let mid = targets.len() / 2;
        let right_targets = targets.split_off(mid);
        let left_targets = targets;

        let splitter = InstanceId(self.next_instance);
        self.next_instance += 1;
        let row = self.average_target_row(&left_targets, &right_targets);
        self.placements.push(MacroInstance {
            id: splitter,
            kind: MacroKind::Splitter,
            column: start_column,
            row,
        });

        self.nets.push(Net {
            from: source,
            to: Endpoint::InstancePort(PortRef {
                instance: splitter,
                port: "in",
            }),
        });

        self.expand_splitter_tree(
            Endpoint::InstancePort(PortRef {
                instance: splitter,
                port: "out0",
            }),
            left_targets,
            start_column + 1,
        );
        self.expand_splitter_tree(
            Endpoint::InstancePort(PortRef {
                instance: splitter,
                port: "out1",
            }),
            right_targets,
            start_column + 1,
        );
    }

    fn average_target_row(
        &mut self,
        left_targets: &[TargetEndpoint],
        right_targets: &[TargetEndpoint],
    ) -> usize {
        let rows = left_targets
            .iter()
            .chain(right_targets.iter())
            .map(|target| self.target_row(target))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            let row = self.next_aux_row;
            self.next_aux_row += 1;
            return row;
        }

        rows.iter().sum::<usize>() / rows.len()
    }

    fn target_row(&self, target: &TargetEndpoint) -> usize {
        match target {
            TargetEndpoint::InputPort(port) => self
                .placements
                .iter()
                .find(|instance| instance.id == port.instance)
                .map(|instance| instance.row)
                .unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, Literal};

    #[test]
    fn test_compile_cnf_produces_enforcer_sink() {
        let formula = CnfFormula::new(vec![Clause::new(vec![Literal::positive("x")])]);
        let compiled = ConstructionCompiler::compile_cnf(&formula).unwrap();

        assert!(compiled
            .instances
            .iter()
            .any(|instance| matches!(instance.kind, MacroKind::Enforcer)));
        assert!(compiled
            .nets
            .iter()
            .any(|net| matches!(net.to, Endpoint::InstancePort(ref p) if p.instance == compiled.output_sink)));
    }

    #[test]
    fn test_compile_cnf_inserts_splitters_for_shared_inputs() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x"), Literal::positive("y")]),
            Clause::new(vec![Literal::negative("x"), Literal::positive("z")]),
        ]);
        let compiled = ConstructionCompiler::compile_cnf(&formula).unwrap();

        assert!(compiled
            .instances
            .iter()
            .any(|instance| matches!(instance.kind, MacroKind::Splitter)));
        assert!(compiled.variable_inputs.contains_key("x"));
    }

    #[test]
    fn test_render_blueprint_mentions_instances_and_nets() {
        let formula = CnfFormula::new(vec![Clause::new(vec![Literal::positive("x")])]);
        let compiled = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let blueprint = compiled.render_blueprint();

        assert!(blueprint.contains("Instances:"));
        assert!(blueprint.contains("Nets:"));
        assert!(blueprint.contains("OR gate tile") || blueprint.contains("enforcer gadget"));
    }

    #[test]
    fn test_compiled_construction_matches_formula_truth_table() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let compiled = ConstructionCompiler::compile_cnf(&formula).unwrap();

        for mask in 0..8usize {
            let assignment = BTreeMap::from([
                ("x1".to_string(), (mask & 1) != 0),
                ("x2".to_string(), (mask & 2) != 0),
                ("x3".to_string(), (mask & 4) != 0),
            ]);
            assert_eq!(
                formula.evaluate(&assignment),
                compiled.evaluate(&assignment).unwrap(),
                "assignment {:?} should preserve satisfiability through the compiler",
                assignment
            );
        }
    }
}
