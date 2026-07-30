# ADR 0008: Declarative DSP Generation Boundary

Status: Accepted
Date: 2026-07-28

The First Public Release will let Authoring Agents create brief-derived pitched polyphonic instruments by composing validated native nodes in the declarative DSP graph, not by generating and compiling arbitrary Rust. Graph validation gives Crump control over types, units, topology, resource bounds, and deterministic safety contracts while still producing source-backed instruments that require no samples. A Rust extension mechanism is deferred until a separate post-release decision resolves code trust, isolation, toolchains, compatibility, failure containment, and reproducibility.
