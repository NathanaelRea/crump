# Freeze the Compiled-Project and Runtime-Transition Contract

Type: grilling
Mode: HITL
Status: open
Blocked by: [02 - Prove Safe Text-to-Audio Graph Replacement](02-prove-safe-graph-replacement.md), [07 - Define the Routing, Modulation, and Parameter Semantic Model](07-define-routing-model.md), [10 - Freeze the DSL and Project-Source Contract](10-freeze-dsl-contract.md)

## Question

What immutable or versioned representations cross the parser, validator, compiler, control thread, audio thread, and renderer, and how do they implement bounded publication, retirement, and ADR 0011's Affected Instrument Restart without attempting exact topology-state migration?

## Research Baseline

Use the accepted graph-replacement prototype as mechanism evidence and ADR 0011 as the observable contract. Preserve transport and unchanged Tracks; permit a visible protective fade and reset of changed-instrument voices and effect state. Defer edit-specific migration, full convergence-point latency compensation, and professional routing transitions.
