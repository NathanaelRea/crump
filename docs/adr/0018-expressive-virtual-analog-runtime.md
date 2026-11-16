# ADR 0018: Expressive Virtual-Analog Runtime

Status: Accepted
Date: 2026-08-25

## Context

ADR 0014 keeps native synthesis graph-authored, ADR 0015 establishes stable source version 2, ADR 0016 places portable Modulation on the Instrument, and ADR 0017 adds one Track-owned shared post-voice graph. Milestone 2 still required a complete sample-free expressive path: explicit mono/poly allocation, stable glide and retrigger behavior, richer envelopes/LFOs, reusable stereo filtering, audio-rate oscillator interactions, bounded admission, and musician-facing generated sections.

A monolithic hybrid-synth runtime would duplicate graph ownership, Parameter evaluation, typed ports, publication transitions, and inspection. Treating richer syntax as accepted placeholders would violate the fail-closed Candidate contract. Callback allocation or block-relative random identity would violate the realtime and rendering contracts.

## Decision

### Track-owned expressive voicing

A source-version-2 graph Instrument may declare one `voicing` policy. The policy is compiled into each referencing Track runtime, not shared through the Song:

- `poly` uses the preallocated graph slots and deterministic `release_oldest` or `active_oldest` selection with stable slot-index tie breaking;
- `mono` requires `polyphony 1`, stores at most 64 held Note-On identities, deterministically evicts the oldest on saturation, and selects `last`, `low`, or `high` notes deterministically;
- mono `legato` changes pitch/gate identity without restarting compatible graph state, while `always` restarts behind the existing preallocated graph-voice crossfade;
- Note Off deterministically restores the selected surviving held note;
- central glide is linear in log frequency and may be off, apply to every transition, or apply only to overlapping legato transitions; and
- Project, Instrument, Track, voice slot, note event, source, and authored seed identities remain explicit inputs to deterministic behavior.

Held-note state, replacement tails, and worst-case work are admitted for both resident publication runtimes. Callback paths neither allocate nor sort unbounded collections.

### Graph-authored virtual analog

`analog_oscillator` is a runnable source-version-2 stereo Node. It retains a small typed Interface while internally owning fixed arrays for at most eight subvoices. Its initial laws are:

- sine, polyBLEP saw, square/pulse, and triangle reads;
- coarse/fine tuning, deterministic phase/random phase/drift, blend, power normalization, width, velocity response, level, and pan;
- typed mono Audio FM, PM, rising-edge hard sync, and AM/ring ports;
- FM/PM/sync before phase read, at most two authored phase-warp stages in source order, at most one normalized post-read distortion stage, then amplitude/blend/pan; and
- fixed state/work estimates based on subvoice count and enabled stages.

Unknown operations, excess stages, illegal units/types, and non-finite domains fail validation and defensive compilation.

### Envelopes and LFOs

The existing graph envelope remains backward-compatible ADSR by default and adds AHDSR hold, attack/decay/release curves, `always`/`legato` retrigger (including selected-note transitions retained by mono voicing), deterministic zero-duration boundaries, and optional source-v2 musical-time periods resolved from the owning Song meter/tempo. Authored hold is legal only with `mode ahdsr`; `mode adsr` is therefore not an inert label.

Song, Instrument, and graph LFOs share deterministic waveform and direction laws. Instrument LFO scope makes reset ownership explicit: per-voice sources require note retrigger, while global/per-Track Instrument sources require transport retrigger. Graph LFOs add phase/random phase, polarity, loop/one-shot, forward/reverse/ping-pong, bounded swing phase relation, note/transport clocks, and 2–64 ordered custom piecewise-linear points. Custom interpolation uses fixed immutable storage and admission charges worst-case point traversal.

### Stereo filters

`stereo_filter` owns independent left/right state and exposes cutoff, resonance, drive, wet/dry mix, pan, key tracking, velocity depth, and Control modulation. `svf_clean` supports low-pass, high-pass, band-pass, and notch, with normalized input soft saturation when drive is nonzero. `ladder_drive` is deliberately a nonlinear four-pole low-pass only; unsupported ladder mode combinations fail closed instead of publishing a misleading approximation. The ladder uses deterministic 2× midpoint-interpolated oversampling, per-substep feedback, saturating stages, 1 ms cutoff/resonance/drive smoothing, deterministic reset, and zero declared whole-frame latency. Admission charges sixteen work units per ladder frame.

### Generated sections

Optional `presentation` metadata belongs to the Instrument but is sound-neutral. It groups stable Nodes, Buses, Parameters, Modulation sources/routes, processors, and Project assets into eight closed first-release section kinds. Validation bounds and resolves every reference. The Workspace preserves authored section order, generates missing sections, appends unassigned topology, exposes exact source locations, and delegates Parameter and route depth/bypass changes to checked Production View edits. Presentation metadata is excluded from audio semantic hashes.

### Publication behavior

Structural voicing fields, graph topology, Node allocation, model changes, stage counts, and custom point structure are compatibility facts. Incompatible changes restart only affected Track Instrument state behind ADR 0011's equal-power transition. Compatible Parameter, route, macro, and scalar changes retain admissible state and use existing bounded handoff behavior. Presentation-only changes retain audio state exactly.

## Consequences

The first complete source-v2 sample-free path is composable from ordinary reusable Nodes and runs identically in playback and offline rendering. It has deterministic finite vectors for alias suppression, phase/reset, unison power/stereo behavior, AHDSR/LFO timing, filter response/isolation/smoothing, Modulation, voice stealing/glide/retrigger, transition behavior, callback grouping, and admission.

The selected algorithms are an inspectable implementation baseline, not proof of producer preference. Level-matched complete-track listening may revise defaults or algorithms through later compatible or explicitly transitioned changes. Independent producer qualification remains an external evidence gate and is not implied by this ADR.

Deterministic exact-asset wavetable behavior was subsequently accepted by [ADR 0019](0019-exact-asset-wavetable-runtime.md). Graph sample/multisample behavior was subsequently accepted by [ADR 0020](0020-graph-sample-and-multisample-runtime.md). Bounded granular behavior was subsequently accepted by [ADR 0021](0021-bounded-granular-runtime.md). Deterministic spectral-peak behavior was subsequently accepted by [ADR 0022](0022-deterministic-spectral-peak-runtime.md).

## Rejected alternatives

- A dedicated hybrid-synth Instrument kind would duplicate graph, Parameter, Modulation, transition, and inspection semantics.
- Dynamic voice or held-note collections would violate callback bounds.
- A generic implicit Audio/Control modulation input would erase FM/PM/AM/ring type and ordering semantics.
- Advertising every ladder mode through weak algebraic approximations would make the model label misleading; unsupported combinations fail closed instead.
- Presentation-owned patch state would create a second authority and unnecessary audio restarts.
