# Current State

This document tracks the proof-facing status of the workspace as of April 2026.

## Summary

The `crates/rev_gol_proof` crate now has a proof pipeline for the `SAT -> Rev-GOL` direction that can reduce its internal proof obligations to zero for the current small example when supplied with the discharged router interface basis certificate.

The proof-critical path is no longer the experimental board stamper in `board.rs`. The current theorem-facing path is:

1. compile DIMACS / circuit input into a macro construction,
2. certify the construction against symbolic contracts for the published gadget basis,
3. build an explicit rectilinear routing witness,
4. certify external-input boundary encodings,
5. discharge a finite router interface basis and save it as a reusable certificate,
6. certify a finite dead-boundary padding bound.

When the saved interface basis is loaded back into the construction certificate, `compile_dimacs` now reports `remaining_obligations=0`.

## What The Code Now Establishes

For the current proof model, the code now has explicit certificates for:

- logical macro compilation over the published gadget basis,
- truth-table preservation on small examples via exhaustive checking,
- finite splitter-expanded netlists with bounded fanout,
- constructive polynomial-time routing witnesses,
- external input realization through binary boundary source strips,
- a finite router interface basis with SAT-backed local-family discharge,
- a conservative finite dead-boundary padding construction.

The key proof-facing modules are:

- `compiler.rs`: macro construction
- `contracts.rs`: symbolic construction certificate
- `routing.rs`: explicit routing witness
- `inputs.rs`: external-input boundary certificate
- `interfaces.rs`: local interface discharge and reusable interface basis
- `padding.rs`: finite dead-boundary padding certificate

## Reproducible Commands

### Fast proof sanity check

```bash
target/release/examples/compile_dimacs \
  --input crates/rev_gol_proof/tests/fixtures/small_example.cnf \
  --check-exhaustive
```

This checks the small reduction exhaustively but does not discharge the expensive SAT-backed interface basis.

### Build the reusable router interface basis certificate

This is expensive and may take several hours.

```bash
target/release/examples/compile_dimacs \
  --input crates/rev_gol_proof/tests/fixtures/small_example.cnf \
  --discharge-router-interface-basis \
  --interface-max-candidates 1 \
  --save-interface-basis crates/rev_gol_proof/tests/fixtures/router_interface_basis.json
```

### Load the interface basis certificate and inspect the final proof certificate

```bash
target/release/examples/compile_dimacs \
  --input crates/rev_gol_proof/tests/fixtures/small_example.cnf \
  --load-interface-basis crates/rev_gol_proof/tests/fixtures/router_interface_basis.json
```

At the current state this reports:

- `interface_basis_complete=true`
- `finite_dead_boundary_complete=true`
- `remaining_obligations=0`

## Current Limitations

The current proof machinery is strongest as a code-checked certificate pipeline. The remaining gaps are mostly mathematical packaging and scope alignment rather than new search or routing work.

What is still not done:

- write the theorem and lemma chain in paper/proof form rather than only in executable certificates,
- state the polynomial bounds cleanly in the writeup,
- align the implementation statement exactly with `wire-SAT`:
  either add a direct wire-SAT front-end or write the lemma reducing wire-SAT to the current CNF / circuit IR,
- write the NP-membership argument for the finite-board predecessor problem in the same proof document,
- decide whether to keep the current conservative finite-board bound or tighten it.

## Not Required For The Proof

The following are optional engineering follow-ups, not proof blockers:

- tightening the finite dead-boundary size bound,
- generating a compact explicit stamped board witness from the experimental board pipeline,
- speeding up the SAT-backed basis discharge,
- expanding the example set beyond `small_example.cnf`.

## Board Emitter Status

`board.rs` remains in the repository, but it is no longer the proof-critical path.

Current status:

- motif auditing is useful,
- the deterministic routing / certificate path is the authoritative proof path,
- exact published-board assembly is still experimental and can fail independently of the proof certificate.

Use it as a witness-generation experiment, not as the theorem engine.
