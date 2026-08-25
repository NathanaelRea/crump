# ADR 0022: Deterministic Spectral Peak Runtime

Status: Accepted
Date: 2026-08-24

## Context

Milestone 5 requires independent source-time scanning and pitch movement without decoding, FFT construction, unbounded peak tracking, or allocation on the audio callback. Exact Project media must remain authoritative, analysis must be disposable and reproducible, and the product must not describe a bounded sinusoidal model as transparent phase-vocoder time stretching.

## Decision

`spectral` is a runnable source-version-2 stereo voice Node. It references one exact Project WAV declared with `usage spectral`, requires typed `pitch`, and requires the explicit algorithm identity `crump-spectral-peaks-v1`. At most two spectral Nodes are admitted in one voice graph, and each Node resynthesizes 1–128 peaks.

Only spectral assets reached from Track-assigned Instruments are decoded and analyzed. The existing `crump-sample-windowed-sinc-v1` Project-rate stereo derivative is the analysis input. Exact source bytes remain Authoritative Source; decoded frames and spectral analysis are independent disposable derivatives.

`crump-spectral-peaks-v1` uses:

- a 2,048-frame periodic Hann window;
- a 256-frame hop;
- deterministic in-tree radix-2 stereo FFTs;
- local-magnitude peak detection below Nyquist;
- magnitude-descending selection with source-bin order as the exact tie breaker;
- at most 128 fixed peak slots per frame, with zero-filled unused slots; and
- a versioned positive spectral-flux transient classifier whose threshold is 0.35 times prior-frame magnitude energy.

The spectral cache identity hashes the algorithm name, exact source SHA-256, Project-rate sample-cache identity, Project sample rate, 2,048/256 geometry, 128-slot ceiling, periodic-Hann/channel/ranking law, and transient rule. Inspection exposes those identities and shapes, analysis/transient frame counts, and payload bytes, but not retained peak arrays or host paths.

Each graph voice preallocates one stereo phase pair per authored `max_peaks` plus fixed scan state. Runtime linearly interpolates corresponding magnitude-ranked slots in adjacent analysis frames. `scan` chooses normalized analysis position; while not frozen, `rate -4…4` advances source time in source-frame units independently of pitch. The note input relative to `root`, plus `tune`, multiplies retained frequencies. Source-analysis `low` and `high` bounds are applied before repitch, and their complete Parameter domains must remain strictly ordered. Post-pitch frequencies at or above Nyquist are omitted.

Transient policy is structural: `preserve` keeps running phases, `reset` adopts analyzed phase when entering a classified transient frame, and `suppress` applies a fixed 0.25 transient gain. `phase_randomization` adds a deterministic Project/Track/voice/note/Node/slot/channel identity offset when phases initialize or reset. Active-peak power normalization and a final finite float clamp prevent non-finite output from extreme but finite source values. The model has zero declared runtime latency because all windowed analysis is already materialized; Note Off follows ordinary graph-voice lifetime and adds no spectral tail.

Immutable analysis payload is estimated from verified container metadata before decode or FFT allocation and added to exact-asset publication preflight. Final admission counts the shared analysis payload once per publication runtime, every graph-program reference/header, phase state for both graph-voice storage slots, peak work on every callback frame, and both resident publication runtimes. Callback code performs only bounded peak reads, interpolation, trigonometric phase synthesis, scalar control evaluation, and fixed-state updates.

Analysis/cache identity, `max_peaks`, root, transient policy, authored seed, and durable Node identity are structural transition facts. Compatible scan/rate/tune/frequency-region/phase-randomization/freeze/level edits retain state. Structural edits restart only the affected Track Instrument behind ADR 0011's bounded equal-power transition.

## Consequences

The source provides deterministic spectral animation, pitch movement, frequency selection, transient shaping, and phase decorrelation under explicit resource ceilings. Rendering is callback-grouping independent, exact asset copies rebuild the same cache, and unused assets/Instruments allocate no analysis derivative.

This is a ranked sinusoidal-peak resynthesizer. Ranked-slot interpolation can change partial correspondence between frames. Crump does not claim transparent time stretching, phase-vocoder reconstruction, formant preservation, transient fidelity, or commercial spectral-tool parity without controlled evidence. Independent producer listening remains pending.

## Rejected alternatives

- Callback FFTs, peak allocation, or mutable analysis would violate realtime bounds.
- An external cache file as authority would weaken Project portability and recovery.
- Unbounded partial tracking would make callback work and state data-dependent.
- A block-relative scan accumulator would change output with callback grouping.
- Calling this model transparent time stretching would overstate the accepted algorithm.
