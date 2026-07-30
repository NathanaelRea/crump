# Prove Safe Text-to-Audio Graph Replacement

Type: prototype
Mode: HITL
Status: resolved
Blocked by: none

## Question

Can a minimal Rust prototype compile edited source into a replacement audio graph, retain the last valid graph after an invalid edit, and swap valid graph state without allocation, locking, dangerous output, or avoidable discontinuity on the audio callback?

## Answer

Yes, at prototype fidelity. Carry forward this planning baseline:

- Parse, validate, compile, and allocate candidate revisions away from the audio callback. An invalid Candidate Revision never enters the publication queue, so the Active Revision remains unchanged.
- Publish complete graphs through a bounded non-blocking handoff. Backpressure or later coalescing belongs on the producer side; the callback never waits.
- Return superseded graph ownership through a second bounded callback-safe handoff and destroy it off-callback.
- Protect the final output independently of transition policy; the prototype converts non-finite output to silence and clamps its deliberately simple signal to `+/-0.95`.
- For a live graph replacement with no more specific semantic rule, apply a `4 ms` crossfade by default. An application-level authoring preference may disable this fallback. The preference is not Authoritative Source and must not affect saved-project or offline-render semantics.
- Do not universalize the fallback. Parameter ramps, state migration, effect-tail preservation, restart behavior, sample/loop transitions, and musical quantization remain decisions for [Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md).

[The throwaway Rust prototype](../assets/safe-graph-replacement-prototype/README.md) uses preallocated SPSC publication and retirement rings around boxed fixed-size graphs. Across 200 stress edits it rejected 29 invalid candidates, completed all 171 valid swaps, retained the final valid revision, recorded zero callback allocations or deallocations, and capped peak output at `0.95`. Its waveform/WAV comparison made the click from disabling the fallback audible. This is evidence for the ownership and handoff shape, not a production queue implementation, final graph representation, or hardware audio benchmark.

[Industry Audio Transition Policies](../assets/industry-audio-transition-policies.md) found no universal transition rule. SuperCollider combines a configurable replacement fade with independent beat quantization and deferred cleanup; FunDSP separates real-time publication/off-render retirement from immediate or crossfaded replacement; Ableton defaults clip edits to short fades governed by an application preference; VST3 and CLAP standardize event timing and restart contracts rather than mandatory smoothing. These precedents support mechanism plus a configurable default fallback while reserving edit-specific semantics.

## Comments

- ADR 0011 now supplies the First Public Release product policy: a valid topology change may use a short protective fade and restart only the affected instrument while transport and unchanged Tracks continue. Exact voice and effect-state migration remains deferred; the prototype remains ownership/publication evidence rather than the final transition contract.
