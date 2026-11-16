# ADR 0023: Native Synthesis Compatibility Boundary

Status: Accepted
Date: 2026-08-25

## Context

Crump now has runnable source-v2 virtual-analog, wavetable, graph sample, multisample, granular, and spectral Instruments, Track-owned shared processing, generated Instrument views, exact media ownership, and deterministic offline rendering. Stable readable source and disposable derived algorithms need a compatibility policy before independent musical and named-hardware qualification is complete.

Treating every implemented algorithm or starter patch as supported sound compatibility would turn automated conformance tests into a producer-credibility claim. Keeping all source experimental would instead allow valid authored topology to be reinterpreted silently after users have published Projects. Offline wall-clock profiling also cannot establish PipeWire/device behavior.

## Decision

- `crump 2` and `crump-module 2` are stable source headers. Accepted declarations retain their documented fail-closed meaning; an incompatible interpretation requires a new source or explicitly versioned algorithm identity.
- The sample conversion, wavetable mip, spectral analysis/resynthesis, schedule/event, and offline qualification identities listed in the native-synthesis contract are stable compatibility identities. Their caches remain disposable and exact-input-bound.
- Sound compatibility is still experimental. Filter palette/defaults, category-level sound expectations, starter-content credibility, and the final Serum-class capability claim may change until the independent-producer protocol and named Linux/PipeWire/device endurance protocol pass.
- Automated fixture, render, callback-allocation, portability, and recovery evidence may establish engineering behavior only. Blank templates, repository-authored listening notes, or an offline `qualify` report cannot satisfy producer, PipeWire, or named-hardware gates.
- The experimental sound disclaimer is removed only by a later evidence-backed support decision. Unsupported or changed behavior continues to fail closed rather than being accepted as a silent no-op.

## Consequences

Users can rely on readable source-v2 structure, exact Project-owned asset identity, named cache rebuild laws, validation, History, and render semantics without Crump claiming that unfinished defaults are producer-credible. Implementations may improve provisional sound algorithms, but must preserve stable source meaning or introduce a new explicit algorithm/version boundary.

Release preparation must keep engineering, producer, and named-hardware evidence separate. The offline qualification command records no PipeWire xruns, and supported-device claims require the named endurance record.

## Evidence

- [Native Synthesis Compatibility](../native-synthesis-contract.md)
- [Native synthesis qualification runbook](../qualification/native-synthesis.md)
- [`tests/native_synthesis_qualification.rs`](../../tests/native_synthesis_qualification.rs)
- [`tests/qualification/clean-account-portability.sh`](../../tests/qualification/clean-account-portability.sh)
- [Milestone 8 evidence templates](../evidence/native-synthesis-m8/README.md)
