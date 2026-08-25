# ADR 0014: Graph-Authored Native Synthesis

Status: Accepted
Date: 2026-08-22

## Context

Crump already represents native pitched Instruments as readable Composable DSP Graphs with typed ports, stable Parameters, explicit assets, deterministic voices, inspection, History, playback, and rendering. A separate monolithic hybrid-synth object, opaque preset, or plug-in-backed state model would duplicate those semantics.

At the time of this decision, the parser accepted `crump-module 1`. It rejected other module versions, unknown Nodes, unknown settings, implicit channel conversion, invalid topology, and declarations without runtime behavior. Unsupported syntax never compiled as a no-op and could not replace the Active Revision. Stable source-v2 foundation behavior was subsequently decided in [ADR 0015](0015-stable-source-v2-graph-foundation.md).

## Decision

- Graph-authored Instruments remain the native synthesis model. Crump has no dedicated `HybridSynth` kind or opaque native preset format.
- Typed Event, Audio, and Control Ports and explicit Buses remain the signal model. Channel conversion and summing are explicit.
- Stable Parameters, Faceplates, durable IDs, source locations, validation, resource admission, inspection, History, playback, and rendering use the existing Project model.
- Project-owned WAV Media Assets are the only external audio authority used by native sample-backed behavior. Decode, sample resampling, and derived wavetable compilation occur away from the audio callback.
- The `sampler` Instrument remains the supported WAV-backed instrument adapter. It shares the Project media and deterministic playback model rather than introducing separate asset authority.
- Unknown synthesis declarations remain invalid. The parser does not reserve a second source mode or accept placeholder Nodes.

The exact current compatibility boundary is summarized in [Native Synthesis Compatibility](../native-synthesis-contract.md).

## Consequences

Current graph and sampler source remains portable and inspectable. Stable source-v2 Instrument-owned Modulation was subsequently accepted by [ADR 0016](0016-instrument-owned-modulation.md), the Track-owned shared post-voice graph runtime by [ADR 0017](0017-shared-post-voice-graph.md), expressive voicing/virtual-analog Nodes plus generated presentation sections by [ADR 0018](0018-expressive-virtual-analog-runtime.md), deterministic exact-asset wavetable import/playback by [ADR 0019](0019-exact-asset-wavetable-runtime.md), graph sample/multisample playback by [ADR 0020](0020-graph-sample-and-multisample-runtime.md), bounded granular playback by [ADR 0021](0021-bounded-granular-runtime.md), deterministic spectral-peak resynthesis by [ADR 0022](0022-deterministic-spectral-peak-runtime.md), and the stable-source/experimental-sound support split by [ADR 0023](0023-native-synthesis-compatibility-boundary.md). Third-party plug-in hosting and arbitrary native extensions are also unsupported.

## Rejected alternatives

- A monolithic built-in synth would duplicate graph, Parameter, Modulation, asset, view, History, and render behavior.
- A dynamically loaded plug-in would introduce opaque state and a separate compatibility boundary.
- Parser-recognized placeholder Nodes would allow valid-looking source without defined runtime behavior.
- Implicit voice/shared placement, channel coercion, or fan-in would hide latency, gain, and resource behavior.
