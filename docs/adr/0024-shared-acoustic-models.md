# 0024 — Shared acoustic DSP and source-owned body profiles

Status: accepted.

## Decision

Crump provides a shared Rust acoustic DSP module and two native graph Nodes, `exciter` and `modal_bank`. Offline rendering adapters use that same DSP. Instruments compose existing graph Nodes and expose physical controls; numerical resonant-body data belongs in versioned Acoustic Profiles.

A `crump-profile 1` file declares one named profile through the existing normalized source import seam. Source-v2 Instrument Modules can embed profiles, and module export includes the bodies an Instrument requires. Profiles participate in revision identity, publication, recovery and History. They contain frequencies, gains and two decay constants per mode, without recorded waveforms, phases or spectral frames.

## Rationale

Separating contact generation, resonance data and Instrument routing lets several instruments share one implementation while retaining different material responses. Sharing DSP with offline tools prevents coefficient, precision and filter differences from invalidating measurements. Numerical profiles keep large banks out of special-purpose synthesis functions.

This preserves the closed native runtime in ADR 0008. A dynamic plugin per instrument would duplicate host integration without improving the boundary between reusable algorithms and instrument-specific data. There is no arbitrary executable Instrument source or callback scripting.

## Consequences

Profile and excitation domains are validated before compilation. Prepared state, mode ceilings and trigger work enter resource admission. Processing is allocation-free. Physical controls latch at note onset; levels remain continuous. Profile changes restart affected Instrument state.

Texture excitation and optional body rise allow independently developing diffuse and coherent components. Rise is a phenomenological emergence envelope, not a propagation solver. Its zero default preserves the unshaped modal response. Native acoustic graphs and one-shot sample sources receive a bounded 30-second release with a terminal 50 ms fade.

The modal bank is linear. Nonlinear mode coupling, coupled string/soundboard dynamics and mechanical hammer contact remain outside this model. The [acoustic reference](../native-acoustics.md) specifies source syntax, physical domains and lifecycle contracts; [integration tests](../../tests/acoustic_foundation.rs) cover portability, deterministic processing, validation and shared-DSP agreement.
