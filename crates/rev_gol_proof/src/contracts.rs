//! Symbolic gadget contracts and construction certificates for the `SAT -> Rev-GOL` proof path.
//!
//! This layer is intentionally independent of the experimental published-board stamper.
//! It binds macro instances to published gadget relations, checks that the compiled netlist is
//! well-typed over those contracts, and records the remaining proof obligations explicitly.

use crate::compiler::{
    CompiledConstruction, Endpoint, InstanceId, MacroInstance, MacroKind, PortRef,
};
use crate::interfaces::{enumerate_router_interface_basis, InterfaceBasisCertificate};
use crate::inputs::{certify_input_boundary_encoding, InputBoundaryCertificate};
use crate::padding::{certify_dead_boundary_padding, DeadBoundaryPaddingCertificate};
use crate::published::{
    published_connector_specs, published_root, published_spec_named, relation_assignments,
    verify_published_spec, PublishedSpec,
};
use crate::routing::{construct_routing_witness, macro_port_dir, RouteDir, RoutingWitness};
use crate::verifier::{GadgetVerifier, GadgetVerifierConfig};
use anyhow::{Context, Result};
use rev_gol::config::SolverBackend;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContractPortRole {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractPortBinding {
    pub macro_port: &'static str,
    pub wire_port: &'static str,
    pub role: ContractPortRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractSemantics {
    Identity {
        input: &'static str,
        output: &'static str,
    },
    Inversion {
        input: &'static str,
        output: &'static str,
    },
    Or {
        lhs: &'static str,
        rhs: &'static str,
        out: &'static str,
    },
    Split {
        input: &'static str,
        out0: &'static str,
        out1: &'static str,
    },
    Crossing {
        horizontal_in: &'static str,
        horizontal_out: &'static str,
        vertical_in: &'static str,
        vertical_out: &'static str,
    },
    RequireTrue {
        input: &'static str,
    },
}

impl ContractSemantics {
    fn allows(&self, values: &BTreeMap<&'static str, bool>) -> bool {
        match self {
            ContractSemantics::Identity { input, output } => values[input] == values[output],
            ContractSemantics::Inversion { input, output } => values[input] != values[output],
            ContractSemantics::Or { lhs, rhs, out } => values[out] == (values[lhs] || values[rhs]),
            ContractSemantics::Split { input, out0, out1 } => {
                values[out0] == values[input] && values[out1] == values[input]
            }
            ContractSemantics::Crossing {
                horizontal_in,
                horizontal_out,
                vertical_in,
                vertical_out,
            } => {
                values[horizontal_in] == values[horizontal_out]
                    && values[vertical_in] == values[vertical_out]
            }
            ContractSemantics::RequireTrue { input } => values[input],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GadgetContract {
    pub label: &'static str,
    pub published_spec_name: &'static str,
    pub bindings: Vec<ContractPortBinding>,
    pub semantics: ContractSemantics,
}

impl GadgetContract {
    fn input_ports(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.bindings
            .iter()
            .filter(|binding| binding.role == ContractPortRole::Input)
            .map(|binding| binding.macro_port)
    }

    fn output_ports(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.bindings
            .iter()
            .filter(|binding| binding.role == ContractPortRole::Output)
            .map(|binding| binding.macro_port)
    }

    fn published_spec(&self) -> Result<PublishedSpec> {
        published_spec_named(self.published_spec_name)
            .with_context(|| format!("Missing published spec '{}'", self.published_spec_name))
    }

    fn expected_wire_relation_rows(&self) -> BTreeSet<Vec<(String, bool)>> {
        let port_names = self
            .bindings
            .iter()
            .map(|binding| binding.macro_port)
            .collect::<Vec<_>>();
        let mut allowed = BTreeSet::new();

        for mask in 0..(1usize << port_names.len()) {
            let mut macro_values = BTreeMap::<&'static str, bool>::new();
            for (idx, port_name) in port_names.iter().enumerate() {
                macro_values.insert(*port_name, (mask & (1usize << idx)) != 0);
            }
            if !self.semantics.allows(&macro_values) {
                continue;
            }

            let wire_values = self
                .bindings
                .iter()
                .map(|binding| {
                    (
                        binding.wire_port.to_string(),
                        macro_values[binding.macro_port],
                    )
                })
                .collect::<BTreeMap<_, _>>();
            allowed.insert(normalize_assignment(&wire_values));
        }

        allowed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractValidationReport {
    pub label: &'static str,
    pub published_spec_name: &'static str,
    pub relation_matches: bool,
    pub port_phases: Vec<(String, Option<i8>)>,
    pub allowed_rows: usize,
}

impl ContractValidationReport {
    pub fn render_summary(&self) -> String {
        format!(
            "{} [{}]: relation_matches={} allowed_rows={} phases={:?}",
            self.label,
            self.published_spec_name,
            self.relation_matches,
            self.allowed_rows,
            self.port_phases
        )
    }
}

#[derive(Debug, Clone)]
pub struct PrimitiveDischargeReport {
    pub label: &'static str,
    pub published_spec_name: &'static str,
    pub symbolic_relation_matches: bool,
    pub published_success: bool,
    pub size_matches: bool,
    pub alignment_matches: bool,
    pub allowed_assignments_hold: bool,
    pub forbidden_assignments_hold: bool,
    pub charging_holds: bool,
    pub error: Option<String>,
}

impl PrimitiveDischargeReport {
    pub fn is_success(&self) -> bool {
        self.symbolic_relation_matches && self.published_success
    }
}

#[derive(Debug, Clone)]
pub struct BasisDischargeReport {
    pub logical: Vec<PrimitiveDischargeReport>,
    pub routing: Vec<PrimitiveDischargeReport>,
}

impl BasisDischargeReport {
    pub fn is_success(&self) -> bool {
        self.logical
            .iter()
            .all(PrimitiveDischargeReport::is_success)
            && self
                .routing
                .iter()
                .all(PrimitiveDischargeReport::is_success)
    }

    pub fn render_summary(&self) -> String {
        format!(
            "sat_discharge_valid={} logical_verified={}/{} routing_verified={}/{}",
            self.is_success(),
            self.logical
                .iter()
                .filter(|report| report.is_success())
                .count(),
            self.logical.len(),
            self.routing
                .iter()
                .filter(|report| report.is_success())
                .count(),
            self.routing.len(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputAnchorObligation {
    pub variable: String,
    pub target: PortRef,
    pub target_spec_name: &'static str,
    pub target_dir: RouteDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetRouteObligation {
    pub from: PortRef,
    pub to: PortRef,
    pub source_spec_name: &'static str,
    pub target_spec_name: &'static str,
    pub source_dir: RouteDir,
    pub target_dir: RouteDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingBasisCoverage {
    pub straight_wires_present: bool,
    pub turn_tiles_present: bool,
    pub crossing_present: bool,
    pub always_one_present: bool,
    pub connector_phase_pairs: BTreeSet<(i8, i8)>,
}

impl RoutingBasisCoverage {
    pub fn is_rectilinear_complete(&self) -> bool {
        let expected_pairs = [-1, 0, 1]
            .into_iter()
            .flat_map(|west| [-1, 0, 1].into_iter().map(move |east| (west, east)))
            .collect::<BTreeSet<_>>();
        self.straight_wires_present
            && self.turn_tiles_present
            && self.crossing_present
            && self.connector_phase_pairs == expected_pairs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertifiedInstance {
    pub id: InstanceId,
    pub kind: String,
    pub published_spec_name: &'static str,
    pub column: usize,
    pub row: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertifiedNet {
    pub from: Endpoint,
    pub to: PortRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructionCertificate {
    pub instances: Vec<CertifiedInstance>,
    pub nets: Vec<CertifiedNet>,
    pub external_inputs: Vec<String>,
    pub logical_contracts: Vec<ContractValidationReport>,
    pub routing_contracts: Vec<ContractValidationReport>,
    pub input_anchor_obligations: Vec<InputAnchorObligation>,
    pub internal_route_obligations: Vec<NetRouteObligation>,
    pub routing_basis_coverage: RoutingBasisCoverage,
    pub routing_witness: RoutingWitness,
    pub input_boundary_certificate: InputBoundaryCertificate,
    pub dead_boundary_padding_certificate: DeadBoundaryPaddingCertificate,
    pub interface_basis_complete: bool,
    pub remaining_obligations: Vec<&'static str>,
}

impl ConstructionCertificate {
    pub fn render_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "symbolic_certificate_valid=true logical_instances={} external_inputs={} nets={} logical_contracts={} routing_contracts={} remaining_obligations={}",
            self.instances.len(),
            self.external_inputs.len(),
            self.nets.len(),
            self.logical_contracts.len(),
            self.routing_contracts.len(),
            self.remaining_obligations.len()
        ));
        lines.push(format!(
            "routing_basis_rectilinear_complete={} input_anchor_obligations={} internal_route_obligations={}",
            self.routing_basis_coverage.is_rectilinear_complete(),
            self.input_anchor_obligations.len(),
            self.internal_route_obligations.len(),
        ));
        lines.push(self.input_boundary_certificate.render_summary());
        lines.push(self.dead_boundary_padding_certificate.render_summary());
        lines.push(format!(
            "interface_basis_complete={}",
            self.interface_basis_complete
        ));
        lines.extend(
            self.routing_witness
                .render_summary()
                .lines()
                .map(str::to_string),
        );
        lines.push(format!(
            "logical_basis={}",
            self.logical_contracts
                .iter()
                .map(|report| report.published_spec_name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(format!(
            "routing_basis={}",
            self.routing_contracts
                .iter()
                .map(|report| report.published_spec_name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push("remaining_obligations:".to_string());
        for obligation in &self.remaining_obligations {
            lines.push(format!("  - {obligation}"));
        }
        lines.join("\n")
    }

    pub fn with_interface_basis_certificate(
        mut self,
        basis: &InterfaceBasisCertificate,
    ) -> Result<Self> {
        let router_basis = enumerate_router_interface_basis()?;
        basis.covers_router_basis(&router_basis)?;
        basis.covers_witness(&self.routing_witness)?;
        self.interface_basis_complete = true;
        self.remaining_obligations = if self.dead_boundary_padding_certificate.is_complete() {
            Vec::new()
        } else {
            vec!["Prove the finite dead-boundary padding lemma that isolates the compiled construction on a finite board."]
        };
        Ok(self)
    }
}

pub fn validate_logical_contract_library() -> Result<Vec<ContractValidationReport>> {
    logical_gadget_contracts()
        .into_iter()
        .map(validate_contract)
        .collect()
}

pub fn validate_routing_contract_library() -> Result<Vec<ContractValidationReport>> {
    routing_gadget_contracts()
        .into_iter()
        .map(validate_contract)
        .collect()
}

pub fn discharge_logical_contract_library(
    verifier: &GadgetVerifier,
) -> Result<Vec<PrimitiveDischargeReport>> {
    discharge_logical_contract_library_filtered(verifier, None)
}

pub fn discharge_routing_contract_library(
    verifier: &GadgetVerifier,
) -> Result<Vec<PrimitiveDischargeReport>> {
    discharge_routing_contract_library_filtered(verifier, None)
}

pub fn discharge_logical_contract_library_filtered(
    verifier: &GadgetVerifier,
    filter: Option<&str>,
) -> Result<Vec<PrimitiveDischargeReport>> {
    discharge_contracts(
        verifier,
        filter_contracts(logical_gadget_contracts(), filter),
    )
}

pub fn discharge_routing_contract_library_filtered(
    verifier: &GadgetVerifier,
    filter: Option<&str>,
) -> Result<Vec<PrimitiveDischargeReport>> {
    discharge_contracts(
        verifier,
        filter_contracts(routing_gadget_contracts(), filter),
    )
}

pub fn discharge_construction_basis(
    verifier: &GadgetVerifier,
    discharge_logical: bool,
    discharge_routing: bool,
    filter: Option<&str>,
) -> Result<BasisDischargeReport> {
    Ok(BasisDischargeReport {
        logical: if discharge_logical {
            discharge_logical_contract_library_filtered(verifier, filter)?
        } else {
            Vec::new()
        },
        routing: if discharge_routing {
            discharge_routing_contract_library_filtered(verifier, filter)?
        } else {
            Vec::new()
        },
    })
}

pub fn default_contract_verifier(timeout: Option<Duration>) -> GadgetVerifier {
    GadgetVerifier::new(GadgetVerifierConfig {
        backend: SolverBackend::Parkissat,
        num_threads: None,
        enable_preprocessing: true,
        verbosity: 0,
        timeout,
    })
}

pub fn certify_compiled_construction(
    construction: &CompiledConstruction,
) -> Result<ConstructionCertificate> {
    let logical_contracts = validate_logical_contract_library()?;
    let routing_contracts = validate_routing_contract_library()?;
    anyhow::ensure!(
        logical_contracts
            .iter()
            .all(|report| report.relation_matches),
        "Logical gadget contract library does not match the published relations"
    );
    anyhow::ensure!(
        routing_contracts
            .iter()
            .all(|report| report.relation_matches),
        "Routing gadget contract library does not match the published relations"
    );

    let instances_by_id = construction
        .instances
        .iter()
        .map(|instance| (instance.id, instance))
        .collect::<HashMap<_, _>>();
    anyhow::ensure!(
        instances_by_id.len() == construction.instances.len(),
        "Compiled construction contains duplicate instance ids"
    );

    let output_sink = instances_by_id
        .get(&construction.output_sink)
        .context("Output sink instance is missing from compiled construction")?;
    anyhow::ensure!(
        matches!(output_sink.kind, MacroKind::Enforcer),
        "Output sink {:?} is not an enforcer instance",
        output_sink.id
    );

    let contracts_by_instance = construction
        .instances
        .iter()
        .map(|instance| Ok((instance.id, logical_contract_for_instance(instance)?)))
        .collect::<Result<HashMap<_, _>>>()?;

    let mut certified_nets = Vec::with_capacity(construction.nets.len());
    let mut driver_by_target = HashMap::<PortRef, Endpoint>::new();
    let mut fanout_by_source = HashMap::<Endpoint, usize>::new();
    let mut input_anchor_obligations = Vec::new();
    let mut internal_route_obligations = Vec::new();

    for net in &construction.nets {
        certify_source_endpoint(
            net.from.clone(),
            &construction.variable_inputs,
            &contracts_by_instance,
        )?;
        let target_port = match &net.to {
            Endpoint::ExternalInput { .. } => {
                anyhow::bail!(
                    "Compiled construction net targets an external input: {:?}",
                    net
                )
            }
            Endpoint::InstancePort(port) => port.clone(),
        };
        certify_target_port(&target_port, &contracts_by_instance)?;

        if let Some(existing) = driver_by_target.insert(target_port.clone(), net.from.clone()) {
            anyhow::bail!(
                "Input port {:?} has multiple drivers: {:?} and {:?}",
                target_port,
                existing,
                net.from
            );
        }

        *fanout_by_source.entry(net.from.clone()).or_insert(0) += 1;
        certified_nets.push(CertifiedNet {
            from: net.from.clone(),
            to: target_port.clone(),
        });

        match &net.from {
            Endpoint::ExternalInput { variable } => {
                let target_instance =
                    instances_by_id
                        .get(&target_port.instance)
                        .with_context(|| {
                            format!("Missing target instance {:?}", target_port.instance)
                        })?;
                let target_contract = &contracts_by_instance[&target_port.instance];
                input_anchor_obligations.push(InputAnchorObligation {
                    variable: variable.clone(),
                    target: target_port.clone(),
                    target_spec_name: target_contract.published_spec_name,
                    target_dir: macro_port_dir(&target_instance.kind, target_port.port)?,
                });
            }
            Endpoint::InstancePort(source_port) => {
                let source_instance =
                    instances_by_id
                        .get(&source_port.instance)
                        .with_context(|| {
                            format!("Missing source instance {:?}", source_port.instance)
                        })?;
                let target_instance =
                    instances_by_id
                        .get(&target_port.instance)
                        .with_context(|| {
                            format!("Missing target instance {:?}", target_port.instance)
                        })?;
                let source_contract = &contracts_by_instance[&source_port.instance];
                let target_contract = &contracts_by_instance[&target_port.instance];
                internal_route_obligations.push(NetRouteObligation {
                    from: source_port.clone(),
                    to: target_port.clone(),
                    source_spec_name: source_contract.published_spec_name,
                    target_spec_name: target_contract.published_spec_name,
                    source_dir: macro_port_dir(&source_instance.kind, source_port.port)?,
                    target_dir: macro_port_dir(&target_instance.kind, target_port.port)?,
                });
            }
        }
    }

    for (source, fanout) in &fanout_by_source {
        anyhow::ensure!(
            *fanout <= 1,
            "Source endpoint {:?} has fanout {}; explicit splitter expansion should keep per-port fanout <= 1",
            source,
            fanout
        );
    }

    for instance in &construction.instances {
        let contract = &contracts_by_instance[&instance.id];
        for port in contract.input_ports() {
            let port_ref = PortRef {
                instance: instance.id,
                port,
            };
            anyhow::ensure!(
                driver_by_target.contains_key(&port_ref),
                "Instance {:?} is missing a driver on required input port '{}'",
                instance.id,
                port
            );
        }
    }

    let certified_instances = construction
        .instances
        .iter()
        .map(|instance| {
            let contract = &contracts_by_instance[&instance.id];
            CertifiedInstance {
                id: instance.id,
                kind: macro_kind_label(&instance.kind).to_string(),
                published_spec_name: contract.published_spec_name,
                column: instance.column,
                row: instance.row,
            }
        })
        .collect::<Vec<_>>();
    let routing_witness = construct_routing_witness(construction)?;
    let input_boundary_certificate =
        certify_input_boundary_encoding(construction, &routing_witness)?;
    let dead_boundary_padding_certificate = certify_dead_boundary_padding(&routing_witness)?;

    Ok(ConstructionCertificate {
        instances: certified_instances,
        nets: certified_nets,
        external_inputs: construction.input_variables(),
        logical_contracts,
        routing_contracts,
        input_anchor_obligations,
        internal_route_obligations,
        routing_basis_coverage: routing_basis_coverage(),
        routing_witness,
        input_boundary_certificate,
        dead_boundary_padding_certificate,
        interface_basis_complete: false,
        remaining_obligations: vec![
            "Prove local horizontal interface lemmas for the connector families used by the routing witness.",
            "Prove local vertical interface lemmas for the vertical adjacency families used by the routing witness.",
        ],
    })
}

fn validate_contract(contract: GadgetContract) -> Result<ContractValidationReport> {
    let spec = contract.published_spec()?;
    let allowed_rows = relation_assignments(&spec)
        .0
        .into_iter()
        .map(|assignment| {
            let wire_values = assignment
                .states
                .into_iter()
                .map(|(port, value)| {
                    let bit = match value.as_str() {
                        "0" => false,
                        "1" => true,
                        _ => anyhow::bail!(
                            "Unsupported non-binary published relation value '{}' on {}",
                            value,
                            spec.name
                        ),
                    };
                    Ok((port, bit))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            Ok(normalize_assignment(&wire_values))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let expected_rows = contract.expected_wire_relation_rows();
    let spec_ports = spec
        .relation_wires
        .chars()
        .map(|ch| ch.to_string())
        .collect::<BTreeSet<_>>();
    let contract_ports = contract
        .bindings
        .iter()
        .map(|binding| binding.wire_port.to_string())
        .collect::<BTreeSet<_>>();
    anyhow::ensure!(
        spec_ports == contract_ports,
        "Contract '{}' expects wire ports {:?}, but published spec '{}' exposes {:?}",
        contract.label,
        contract_ports,
        spec.name,
        spec_ports
    );

    Ok(ContractValidationReport {
        label: contract.label,
        published_spec_name: contract.published_spec_name,
        relation_matches: allowed_rows == expected_rows,
        port_phases: contract
            .bindings
            .iter()
            .map(|binding| {
                (
                    binding.macro_port.to_string(),
                    phase_for_wire(&spec, binding.wire_port),
                )
            })
            .collect(),
        allowed_rows: expected_rows.len(),
    })
}

fn discharge_contracts(
    verifier: &GadgetVerifier,
    contracts: Vec<GadgetContract>,
) -> Result<Vec<PrimitiveDischargeReport>> {
    let root = published_root();
    let mut discharged = Vec::with_capacity(contracts.len());

    for contract in contracts {
        let validation = validate_contract(contract.clone())?;
        let spec = contract.published_spec()?;
        match verify_published_spec(verifier, &root, &spec) {
            Ok(published_report) => discharged.push(PrimitiveDischargeReport {
                label: contract.label,
                published_spec_name: contract.published_spec_name,
                symbolic_relation_matches: validation.relation_matches,
                published_success: published_report.is_success(),
                size_matches: published_report.size_matches,
                alignment_matches: published_report.alignment_matches,
                allowed_assignments_hold: published_report.relation_report.allowed_assignments_hold,
                forbidden_assignments_hold: published_report
                    .relation_report
                    .forbidden_assignments_hold,
                charging_holds: published_report
                    .charging_reports
                    .iter()
                    .all(|report| report.all_outputs_are_named_states),
                error: None,
            }),
            Err(err) => discharged.push(PrimitiveDischargeReport {
                label: contract.label,
                published_spec_name: contract.published_spec_name,
                symbolic_relation_matches: validation.relation_matches,
                published_success: false,
                size_matches: false,
                alignment_matches: false,
                allowed_assignments_hold: false,
                forbidden_assignments_hold: false,
                charging_holds: false,
                error: Some(err.to_string()),
            }),
        }
    }

    Ok(discharged)
}

fn filter_contracts(contracts: Vec<GadgetContract>, filter: Option<&str>) -> Vec<GadgetContract> {
    match filter {
        None => contracts,
        Some(filter) => contracts
            .into_iter()
            .filter(|contract| {
                contract.label.contains(filter) || contract.published_spec_name.contains(filter)
            })
            .collect(),
    }
}

fn certify_source_endpoint(
    endpoint: Endpoint,
    variable_inputs: &BTreeMap<String, Endpoint>,
    contracts_by_instance: &HashMap<InstanceId, GadgetContract>,
) -> Result<()> {
    match endpoint {
        Endpoint::ExternalInput { variable } => {
            anyhow::ensure!(
                variable_inputs.contains_key(&variable) || variable.starts_with("const_"),
                "Undeclared external input source '{}'",
                variable
            );
        }
        Endpoint::InstancePort(port) => {
            let contract = contracts_by_instance.get(&port.instance).with_context(|| {
                format!("Missing contract for source instance {:?}", port.instance)
            })?;
            anyhow::ensure!(
                contract
                    .output_ports()
                    .any(|candidate| candidate == port.port),
                "Port {:?} is not an output port of contract '{}'",
                port,
                contract.label
            );
        }
    }
    Ok(())
}

fn certify_target_port(
    port: &PortRef,
    contracts_by_instance: &HashMap<InstanceId, GadgetContract>,
) -> Result<()> {
    let contract = contracts_by_instance
        .get(&port.instance)
        .with_context(|| format!("Missing contract for target instance {:?}", port.instance))?;
    anyhow::ensure!(
        contract
            .input_ports()
            .any(|candidate| candidate == port.port),
        "Port {:?} is not an input port of contract '{}'",
        port,
        contract.label
    );
    Ok(())
}

fn logical_contract_for_instance(instance: &MacroInstance) -> Result<GadgetContract> {
    match instance.kind {
        MacroKind::NotGate => Ok(GadgetContract {
            label: "logical_not",
            published_spec_name: "NOT gate tile",
            bindings: vec![
                ContractPortBinding {
                    macro_port: "in",
                    wire_port: "W",
                    role: ContractPortRole::Input,
                },
                ContractPortBinding {
                    macro_port: "out",
                    wire_port: "E",
                    role: ContractPortRole::Output,
                },
            ],
            semantics: ContractSemantics::Inversion {
                input: "in",
                output: "out",
            },
        }),
        MacroKind::OrGate => Ok(GadgetContract {
            label: "logical_or",
            published_spec_name: "OR gate tile",
            bindings: vec![
                ContractPortBinding {
                    macro_port: "lhs",
                    wire_port: "N",
                    role: ContractPortRole::Input,
                },
                ContractPortBinding {
                    macro_port: "rhs",
                    wire_port: "S",
                    role: ContractPortRole::Input,
                },
                ContractPortBinding {
                    macro_port: "out",
                    wire_port: "E",
                    role: ContractPortRole::Output,
                },
            ],
            semantics: ContractSemantics::Or {
                lhs: "lhs",
                rhs: "rhs",
                out: "out",
            },
        }),
        MacroKind::Splitter => Ok(GadgetContract {
            label: "logical_splitter",
            published_spec_name: "splitter tile",
            bindings: vec![
                ContractPortBinding {
                    macro_port: "in",
                    wire_port: "S",
                    role: ContractPortRole::Input,
                },
                ContractPortBinding {
                    macro_port: "out0",
                    wire_port: "E",
                    role: ContractPortRole::Output,
                },
                ContractPortBinding {
                    macro_port: "out1",
                    wire_port: "N",
                    role: ContractPortRole::Output,
                },
            ],
            semantics: ContractSemantics::Split {
                input: "in",
                out0: "out0",
                out1: "out1",
            },
        }),
        MacroKind::Enforcer => Ok(GadgetContract {
            label: "logical_enforcer",
            published_spec_name: "enforcer gadget",
            bindings: vec![ContractPortBinding {
                macro_port: "in",
                wire_port: "W",
                role: ContractPortRole::Input,
            }],
            semantics: ContractSemantics::RequireTrue { input: "in" },
        }),
        MacroKind::InputPort { .. } => anyhow::bail!(
            "Compiled construction should not materialize input ports as gadget instances"
        ),
    }
}

fn logical_gadget_contracts() -> Vec<GadgetContract> {
    [
        MacroKind::NotGate,
        MacroKind::OrGate,
        MacroKind::Splitter,
        MacroKind::Enforcer,
    ]
    .into_iter()
    .map(|kind| {
        logical_contract_for_instance(&MacroInstance {
            id: InstanceId(0),
            kind,
            column: 0,
            row: 0,
        })
        .expect("Built-in logical contract should be valid")
    })
    .collect()
}

fn routing_gadget_contracts() -> Vec<GadgetContract> {
    let mut contracts = vec![
        direct_identity_contract("route_horizontal_wire", "horizontal wire tile", "W", "E"),
        direct_identity_contract("route_vertical_wire", "vertical wire tile", "N", "S"),
        direct_identity_contract("route_ne_turn", "NE turn tile", "N", "E"),
        direct_identity_contract("route_nw_turn", "NW turn tile", "N", "W"),
        direct_identity_contract("route_sw_turn", "SW turn tile", "S", "W"),
        direct_identity_contract("route_se_turn", "SE turn tile", "S", "E"),
        GadgetContract {
            label: "route_crossing",
            published_spec_name: "crossing tile",
            bindings: vec![
                direct_binding("W", ContractPortRole::Input),
                direct_binding("E", ContractPortRole::Output),
                direct_binding("N", ContractPortRole::Input),
                direct_binding("S", ContractPortRole::Output),
            ],
            semantics: ContractSemantics::Crossing {
                horizontal_in: "W",
                horizontal_out: "E",
                vertical_in: "N",
                vertical_out: "S",
            },
        },
        GadgetContract {
            label: "route_always_one",
            published_spec_name: "always-1 tile",
            bindings: vec![direct_binding("W", ContractPortRole::Output)],
            semantics: ContractSemantics::RequireTrue { input: "W" },
        },
    ];
    contracts.extend(
        published_connector_specs()
            .into_iter()
            .map(|spec| direct_identity_contract(spec.name, spec.name, "W", "E")),
    );
    contracts
}

fn direct_binding(port_name: &'static str, role: ContractPortRole) -> ContractPortBinding {
    ContractPortBinding {
        macro_port: port_name,
        wire_port: port_name,
        role,
    }
}

fn direct_identity_contract(
    label: &'static str,
    published_spec_name: &'static str,
    input: &'static str,
    output: &'static str,
) -> GadgetContract {
    GadgetContract {
        label,
        published_spec_name,
        bindings: vec![
            direct_binding(input, ContractPortRole::Input),
            direct_binding(output, ContractPortRole::Output),
        ],
        semantics: ContractSemantics::Identity { input, output },
    }
}

fn routing_basis_coverage() -> RoutingBasisCoverage {
    let connector_phase_pairs = published_connector_specs()
        .into_iter()
        .filter_map(|spec| {
            let (east, _, west, _) = spec.align?;
            Some((west?, east?))
        })
        .collect::<BTreeSet<_>>();

    RoutingBasisCoverage {
        straight_wires_present: published_spec_named("horizontal wire tile").is_some()
            && published_spec_named("vertical wire tile").is_some(),
        turn_tiles_present: [
            "NE turn tile",
            "NW turn tile",
            "SW turn tile",
            "SE turn tile",
        ]
        .into_iter()
        .all(|name| published_spec_named(name).is_some()),
        crossing_present: published_spec_named("crossing tile").is_some(),
        always_one_present: published_spec_named("always-1 tile").is_some(),
        connector_phase_pairs,
    }
}

fn normalize_assignment(assignment: &BTreeMap<String, bool>) -> Vec<(String, bool)> {
    assignment
        .iter()
        .map(|(port, value)| (port.clone(), *value))
        .collect()
}

fn phase_for_wire(spec: &PublishedSpec, wire_port: &str) -> Option<i8> {
    let Some((east, north, west, south)) = spec.align else {
        return None;
    };
    match wire_port {
        "E" => east,
        "N" => north,
        "W" => west,
        "S" => south,
        _ => None,
    }
}

fn macro_kind_label(kind: &MacroKind) -> &'static str {
    match kind {
        MacroKind::InputPort { .. } => "InputPort",
        MacroKind::NotGate => "NotGate",
        MacroKind::OrGate => "OrGate",
        MacroKind::Splitter => "Splitter",
        MacroKind::Enforcer => "Enforcer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;
    use crate::interfaces::{
        horizontal_family_label, vertical_family_label, InterfaceBasisCertificate,
        InterfaceBasisEntry,
    };

    #[test]
    fn test_logical_contracts_match_published_relations() {
        let reports = validate_logical_contract_library().unwrap();
        assert!(!reports.is_empty());
        assert!(reports.iter().all(|report| report.relation_matches));
    }

    #[test]
    fn test_routing_contracts_match_published_relations() {
        let reports = validate_routing_contract_library().unwrap();
        assert!(!reports.is_empty());
        assert!(reports.iter().all(|report| report.relation_matches));
    }

    #[test]
    fn test_certify_compiled_construction_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let certificate = certify_compiled_construction(&construction).unwrap();

        assert_eq!(certificate.instances.len(), construction.instances.len());
        assert_eq!(certificate.nets.len(), construction.nets.len());
        assert_eq!(certificate.input_anchor_obligations.len(), 3);
        assert_eq!(certificate.internal_route_obligations.len(), 9);
        assert!(certificate.input_boundary_certificate.is_complete());
        assert!(certificate.dead_boundary_padding_certificate.is_complete());
        assert!(certificate
            .logical_contracts
            .iter()
            .all(|report| report.relation_matches));
        assert!(certificate
            .routing_contracts
            .iter()
            .all(|report| report.relation_matches));
        assert!(certificate.routing_basis_coverage.is_rectilinear_complete());
        assert_eq!(
            certificate.routing_witness.net_paths.len(),
            construction.nets.len()
        );
        assert!(!certificate.routing_witness.horizontal_families.is_empty());
        assert!(!certificate.routing_witness.vertical_families.is_empty());
        assert!(!certificate.interface_basis_complete);
        assert!(certificate
            .render_summary()
            .contains("symbolic_certificate_valid=true"));
    }

    #[test]
    fn test_attach_interface_basis_certificate_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let certificate = certify_compiled_construction(&construction).unwrap();
        let router_basis = enumerate_router_interface_basis().unwrap();
        let basis = InterfaceBasisCertificate {
            horizontal: router_basis
                .horizontal
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: crate::interfaces::InterfaceOrientation::Horizontal,
                    label: horizontal_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
            vertical: router_basis
                .vertical
                .iter()
                .map(|family| InterfaceBasisEntry {
                    orientation: crate::interfaces::InterfaceOrientation::Vertical,
                    label: vertical_family_label(family),
                    witness_count: family.count,
                    external_ports: Vec::new(),
                    expected_allowed_rows: 0,
                    candidate_count: 1,
                })
                .collect(),
        };
        let certificate = certificate
            .with_interface_basis_certificate(&basis)
            .unwrap();
        assert!(certificate.interface_basis_complete);
        assert_eq!(certificate.remaining_obligations.len(), 0);
    }

    #[test]
    #[ignore = "SAT-backed contract discharge is available but intentionally slow"]
    fn test_discharge_logical_contracts_when_sources_exist() {
        if !published_root().exists() {
            return;
        }

        let verifier = default_contract_verifier(None);
        let reports = discharge_logical_contract_library(&verifier).unwrap();
        assert!(!reports.is_empty());
        assert!(reports.iter().all(PrimitiveDischargeReport::is_success));
    }
}
