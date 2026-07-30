# ADR 0004: Routing, Modulation, and Parameter Semantics

Status: Accepted destination semantics; First Public Release subset below
Date: 2026-07-26

## Context

Crump needs one source-readable model for timeline processing, reusable DSP graphs, mixer routing, sidechains, automation, modulation, live editing, and offline rendering. DAW terms such as group, return, and bus overlap in common usage, while hidden channel conversion, parameter normalization, control-rate behavior, or latency policy would undermine deterministic playback and portable projects.

## Decision

- The compiled project is an acyclic graph of Processing Nodes with named typed Ports and Buses. Ports carry Event, channel-layout-aware Audio, or unit-typed scalar Control streams in one sample-time domain. Input cardinality is single, optional, or explicitly summing; incompatible signal units or Audio layouts require explicit conversion nodes. Sidechains are named auxiliary Audio inputs, not a separate routing mechanism. Intentional feedback is encapsulated by an explicit bounded stateful device.
- Authored DSP graphs are hierarchical reusable definitions with typed interfaces. Internal nodes have definition-local Durable Domain IDs; runtime identity composes the enclosing instance-ID path with the local ID even if compilation flattens the graph.
- Track, Group, Return, and singleton Master are distinct channel roles. Group membership requires a member's main output to pass through its parent Group. A top-level Track or Group defaults to Master. A Return receives durably identified Sends, defaults to Master, and may instead feed a Group or later Return if the graph remains acyclic. Bus names the typed routed signal, not another channel kind, and timeline Tracks cannot act as ad hoc main-output buses.
- A Send targets exactly one Return and independently selects a pre-fader or post-fader tap. Every Track, Group, and Return sums inputs, runs ordered inserts, exposes the pre-fader tap, applies gain and an explicit panner mode, applies mute, exposes the post-fader tap, then routes its main output. `mono_pan` uses equal-power mono-to-stereo placement, `balance` attenuates stereo sides without crossfeed, and `stereo_pan` positions the two input sides independently using the mono law.
- A Parameter is identified by its owner's durable or intrinsic identity plus an immutable owner-defined key. First-release kinds are continuous real, bounded integer, boolean, and enum. Parameters declare domains, defaults, physical units, display mappings, and automation/modulation capabilities; source never stores adapter-normalized values. Authored out-of-domain values invalidate the Candidate Revision. Runtime modulation overshoot clamps, and non-finite Control holds the last finite effective value with a diagnostic.
- Effective values evaluate in a fixed order: base value, one optional Song-scoped absolute Automation function, additive Modulation routes in stable identity order, then domain constraint. Point Automation starts at Song time zero, has unique increasing points, holds its last value, and explicitly uses `step`, `linear`, or `power(p)` per segment in canonical units. For `power(p)`, `p > 0` and `v(x) = a + (b - a) * x^p`. Integer, boolean, and enum Automation is step-only.
- Reusable Pattern, envelope, and LFO control reaches Parameters as Modulation rather than competing absolute Automation. Modulation sources declare normalized unipolar `[0,1]` or bipolar `[-1,1]` output; every route declares depth in the target's modulation unit. Continuous Automation and Modulation are sample-accurate and block-size-independent. The host does not add universal smoothing; node-owned smoothing is deterministic DSP behavior shared by playback and rendering.
- Every node output declares deterministic sample latency for its active configuration. Compilation compensates Event, Audio, and Control paths at every convergence so all inputs represent the same source time, including parallel, Send, Return, sidechain, and cross-domain paths. A latency change produces a new Candidate Revision.
- Mute is source-backed, step-automatable project state. Solo, cue, and monitoring are transient Audition State and do not affect offline rendering. A non-authoritative safety stage sanitizes and emergency-limits real-time output after Master; offline rendering bypasses it, fails on non-finite output, and is limited only by explicit authored Master processing.

## First Public Release Scope

The binding subset is a hierarchical acyclic graph of typed Event, Audio, and Control Ports; durable graph-node identity; explicit legal conversion and summing; physical-unit Parameters with domains and defaults; deterministic envelopes and modulation required by the graph instrument; filtering, gain/pan, simple delay, simple reverb, and saturation; simple Track level balance into one Master; and protected real-time output. Groups, Sends, Returns, general mixer Buses, Song Automation, sidechains, compressors, detailed EQ, mastering, arbitrary parallel routing, full convergence-point latency compensation, and advanced audition semantics remain accepted destination design but are not release obligations until a validated workflow needs them.

## Consequences

The DSL and compiler must expose routing, conversion, identities, units, interpolation, and latency rather than infer them from source position or device convention. The model gives graph replacement and state migration stable targets, makes sidechains and parallel processing ordinary typed routing, and keeps rendering independent of block size and workspace audition controls. It deliberately rejects arbitrary routing cycles, implicit channel coercion, normalized authored parameters, overlapping absolute clip automation, generic Track-as-bus behavior, and hidden host smoothing.

## Alternatives Considered

Separate Group, Return, and Bus channel kinds; fully generic channels whose topology alone determines their role; arbitrary delayed routing cycles; implicit Audio conversion and fan-in; normalized or device-defined Parameter values; unrestricted control graphs; clip-automation precedence; partial latency compensation; and persistent Solo state were rejected because they create hidden or unstable semantics, weaken static validation, or make playback and rendering diverge.

## Evidence

- [Resolving issue](../../.scratch/crump-first-release-plan/issues/07-define-routing-model.md)
- [Ableton Live 12: Mixing](https://www.ableton.com/en/live-manual/12/mixing/)
- [Ableton Live 12: Routing and I/O](https://www.ableton.com/en/live-manual/12/routing-and-i-o/)
