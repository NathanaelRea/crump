# ADR 0021: Bounded Granular Runtime

Status: Accepted
Date: 2026-08-24

## Context

Milestone 5 requires sample-derived texture generation without weakening exact asset authority, deterministic rendering, or callback bounds. A granular source has bursty note-start work and many independently moving cursors, so an unbounded grain collection, wall-clock randomness, or callback-block scheduler would violate Crump's runtime contract.

## Decision

`granular` is a runnable source-version-2 stereo voice Node. It references one exact Project asset declared with `usage granular` and shares the `crump-sample-windowed-sinc-v1` immutable Project-rate frames accepted by ADR 0020. Only assets reached by Track-assigned Instruments compile, and their payload enters asset-aware two-runtime admission before decode allocation.

The Node requires typed `pitch` and `gate` inputs. It owns 1–64 preallocated grain slots. Scalar controls are normalized position, signed scan speed, 5 ms–1 s size, 0.1–1,000 Hz density, root/tune, normalized spray, reverse-direction probability, stereo spread, freeze, and level. Structural settings are maximum grains, Hann/triangle/Tukey window, wrap/reflect/clamp edge behavior, and an authored 16-bit seed.

A per-voice fractional density accumulator advances once per sample. At most one spawn is eligible per sample under the admitted density range. Spawn identity mixes the authored seed with the stable Project/Instrument/Track/voice/note/durable-Node state and a monotonic per-note spawn index; callback grouping, topological vector position, unrelated Node insertion, and source order do not participate. A spawn scans the already allocated slots and is dropped deterministically if all are active.

Each grain snapshots position, size, pitch increment, direction, and pan at spawn. Position combines the current same-frame control, scan phase unless frozen, and deterministic signed spray. Linear source interpolation and the authored window run for an exact grain duration. Wrap uses Euclidean source-frame wrapping, reflect uses one bounded triangular-period fold and derives direction after any number of admitted boundary overshoots, and clamp terminates the grain. Grain outputs receive equal-power stereo placement and active-count power normalization. Note Off stops spawning; already active grains finish, so the source tail is bounded by the one-second maximum grain size.

Work admission charges every declared grain slot for every resident graph voice, including note-start selection/spawn overhead. State admission includes the boxed granular state and complete fixed-capacity grain vector for both graph-voice storage slots and both resident publication runtimes. The callback allocates, decodes, parses, and locks nothing.

Asset/cache identity, maximum slots, window, edge law, root, and seed are transition compatibility facts. Changes restart only the affected Track Instrument behind ADR 0011's bounded equal-power transition. Compatible position/scan/size/density/tune/spray/direction/spread/freeze/level changes retain state and affect future spawns; active grains keep their snapshots.

Generated Instrument rows expose exact asset/container/hash facts, waveform preview, grain settings, and source navigation. The fixture asset remains original MPL-2.0 repository content.

## Consequences

Finite output, deterministic semantic seeding under unrelated topology insertion, stereo spread, every window/edge law, arbitrary multi-boundary reflection, valid 64-slot admission, 65-slot rejection, callback-grouping equivalence, structural transitions, exact portability, and cache/admission reuse are machine checked. These checks establish bounded behavior, not producer-credible texture quality. Independent listening remains pending.

Spectral analysis/resynthesis is decided separately by ADR 0022.

## Rejected alternatives

- Dynamic grain allocation or a priority queue would violate callback bounds.
- Block-relative spawn timing would change output with callback grouping.
- Mutable global randomness would leak between Tracks and voices.
- External sample paths or a second granular cache would violate exact Project ownership.
- Calling rate independent time stretch would misstate the repitch-only cursor law.
