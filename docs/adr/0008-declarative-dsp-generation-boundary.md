# ADR 0008: Declarative DSP Generation Boundary

Status: Accepted
Date: 2026-07-28

Authoring Agents create pitched polyphonic Instruments by composing validated native Nodes in the declarative DSP graph, not by generating and compiling arbitrary Rust. Graph validation owns types, units, topology, resource bounds, and deterministic behavior while preserving readable source-backed Instruments.

Crump does not load user Rust extensions, compile Agent-generated native code, or expose arbitrary callback scripting.
