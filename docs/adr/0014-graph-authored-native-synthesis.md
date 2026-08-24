# ADR 0014: Graph-Authored Native Synthesis

Status: Accepted
Date: 2026-08-22

## Context

Crump already represents native pitched Instruments as readable Composable DSP Graphs with typed ports, stable Parameters, explicit assets, deterministic voices, inspection, History, playback, and rendering. A separate monolithic hybrid-synth object, opaque preset, or plug-in-backed state model would duplicate those semantics.

The current parser accepts `crump-module 1`. It rejects other module versions, unknown Nodes, unknown settings, implicit channel conversion, invalid topology, and declarations without runtime behavior. Unsupported syntax never compiles as a no-op and cannot replace the Active Revision.

## Decision

- Graph-authored Instruments remain the native synthesis model. Crump has no dedicated `HybridSynth` kind or opaque native preset format.
- Typed Event, Audio, and Control Ports and explicit Buses remain the signal model. Channel conversion and summing are explicit.
- Stable Parameters, Faceplates, durable IDs, source locations, validation, resource admission, inspection, History, playback, and rendering use the existing Project model.
- Project-owned WAV Media Assets are the only external audio authority used by native sampler behavior. Decode and resampling occur away from the audio callback.
- The `sampler` Instrument remains the supported WAV-backed instrument adapter. It shares the Project media and deterministic playback model rather than introducing separate asset authority.
- Unknown synthesis declarations remain invalid. The parser does not reserve a second source mode or accept placeholder Nodes.

The exact current compatibility boundary is summarized in [Native Synthesis Compatibility](../native-synthesis-contract.md).

## Consequences

Current graph and sampler source remains portable and inspectable. Shared post-voice graph syntax, Instrument-owned modulation matrices, wavetable, granular, multisample-graph, and spectral Nodes are unsupported by the current grammar. Third-party plug-in hosting and arbitrary native extensions are also unsupported.

## Rejected alternatives

- A monolithic built-in synth would duplicate graph, Parameter, Modulation, asset, view, History, and render behavior.
- A dynamically loaded plug-in would introduce opaque state and a separate compatibility boundary.
- Parser-recognized placeholder Nodes would allow valid-looking source without defined runtime behavior.
- Implicit voice/shared placement, channel coercion, or fan-in would hide latency, gain, and resource behavior.
