# Define the Routing, Modulation, and Parameter Semantic Model

Type: grilling
Mode: HITL
Status: resolved
Blocked by: [02 - Prove Safe Text-to-Audio Graph Replacement](02-prove-safe-graph-replacement.md), [06 - Define the Composition and Timeline Semantic Model](06-define-composition-model.md)

## Question

What canonical model represents audio and control nodes, buses, groups, sends, returns, sidechains, parameters, units, interpolation, modulation, automation, latency, and stable identity across edits?

## Answer

The accepted routing, modulation, and parameter model is:

- The compiled project is an acyclic typed graph. Processing Nodes expose named Event, channel-layout-aware Audio, and unit-typed Control Ports in a shared sample-time domain; Ports explicitly declare cardinality, incompatible layouts or units require converter nodes, and sidechains are ordinary named auxiliary Audio inputs. Explicit bounded stateful devices contain intentional feedback.
- Reusable DSP graphs retain hierarchical typed interfaces and identity. Authored Tracks, Groups, Returns, device instances, Sends, and graph nodes carry scoped Durable Domain IDs; nested runtime identity composes instance paths with definition-local node IDs. Parameters use owner identity plus immutable owner-defined keys.
- Track, Group, Return, and singleton Master are distinct roles; Bus means their typed connection, not another channel kind. Group membership guarantees main-output summing through the parent. Top-level channels default to Master, while Returns may feed a Group or later Return if acyclic. Timeline Tracks cannot become ad hoc main-output buses.
- Each Send is a gain-controlled branch to one Return with its own explicit pre-fader or post-fader tap. Tracks, Groups, and Returns share a fixed strip order: input sum, ordered inserts, pre-fader tap, gain and explicit panner mode, mute, post-fader tap, main output. Audio layout conversion is never implicit.
- Parameters use continuous real, bounded integer, boolean, or enum values with declared domains, defaults, physical units, display mappings, and capabilities. Authored invalid values reject the Candidate Revision; runtime modulation overshoot clamps, while non-finite Control holds the last finite result and reports a diagnostic. Device-owned deterministic smoothing is allowed, but the host adds none.
- Effective Parameter values evaluate as base, one optional Song-scoped absolute Automation function, additive normalized Modulation routes, then domain constraint. Point Automation starts at zero, uses unique increasing Musical Time positions, holds the final value, and explicitly selects `step`, `linear`, or `power(p)` interpolation in canonical units. Reusable control material combines through Modulation instead of competing clip Automation.
- Continuous Automation and Modulation are sample-accurate and block-size-independent. Every node output declares sample latency, and full-graph compensation aligns Event, Audio, Control, parallel, Send, Return, and sidechain paths to common source time in playback and rendering.
- Mute is durable step-automatable project state; Solo, cue, and monitoring are transient Audition State. The hidden post-Master safety stage protects real-time monitoring only; renders bypass it and require explicit authored limiting.

Decision record: [ADR 0004: Routing, Modulation, and Parameter Semantics](../../../docs/adr/0004-routing-modulation-and-parameter-semantics.md)

## Comments

- ADR 0004 now distinguishes accepted destination semantics from the binding First Public Release subset. The Musical Sketch requires hierarchical typed Event/Audio/Control graph structure, durable node identity, physical-unit Parameters, instrument envelopes/modulation, minimal creative effects, simple Track balance into Master, and protected monitoring. Groups, Sends, Returns, general mixer Buses, Song Automation, sidechains, compressors, detailed EQ/mastering, arbitrary parallel routing, and full latency compensation are not first-release obligations.
