# ADR 0019: Exact-Asset Wavetable Runtime

Status: Accepted
Date: 2026-08-24

## Context

ADR 0014 keeps native synthesis graph-authored, ADR 0015 establishes stable source version 2, and ADR 0018 defines the bounded expressive oscillator and voicing contract. Milestone 3 requires a wavetable source without introducing an external library dependency, opaque patch state, callback compilation, or a second Instrument runtime.

A WAV used as a wavetable is not ordinary time-domain sample playback. Its exact sample sequence must have an explicit periodic-frame interpretation, amplitude policy, and inter-frame morph law. High-frequency playback also needs deterministic band limiting, while the original Project-owned bytes remain the only durable authority. Derived tables may be rebuilt and must not become History state.

## Decision

### Exact Project-owned source

A source-version-2 Project may declare a WAV Media Asset with `usage wavetable` and complete metadata; an imported Instrument Module may reference that Project-owned asset by durable ID:

- `frame_length` is a power of two from 32 through 8,192 samples;
- `frames` is 1 through 256, with at most 2,097,152 interpreted samples in one asset;
- the WAV is mono and contains exactly `frame_length × frames` samples;
- `normalization` is `preserve`, `global_peak`, or `frame_peak`; and
- `morph` is `linear` or `catmull_rom`.

Frames are consecutive in the exact decoded mono sample order. The WAV container sample rate does not retime a periodic frame; oscillator pitch is resolved at the owning Project's supported sample rate. Non-finite decoded values, incomplete metadata, wrong channel count or length, missing/changed bytes, unsupported bounds, and non-wavetable asset references fail before publication.

`asset import-wavetable` copies an external regular mono WAV into `media/ID.wav`, computes the complete-byte SHA-256, authors all interpretation metadata, validates the source-and-binary overlay, and publishes both sides through one History transaction. It never persists the external path or replaces an existing destination.

### Disposable deterministic compilation

Compilation occurs away from the callback and materializes only tables referenced by Instruments assigned to runtime Tracks; unused declarations retain exact source/asset inspection without allocating derived mips. For each normalized authored frame, `crump-wavetable-radix2-v1` performs a deterministic radix-2 transform and builds Nyquist-bounded mip levels whose maximum harmonics descend from `frame_length / 2` by powers of two through one. DC is retained. Inverse-transform results are stored as immutable `f32` samples in level-major, then authored-frame order.

The cache identity hashes the algorithm ID, exact asset SHA-256, Project sample rate, frame length/count, normalization, and morph law. It does not replace the Media Asset identity and is not Authoritative Source. Rebuilds from unchanged source produce the same identity, admitted program, inspection, and audio under Crump's declared deterministic domain.

Compiled tables are immutable and shared through `Arc`; graph voice states retain only fixed-size phase, glide, drift, and synchronization state. Admission includes the immutable table object, Arc headers and payload conservatively, all subvoice/state work, and both resident publication runtimes. No table decode, transform, allocation, deallocation, filesystem access, or unbounded search occurs in the callback.

### Runnable graph Node

`wavetable` is a runnable source-version-2 stereo voice Node. It requires one declared wavetable asset and a mono `pitch` input. Optional typed inputs are mono Audio `fm`, `pm`, and `sync`, plus Control `gain` and `velocity`. Scalar settings cover coarse/fine tuning, one to eight preallocated unison lanes, phase/random phase, table position, detune/blend/width/drift/glide, FM/PM depth, velocity amount, level, and pan.

For every subvoice and frame, FM/PM/rising-edge hard sync are resolved before the phase read. At most two authored `pulse_width`, `bend`, `remap`, or `phase_remap` stages transform phase in declaration order. The runtime selects the first mip whose harmonic ceiling fits the current Nyquist bound, performs cyclic linear phase interpolation, and applies the declared linear or boundary-clamped Catmull–Rom interpolation between adjacent authored frames. Gain, velocity response, power-normalized unison blend, and equal-power pan follow the read. Stable Project, Instrument, Track, voice, note-event, Node, subvoice, and authored seed identities make phase and drift independent of callback grouping.

Table position and warp amounts may be ordinary scalar Parameters and therefore receive Automation, Track-local locks, and compatible Modulation under the existing ownership and same-frame control rules. The Workspace generated Instrument view shows exact asset metadata, frame order, normalization/morph law, source location, deterministic waveform previews, and live route activity without owning DSP state.

### Publication behavior

Asset identity, frame metadata, morph law, unison allocation, stage structure, Node variant, and graph topology are structural compatibility facts. A change to any of them restarts only the affected Track Instrument state behind ADR 0011's bounded equal-power transition. Compatible scalar and route edits retain state and use the existing bounded handoff. Presentation-only changes remain sound-neutral.

## Consequences

Wavetable assets are portable with Project source and History, and deleting derived runtime data cannot make them unrecoverable. Exact-source validation, deterministic cache identity, source-mip alias bounds, normalization/morph vectors, finite output, stereo unison, retrigger behavior, publication compatibility, admission, and callback-grouping equivalence are machine checked. The mip bound describes harmonics retained from the authored frame before phase warps or audio-rate FM/PM; nonlinear and cross-modulation sidebands require their own qualification and are not silently covered by that claim.

This decision establishes deterministic playback and import semantics, not a full spectral-bin/formula editor and not evidence of commercial or producer preference. The original procedural starter table is MIT fixture content. Independent producer and named-hardware qualification remain external evidence gates.

Graph sample/multisample behavior was subsequently accepted by [ADR 0020](0020-graph-sample-and-multisample-runtime.md). Bounded granular behavior was subsequently accepted by [ADR 0021](0021-bounded-granular-runtime.md). Deterministic spectral-peak behavior was subsequently accepted by [ADR 0022](0022-deterministic-spectral-peak-runtime.md).

## Rejected alternatives

- Retaining an external wavetable-library path would violate Project portability and History.
- Storing compiled mip tables as authoritative assets would couple source identity to disposable implementation data.
- Compiling, normalizing, or selecting frames through dynamic collections in the callback would violate realtime bounds.
- Interpreting arbitrary WAV length or stereo layout heuristically would make authored frame order and morph behavior unstable.
- Full-band lookup at every pitch would knowingly alias; silently changing the source bytes would destroy exact ownership.
- Accepting the Node before parser, validation, compilation, inspection, admission, playback, rendering, and failure behavior agreed would create a valid-looking no-op.
