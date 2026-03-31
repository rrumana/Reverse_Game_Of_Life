//! Gadget catalogs for the two proof directions around reverse Game of Life.

/// Which reduction direction a gadget belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GadgetDirection {
    RevGolToSat,
    SatToRevGol,
}

/// Proof status of a gadget entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GadgetStatus {
    Implemented,
    RequiredButMissing,
    UnsafeHeuristic,
}

/// A proof-facing gadget description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GadgetSpec {
    pub id: &'static str,
    pub direction: GadgetDirection,
    pub status: GadgetStatus,
    pub purpose: &'static str,
    pub construction: &'static str,
    pub proof_obligations: &'static [&'static str],
}

impl GadgetSpec {
    pub const fn new(
        id: &'static str,
        direction: GadgetDirection,
        status: GadgetStatus,
        purpose: &'static str,
        construction: &'static str,
        proof_obligations: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            direction,
            status,
            purpose,
            construction,
            proof_obligations,
        }
    }
}

/// The gadgets already present in the current proof-safe `Rev-GOL -> SAT` encoding.
pub fn rev_gol_to_sat_gadgets() -> Vec<GadgetSpec> {
    vec![
        GadgetSpec::new(
            "final_state_pinning",
            GadgetDirection::RevGolToSat,
            GadgetStatus::Implemented,
            "Fix the final time slice to the target instance.",
            "One unit clause per cell in the final layer.",
            &[
                "A satisfying assignment matches the target exactly at the final time step.",
                "Any valid reverse-Life witness satisfies all unit clauses.",
            ],
        ),
        GadgetSpec::new(
            "local_transition_truth_table",
            GadgetDirection::RevGolToSat,
            GadgetStatus::Implemented,
            "Enforce the Life update rule for one cell over one time step.",
            "For each current-cell value and each exact neighbor pattern, emit one implication clause fixing the next-state bit.",
            &[
                "Every satisfying assignment yields a valid Life transition cellwise.",
                "Every valid Life transition satisfies every generated clause.",
                "The clause count is constant per cell-time pair.",
            ],
        ),
        GadgetSpec::new(
            "boundary_wiring",
            GadgetDirection::RevGolToSat,
            GadgetStatus::Implemented,
            "Interpret the neighborhood under dead, wrap, or mirror boundaries.",
            "Out-of-bounds neighbors are either omitted, wrapped, or mirrored before connecting to cell variables.",
            &[
                "The chosen boundary condition matches the forward simulator exactly.",
                "Repeated wrapped or mirrored neighbors are treated as a multiset of neighbor slots.",
            ],
        ),
        GadgetSpec::new(
            "symmetry_breaking_heuristics",
            GadgetDirection::RevGolToSat,
            GadgetStatus::UnsafeHeuristic,
            "Prune the search space with ad hoc dominance constraints.",
            "Pairwise implications over sampled cells and geometric transforms.",
            &[
                "These constraints are not part of the proof-safe core encoding.",
                "They must be excluded from completeness claims unless separately justified.",
            ],
        ),
    ]
}

/// The gadget basis still needed for a full `SAT -> Rev-GOL` NP-hardness proof.
pub fn sat_to_rev_gol_required_gadgets() -> Vec<GadgetSpec> {
    vec![
        GadgetSpec::new(
            "bit_wire",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Transmit a Boolean signal across space while preserving phase/timing.",
            "A fixed rectangular target pattern with one input port and one output port.",
            &[
                "Exactly two interface behaviors encode false/true.",
                "The output port realizes the same bit as the input port after the gadget delay.",
                "Internal cells do not leak unintended signals outside the gadget footprint.",
            ],
        ),
        GadgetSpec::new(
            "wire_turn",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Route a wire around the layout without changing the represented bit.",
            "A constant-size elbow gadget compatible with the wire protocol.",
            &[
                "Truth values are preserved across the bend.",
                "The phase delay matches the routing model used by the compiler.",
            ],
        ),
        GadgetSpec::new(
            "wire_splitter",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Fan one signal out to multiple downstream gadgets.",
            "A one-input, two-output gadget using the same port protocol as the wire.",
            &[
                "Both outputs equal the input bit.",
                "No additional valid predecessor behaviors appear on the outputs.",
            ],
        ),
        GadgetSpec::new(
            "wire_crossover",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Allow two wires to cross without interacting.",
            "A constant-size gadget with two independent channels.",
            &[
                "Each output depends only on its corresponding input channel.",
                "The gadget composes without crosstalk in the surrounding zero background.",
            ],
        ),
        GadgetSpec::new(
            "inverter",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Implement logical negation.",
            "A one-input, one-output gadget compatible with the wire protocol.",
            &[
                "True maps to false and false maps to true.",
                "Delay and phase are explicit so the gadget can be composed in circuits.",
            ],
        ),
        GadgetSpec::new(
            "universal_gate",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Implement a functionally complete Boolean gate such as NAND or NOR.",
            "A constant-size gadget with two inputs and one output.",
            &[
                "The output realizes the target Boolean function of the inputs.",
                "Every allowed port assignment has at least one preimage inside the gadget.",
                "Every forbidden port assignment is UNSAT inside the gadget footprint.",
            ],
        ),
        GadgetSpec::new(
            "input_anchor",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Introduce unconstrained primary inputs corresponding to SAT variables.",
            "A boundary or source gadget whose predecessor choices encode true/false.",
            &[
                "Each SAT variable has exactly two legal predecessor encodings.",
                "The chosen encoding can be routed into the circuit without ambiguity.",
            ],
        ),
        GadgetSpec::new(
            "output_constraint",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Force the compiled circuit output to be true in the target instance.",
            "A sink gadget or terminal condition attached to the circuit output wire.",
            &[
                "A predecessor exists iff the output bit is true.",
                "The sink does not introduce spurious predecessors unrelated to the circuit value.",
            ],
        ),
        GadgetSpec::new(
            "moat_padding",
            GadgetDirection::SatToRevGol,
            GadgetStatus::RequiredButMissing,
            "Isolate gadgets on a finite board and prevent unwanted edge interactions.",
            "A fixed all-dead collar or explicit buffer region around the compiled layout.",
            &[
                "Signals do not interact across gadget boundaries except at declared ports.",
                "The finite-board construction simulates the intended infinite or padded environment.",
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rev_gol_catalog_marks_core_gadgets_as_implemented() {
        let gadgets = rev_gol_to_sat_gadgets();
        assert!(gadgets
            .iter()
            .any(|g| g.id == "local_transition_truth_table"));
        assert!(gadgets
            .iter()
            .any(|g| g.status == GadgetStatus::UnsafeHeuristic));
    }

    #[test]
    fn test_sat_to_rev_gol_catalog_lists_required_basis() {
        let gadgets = sat_to_rev_gol_required_gadgets();
        assert!(gadgets.iter().any(|g| g.id == "bit_wire"));
        assert!(gadgets
            .iter()
            .all(|g| g.status == GadgetStatus::RequiredButMissing));
    }
}
