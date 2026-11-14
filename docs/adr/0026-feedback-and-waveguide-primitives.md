# ADR 0026: Feedback and Waveguide Primitives

Status: Accepted
Date: 2026-11-14

Amends [ADR 0008](0008-declarative-dsp-generation-boundary.md), [ADR 0014](0014-graph-authored-native-synthesis.md), the graph-Control source restriction of [ADR 0016](0016-instrument-owned-modulation.md) and the acyclic-schedule rule of [ADR 0017](0017-shared-post-voice-graph.md).

## Context

Strings, reeds, bores and bows are delay loops: a wave travels along a delay, loses energy in a filter and returns through a nonlinearity. Composable DSP Graphs were acyclic, the only delay was a fixed-length echo with internal feedback, Control could not drive Audio-rate Ports, and there was no general arithmetic or waveshaping. Instruments of these families were therefore rendered offline into sample banks by Python generators, outside the graph and outside the user's reach.

Every modular or physical-modelling environment exposes the same small set of capabilities instead: a delay that may close a loop, sample arithmetic, waveshaping and explicit signal conversion. These are generic operations with closed, validated behaviour, so they fit the declarative boundary.

## Decision

- **Feedback through delay.** A graph may contain a cycle only if it passes through the input of a Node whose output depends solely on earlier frames. `delay_line` is the only such Node. Each frame, every other Node runs in topological order with Buses into `delay_line` removed from the ordering. Every `delay_line` then reads its inputs from that frame, and all of them publish their outputs for the next frame together. A loop therefore has exactly its authored delay. A cycle without a `delay_line` is rejected with CRV048, which names the cycle. A feedback Bus is an ordinary typed Bus.
- **The existing `delay` does not break cycles.** Its output includes its dry input from the same frame. It keeps its echo semantics.
- **`delay_line`** is a mono fractional delay with no internal feedback. In `time` mode it delays by `time`. In `pitch` mode it delays by `ratio / pitch`, following its `pitch` input. `time_mod` scales either delay by `1 + mod_depth × time_mod`. `tune_compensation` subtracts a loop filter's delay in frames. Interpolation is `linear`, `lagrange3` or first-order `allpass`. Each has a minimum delay that never reads the frame being written. `max_time` is a literal size, never a Parameter reference, and sizes a preallocated buffer that enters resource admission. The delay clamps to it, so a note below the buffer's range plays sharp rather than reading out of bounds. Buffers reset at voice start.
- **Signal primitives.** `math` and `stereo_math` provide add, subtract, multiply, min, max, abs, negate, offset and scale. `shaper` provides tanh, hard and soft clip, one-sided, asymmetric and piecewise-linear table curves with drive and bias. Table points are authored inline, and their inputs must strictly increase. `to_audio` and `to_control` are the explicit conversions between Control and Audio, so CRV044 still rejects implicit ones. A `to_control.control` output is also a bipolar per-voice graph-Port Modulation source, with the same-frame scheduling and delay-loop rules used by other Control sources. `dc_block` and `linear_phase_lowpass` are loop-safe filters. The latter delays every frequency by exactly one frame, so a loop containing it tunes exactly.
- **Feedback guard.** The graph runtime watches every voice and shared runtime that contains a `delay_line`. If any Node output in the graph is non-finite, or a loop or output sample exceeds +60 dBFS or stays above +24 dBFS for 50 ms, the guard mutes the voice and resets its state. A new note clears the loud-frame timer, and every produced signal is checked before delay-input sanitization can hide a failure. The count is reported in Runtime telemetry, the render manifest's `feedback_mutes` and the CLI render summary. Graphs without a `delay_line` behave as before. Validation bounds ranges, memory and work but does not claim a loop is stable.
- **Both graph domains.** Voice and shared graphs share one runtime, so `delay_line` and feedback work in both. Shared-graph chorus, flanger and comb effects need no separate mechanism.
- **Ladder resonance.** `ladder_drive` feedback is normalised by the drive stage's small-signal gain. The loop gain is therefore `4 × resonance` at every drive, and the four oversampled stages need at least 4 to oscillate at any cutoff. Resonance below 1 always decays. Before this change, drive above about 0.1 with moderate resonance self-oscillated indefinitely. Under [ADR 0023](0023-native-synthesis-compatibility-boundary.md), sound compatibility is experimental, so the model keeps its name and existing patches are retuned rather than versioned. Every affected expansion patch used drive 0.

## Consequences

- Plucked, bridged and blown instruments are authored as ordinary Instrument Modules. The [waveguide examples](../../examples/waveguides/README.md) include a plucked string, a sitar and a clarinet, each tuned within a fraction of a cent across its range.
- Routing between Tracks, Groups, Returns and the Master stays acyclic under [ADR 0004](0004-routing-modulation-and-parameter-semantics.md). Feedback exists only inside an Instrument's graphs.
- Graph topology now orders a frame in two phases. Graph-port Modulation routes still run in the first phase, before every `delay_line` reads Parameters.
- A `delay_line` costs four work units and `max_time × sample rate` f32 frames per voice. The runtime also admits one published-output slot per `delay_line`.
- `exciter` and `modal_bank` remain the higher-level conveniences for struck bodies. Instrument families can move from offline recipes to native graphs once the native version is at least as good. That migration is separate work.

## Rejected alternatives

- **Per-family Nodes** (`string`, `reed`, `bow`). Each would hide its loop behind a fixed parameter set and repeat the problem the offline generators have.
- **Arbitrary feedback with an implicit one-frame delay on every back edge.** The delay would depend on Node ordering rather than authored topology, so loops could not be tuned exactly.
- **Making `delay` a feedback breaker.** Its dry path is a zero-latency route through the Node, and changing that would alter existing echoes.
- **Proving stability during validation.** A loop with nonlinear, Parameter-driven gain cannot be proven stable statically. Bounding damage at runtime and reporting it is honest and cheap.
