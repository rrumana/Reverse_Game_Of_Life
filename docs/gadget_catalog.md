# Gadget Catalog

This repository now separates the proof obligations into two directions:

- `Rev-GOL -> SAT`: implemented in the solver and proof-safe when `symmetry_breaking = false`
- `SAT -> Rev-GOL`: not implemented as a compiler yet; this file names the gadget basis still required for an NP-hardness proof

## Implemented `Rev-GOL -> SAT` gadgets

- `final_state_pinning`: one unit clause per final-layer cell
- `local_transition_truth_table`: one local clause family per cell-time pair, enumerating all predecessor neighborhoods
- `boundary_wiring`: dead / wrap / mirror neighborhood interpretation

## Not proof-safe

- `symmetry_breaking_heuristics`: search heuristics only; exclude from completeness proofs

## Required `SAT -> Rev-GOL` gadget basis

- `bit_wire`
- `wire_turn`
- `wire_splitter`
- `wire_crossover`
- `inverter`
- `universal_gate`
- `input_anchor`
- `output_constraint`
- `moat_padding`

The corresponding structured catalog lives in `src/proof/gadgets.rs`, so the program can refer to the same names and proof obligations that the paper uses.
