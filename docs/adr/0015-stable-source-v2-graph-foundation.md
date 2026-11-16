# ADR 0015: Stable Source-v2 Graph Foundation

Status: Accepted
Date: 2026-08-24

## Context

The graph-authored synthesis model needs durable Bus identity, exact setting sites, explicit channel conversion, and bounded typed summing before deeper source or shared-runtime capabilities can be added. Those additions must not change version-1 parsing or audible behavior, and a version bump must not make a mechanically upgraded Project lose the version-1 language floor.

## Decision

- `crump 2` and `crump-module 2` are stable headers and must be the first non-comment declaration. Candidate spellings such as `2-alpha` remain invalid.
- Version 1 is unchanged. A version-1 Project imports version-1 modules only; a version-2 Project may import version-1 or version-2 modules.
- Version 2 accepts the complete version-1 floor, including `graph N` and `connect FROM -> TO`. Legacy connections receive deterministic compatibility IDs in an internal namespace disjoint from valid authored IDs. The durable version-2 authored form is `bus ID FROM -> TO`.
- Runnable Node settings are retained in source order with exact key and value sites instead of being collapsed into a map. Version-1 settings remain inline-only. Reserved future Nodes retain repeated, multi-token, and recursively nested directives as an ordered exact-token/source-site tree while remaining unsupported.
- `mono_to_stereo`, `stereo_to_mono`, `stereo_mixer`, and `stereo_sum` are explicit typed Nodes. Stereo sum has a declared fan-in ceiling no greater than 64 and folds sources in durable Bus-ID order.
- Version 2 can represent explicit voice/shared graph domains and structured future source-Node settings, but shared graphs and source families without complete runtimes fail validation and cannot publish.
- Validation and defensive compilation enforce the same schemas, bounds, topology, reachability, and Output contract. Admission includes complete two-voice slot containers and both resident publication runtimes.
- Compiler-effective graph facts are canonicalized for compatibility, and inspection shows topological Node order plus Bus-ID convergence order; declaration-only reordering does not restart compatible state.

## Consequences

Existing version-1 source and deterministic vectors remain compatible. Version-2 authors can use stable Bus identities and explicit stereo topology without hidden coercion or order-dependent fan-in. A header-only upgrade from version 1 to version 2 remains valid. Instrument-owned Modulation was subsequently accepted by [ADR 0016](0016-instrument-owned-modulation.md), shared processing by [ADR 0017](0017-shared-post-voice-graph.md), and the first exact-asset source family (`wavetable`) by [ADR 0019](0019-exact-asset-wavetable-runtime.md). Remaining unfinished source families stay fail-closed rather than acquiring placeholder behavior.

## Rejected alternatives

- Requiring every version-2 graph to replace legacy `connect` declarations would violate the complete-version-1-floor rule and make header-only upgrades fail.
- Implicit channel coercion or input overwrite would hide gain, order, and resource behavior.
- Accepting shared or future source declarations as no-ops would create source that appears valid without defined audio behavior.
