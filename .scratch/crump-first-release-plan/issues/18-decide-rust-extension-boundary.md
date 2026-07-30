# Decide the First-Release Rust Extension Boundary

Type: grilling
Mode: HITL
Status: resolved
Blocked by: [10 - Freeze the DSL and Project-Source Contract](10-freeze-dsl-contract.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md), [14 - Define Project Persistence, Asset, and Compatibility Semantics](14-define-persistence-semantics.md)

## Question

Must the first public release expose Rust DSP extensions, and if so, what minimum API or ABI, versioning, trust, loading, failure, and reproducibility contract is supportable?

## Research Baseline

Before deciding, research official professional DAW extension policies and primary plugin/API specifications, including CLAP, with attention to versioning, thread roles, state, latency, isolation, and failure. Use those obligations as the baseline. Prefer no public first-release extension boundary unless the reference workflow proves it necessary and Crump can support the full professional contract rather than a deceptively simple unsafe API.

## Answer

Deferred by ADR 0008. First-release Authoring Agents construct instruments only from validated native nodes in the declarative Composable DSP Graph. A Rust extension mechanism requires a separate post-release decision covering trust, isolation, real-time safety, toolchains, compatibility, failure containment, and reproducibility.
