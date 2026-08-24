# ADR 0003: Composition and Timeline Semantics

Status: Accepted
Date: 2026-07-23

## Context

Crump needs reusable code-defined material, linear arrangement, deterministic evaluation, source-backed identity, tempo and meter changes, and one musical model shared by playback and rendering. Conventional DAW distinctions such as MIDI versus audio tracks, clip-owned material, and implicit tick resolution would create special cases or precision limits that conflict with that model.

## Decision

- A project owns exactly one finite song. Authored time is exact rational quarter-note time, interpreted by explicit tempo and meter maps beginning at zero.
- A pattern is finite, timeline-independent reusable material that produces typed events. A clip is a durably identified placement of a pattern on a durably identified, device-defined track.
- Pattern composition and transformations are pure and have explicit span semantics. Clip evaluation transforms the finite pattern before applying source offset, once-or-loop traversal, cropping, and timeline placement.
- Overlapping clips merge additively. Pattern boundaries schedule event onsets; clip boundaries gate active note-like events while allowing downstream release and effect tails.
- Random choices derive deterministically from project, pattern, clip, loop-iteration, and source-local identity, with explicit source overrides available.
- Track order is presentational and cannot affect sound. Routing and processing dependencies must be explicit.

## Consequences

The DSL, compiled project, runtime transition, routing, rendering, and structured-editing designs must preserve these identities, interval rules, exact authored times, and deterministic scopes. Syntax, runtime sample rounding, routing topology, device behavior, and render-tail policy remain separate decisions.

## Alternatives Considered

Fixed track kinds, clip-owned events, integer PPQ time, placement-dependent pattern meter, implicit looping, overlap precedence, and timeline-position-seeded randomness were rejected because they introduce hidden behavior, precision ceilings, or unstable edits.
