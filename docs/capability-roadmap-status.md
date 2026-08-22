# Producer capability roadmap status

Status: engineering record; not product-scope or listening acceptance

This record accompanies the draft producer capability roadmap without superseding the Product Contract or accepted ADRs. Automated evidence establishes deterministic engineering behavior, not musical quality or proprietary-device equivalence.

## Implemented engineering

### M1 — shared cookbook

- Self-contained 100-row feasibility matrix with reviewed per-recipe primitives, workaround, gap, source assumptions, and packaging layer.
- Current counts: **64 direct, 21 approximation, 14 blocked, 1 intentional non-goal**.
- Eight readable recipe pages and runnable Track/Return/Group/Master/Automation/sample fixtures.
- Human and Agent guidance points to the same matrix and source/publication path.
- Clean-checkout tests and packages do not depend on optional untracked research notes.

Musician reuse judgment and a recorded human-versus-Agent manual reproduction remain open.

### M2 — sample transport and bounce

- Forward/reverse traversal, bounded half-open loops, loop crossfades, inheritance, checked occurrence operations, and deterministic slice-map creation.
- Exact-Revision Master and Track/Group/Return pre/post-fader bounce with isolated authored release/effect tails.
- Sealed generated artifacts, canonical readable provenance, exact float WAV/hash, and atomic source/media History publication.
- Long sources remain immutable whole assets under explicit byte/compiled-memory limits; no callback decode or streaming fallback exists.

Producer listening workflows and an optional post-audition checked Clip replacement remain open.

### M3a — prepared sample duration/pitch

- Accepted [ADR 0012](adr/0012-prepared-sample-transform-contract.md).
- Exact output-frame offline preparation from one exact source asset/hash/region.
- Independent ±24-semitone pitch using versioned `crump-wsola-effect-v1`.
- Explicit `shift` formant policy; `preserve` is recognized and rejected as unavailable.
- Exact-byte deterministic domain is `x86_64-linux`; preparation is bounded and off-callback.
- Tagged `sample_transform` provenance and atomic publication preserve original assets.

A realtime routed Track/Group pitch processor remains separately gated. Exposed-vocal quality, formant preservation, and listening selection are not claimed.

### M4 — modulation-delay and Crusher

- One bounded `modulation_delay` kind with structural Chorus/Flanger modes and shared typed delay/depth/rate/signed-feedback/mix/width controls.
- One separate `crusher` kind with physical `bits`, reduced sample rate, bounded reconstruction, mix, and output compensation.
- Zero intrinsic latency, preallocated state, continuing bypass behavior, explicit tail policies, exact admission estimates, inspection, Automation/Modulation targets, and objective vectors.

Phaser and added noise remain deferred. Level-matched mode usefulness remains a listening gate.

### M5 — Gate, Envelope Follower, and Stutter

- One sidechain-capable Gate/Expander with explicit detector, threshold, ratio, attack, hold, release, range, hysteresis, and mix.
- Accepted [ADR 0013](adr/0013-audio-derived-control-and-stutter.md).
- Typed channel-tap Envelope Follower with route-local operators, additive physical depth, route/value provenance, fixed resource accounting, and intentional one-sample control latency.
- Finite preceding-audio Stutter with exact structural musical durations, preallocated capture, rising-edge/manual and absolute-grid seeded triggers, ignored/reported warm-up and active triggers, bounded crossfade, finite tail, and grouping-independent output.

General Step/Sample-and-Hold modulation remains deferred until a selected production requires it. Workflow ergonomics and listening acceptance remain open.

## Deliberately not implemented

### M3b — realtime routed pitch

Prepared assets cannot represent every continuously automated Track/Group pitch transition, but no selected effect-heavy production and listening comparison has yet justified a permanent realtime algorithm and its latency/startup/tail contract. This gate remains open rather than labeling a variable-delay approximation as transparent pitch processing.

### M6 — specialist processing

Granular, Grain Delay, vocoder, spectral freeze/time, convolution, dynamic EQ, multiband, transient, and vocal-specialist processors remain blocked by M6's entry rule: a selected complete production must show that simpler native chains or generated assets are inadequate, then pass level-matched independent listening. No such evidence exists in this automated change.

### M7 — CLAP hosting

CLAP remains conditional. No remaining plugin-sized blocker has been selected, and isolated scanning, timeout/crash attribution, quarantine, process isolation, opaque state identity, missing-plugin recovery, GUI policy, offline mode, and deterministic-disclosure contracts are not accepted or implemented.

Arbitrary Project/Agent-generated Rust remains prohibited by ADR 0008. Effect Module syntax also remains deferred until repeated source fixtures prove a deep reusable interface. Live microphone input remains an intentional non-goal.

## Evidence boundary

Automated tests cover source parsing/validation, exact edits, resource admission, processing vectors, supported grouping equality, generated bytes/provenance, History atomicity/recovery, inspection, CLI contracts, rendering, and packaging. They cannot satisfy:

- musician or independent-producer listening;
- sonic equivalence to Ableton or named third-party devices;
- complete-track blind/level-matched selection;
- named-hardware realtime/xrun/endurance qualification; or
- M6/M7 product-entry decisions.
