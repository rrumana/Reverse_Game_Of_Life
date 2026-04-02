# Gadget Catalog

This repository now separates the proof obligations into two directions:

- `Rev-GOL -> SAT`: implemented in the solver and proof-safe when `symmetry_breaking = false`
- `SAT -> Rev-GOL`: implemented as a certificate-driven construction stack in `crates/rev_gol_proof`, with the experimental board emitter kept separate from the proof-critical path

## Implemented `Rev-GOL -> SAT` gadgets

- `final_state_pinning`: one unit clause per final-layer cell
- `local_transition_truth_table`: one local clause family per cell-time pair, enumerating all predecessor neighborhoods
- `boundary_wiring`: dead / wrap / mirror neighborhood interpretation

## Not proof-safe

- `symmetry_breaking_heuristics`: search heuristics only; exclude from completeness proofs

## Implemented `SAT -> Rev-GOL` proof basis

The current proof-side implementation has explicit contracts or certificates for:

- `bit_wire`: realized by the published wire / connector basis and checked through the router interface basis
- `wire_turn`: realized by the published turn tiles and checked through the router interface basis
- `wire_splitter`: realized by the published splitter tile
- `wire_crossover`: realized by the published crossing tile
- `inverter`: realized by the published NOT tile
- `input_anchor`: realized by the external-input boundary certificate
- `output_constraint`: realized by the published enforcer gadget
- `moat_padding`: realized by the finite dead-boundary padding certificate

The current logical basis is:

- `NOT gate tile`
- `OR gate tile`
- `splitter tile`
- `enforcer gadget`

The current routing basis is:

- `horizontal wire tile`
- `vertical wire tile`
- `NE turn tile`
- `NW turn tile`
- `SW turn tile`
- `SE turn tile`
- `crossing tile`
- `always-1 tile`
- `connector -1 to -1`
- `connector 0 to -1`
- `connector 1 to -1`
- `connector -1 to 0`
- `connector 0 to 0`
- `connector 1 to 0`
- `connector -1 to 1`
- `connector 0 to 1`
- `connector 1 to 1`

## Still left to do

- write the theorem and lemmas in mathematical proof form,
- state the polynomial bounds explicitly in the writeup,
- align the theorem statement exactly with `wire-SAT` rather than only the current CNF / circuit front-end,
- include the NP-membership argument for the finite-board predecessor problem in the same writeup,
- optionally tighten the conservative finite-board size bound.

The corresponding structured catalog lives in `crates/rev_gol_proof/src/gadgets.rs`, so the program can refer to the same names and proof obligations that the paper uses.
