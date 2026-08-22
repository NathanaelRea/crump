# ADR 0013: Audio-derived control and bounded Stutter semantics

Status: Accepted
Date: 2026-08-22

## Context

Envelope Followers require current Audio to influence a later control value, but Crump deliberately evaluates controls before Instruments and routing for each frame. Same-frame follower semantics would require a second Audio/control pass or an algebraic control graph. Stutter also needs explicit capture, retrigger, random, duration, and tail rules to remain bounded and independent of callback grouping.

## Decision

### Envelope Follower

- A Follower is a typed Song-level channel-tap-to-Parameter route, not a processor with hidden global output.
- It reads one stable Track/Group/Return pre/post-fader tap, applies linked peak or RMS detection, attack/release smoothing, finite floor/ceiling normalization, route-local operators, and physical-unit depth.
- The contribution is additive in the existing base → Automation → lock → Modulation → constraint order.
- Audio observed at frame N contributes exactly at frame N+1. This one-sample control latency is inspectable and intentional.
- Route identity, source/tap, target/unit/depth/operators, normalized value, physical contribution, and latency are inspectable. Pending contributions and follower state clear on explicit transport discontinuities.
- Same-frame feedback and normalized replacement macros are rejected.

### Stutter

- One native `stutter` processor continuously records preceding Audio into a compile-time-sized ring while idle.
- `slice` and `interval` use a generic exact structural musical-duration contract resolved from the fixed Song tempo. `interval >= slice` is required.
- A false-to-true manual trigger or an eligible absolute-grid trigger freezes the preceding complete slice and replays it oldest-frame first for an exact finite repeat count.
- Automatic chance uses a versioned SplitMix64 decision from authored seed, stable processor identity, and absolute interval cycle. Wall clock, callback grouping, mutable RNG state, and declaration position do not participate.
- Triggers during warm-up or an active repeat are ignored and reported; they do not restart or queue. A held trigger cannot extend the tail.
- Crossfade is an explicit ratio bounded to half the slice. After completion, recording resumes with a fresh full-slice warm-up.
- The processor has zero intrinsic latency, fixed preallocated storage, a finite `repeats * slice` tail ceiling, continuing bypass state, and fixed work per frame.

## Consequences

Envelope Follower state can target earlier or later channels without channel-order semantics because every contribution is delayed one sample. Exact seek uses deterministic preroll; explicit cleared seeks reset pending follower state.

Stutter is a preceding-audio repeater, not a lookahead/oncoming-slice effect and not a proprietary transient-warp mode. A future Tempo Map requires a separately versioned rule because current structural durations compile against one fixed tempo.

General Step, Sample-and-Hold, and standalone random modulation remain deferred until a selected production demonstrates a need beyond deterministic Stutter triggering. Listening and workflow acceptance remain external to these engineering contracts.
