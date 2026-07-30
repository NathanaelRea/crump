# Define the Composition and Timeline Semantic Model

Type: grilling
Mode: HITL
Status: resolved
Blocked by: [01 - Freeze the First-Release Reference-Project Acceptance Contract](01-freeze-reference-project-contract.md)

## Question

What are the canonical identities, relationships, invariants, and semantics of projects, songs, tracks, clips, patterns, events, tempo, meter, repetition, transformations, and timeline placement required by the reference project?

## Answer

The accepted composition and timeline model is:

- A Project owns exactly one Song. Song time starts at zero and has an explicit positive arrangement end; Clips cannot start before zero or extend beyond it. Render-tail policy remains a downstream decision.
- Authored Musical Time is exact rational quarter-note time. Bar-and-beat notation resolves through an explicit Meter Map, while an explicit Tempo Map converts musical time to elapsed runtime under downstream deterministic sample-rounding rules.
- Tempo and Meter Maps both require a marker at zero, strictly increasing unique marker positions, and no hidden defaults. Tempo supports positive BPM step and linear segments. Meter supports positive numerators and power-of-two denominators; a mid-bar change ends a legal fragmentary bar and starts the next numbered bar at beat one.
- A Track is a device-defined timeline lane, not a fixed MIDI, audio, or instrument kind. Track order is presentational only. Overlapping Clips contribute typed Events additively and merge deterministically; device or routing semantics, not timeline precedence, determine processing.
- A Pattern is reusable, timeline-independent material with a positive finite span. It produces an ordered multiset of typed Events whose onsets lie in the half-open Pattern span. Event durations may cross that span. Chords are simultaneous note events, rests are gaps, one-shot triggers may be instantaneous, and note-like events have positive duration.
- Pattern bar notation requires an explicit pattern-local meter and never inherits meter from a Clip placement. Pure composition provides sequence with summed span, parallel with maximum span, and positive integer repetition with multiplied span.
- The required pure transformation floor is rational shift, positive rational time scale, reversal, cyclic rotation, pitch transposition, velocity or value scaling, event filtering, and deterministic probability gating. Transformations are type-checked, applied left to right, and explicit about event and span mapping.
- A Clip references a Pattern and owns a timeline start, positive duration, source offset, placement transformations, and explicit `once` or `loop` mode. Evaluation transforms the finite Pattern first, then applies source offset, traversal, half-open cropping, and Song placement. Loop boundaries do not truncate prior-cycle events; the final Clip boundary gates active note-like events while downstream instrument release and effect tails may continue. A source offset does not chase earlier events.
- Tracks and Clips carry project-unique Durable Domain IDs. Rename, movement, reorder, reload, restart, and cut-and-reinsert preserve them; structured duplication generates fresh IDs and rewrites copied internal references. Missing or duplicate IDs invalidate the candidate revision rather than being silently repaired. Project and Song need no IDs, Patterns use semantic names, and Events use revision-local source identity.
- Random choices derive by default from the Project seed, Pattern semantic path, Clip ID, loop iteration, and a stable source-local choice key. Separate Clips and iterations vary, while Clip movement or rename does not reroll choices; source may explicitly override seed or scope.

Routing topology, parameter and modulation semantics, DSL syntax and type-system depth, sample and device behavior, runtime sample rounding, and render-tail capture remain owned by downstream decisions.

Decision record: [ADR 0003: Composition and Timeline Semantics](../../../docs/adr/0003-composition-and-timeline-semantics.md)

## Comments

- ADR 0003 now distinguishes accepted destination semantics from the binding First Public Release subset. The Musical Sketch requires one finite Song, exact Musical Time, one fixed tempo and meter declared at zero, concrete typed Events in finite Patterns, Clips with explicit traversal, and deterministic playback/reopen/render. Tempo or meter changes, advanced Pattern composition/transforms, probability, and full random scoping are not first-release obligations.
