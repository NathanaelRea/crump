# ADR 0011: Graph Transition Behavior

Status: Accepted
Date: 2026-07-28

A valid topology change restarts the affected Instrument's voices and effect state behind an aligned short equal-power fade. Transport and semantically unchanged Tracks continue, and the workspace reports the transition.

Topology-compatible publication retains compatible voice, Track-owned shared post-voice graph, processor, and compensation state. Only changed continuous effective targets receive a preallocated monitoring ramp; unchanged step and linear Automation remains sample-exact, and boolean controls remain discrete. Shared graph declaration reordering is canonicalized; compatible shared scalar edits use the same preallocated 10 ms affected-target handoff as voice controls.

Crump does not migrate incompatible voice, held-note/voicing, shared graph, oscillator, envelope/LFO, filter, delay, or reverb state across topology or allocation-changing edits. Voicing mode/retrigger/glide-mode/priority/steal/seed changes are structural; compatible glide-duration changes update retained Track-local policy. Sound-neutral presentation metadata is excluded from audio semantic compatibility and never restarts state. It also does not restart the whole transport for one affected Instrument. The shared-domain details are fixed by [ADR 0017](0017-shared-post-voice-graph.md), expressive voicing/virtual-analog details by [ADR 0018](0018-expressive-virtual-analog-runtime.md), exact-asset wavetable structure/cache identity by [ADR 0019](0019-exact-asset-wavetable-runtime.md), graph sample/multisample cursor/map structure by [ADR 0020](0020-graph-sample-and-multisample-runtime.md), granular pool/window/edge structure by [ADR 0021](0021-bounded-granular-runtime.md), and spectral analysis/peak/transient structure by [ADR 0022](0022-deterministic-spectral-peak-runtime.md).
