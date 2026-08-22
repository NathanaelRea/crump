# ADR 0012: Prepared sample transform contract

Status: Accepted
Date: 2026-08-22

## Context

Independent sample duration and pitch preparation creates durable media whose exact bytes depend on algorithm, boundary, rounding, formant, and platform choices. Treating that operation as a channel bounce would make provenance false. Authoring only a floating stretch ratio would also leave the authoritative output duration dependent on implicit rounding.

## Decision

Crump admits one narrow offline prepared-sample transform before any realtime routed pitch processor:

- Generated-asset provenance is a tagged `bounce` or `sample_transform` contract. Existing untagged `generated { ... }` declarations remain legacy bounce spelling.
- A sample transform names an exact Active Revision, source asset and exact source-byte hash, half-open original-source frame region, exact output frame count, finite pitch displacement, formant policy, quality, format, algorithm, deterministic domain, and build.
- Exact output frames are authoritative. Ratios or musical durations may be client conveniences only after resolution to that frame count.
- `effect_v1` uses the versioned native `crump-wsola-effect-v1` algorithm. Algorithm changes require a new identity rather than changing existing output silently.
- Exact-byte determinism is promised only for the `x86_64-linux` domain of the current release baseline. Provenance records that domain.
- The first formant policy is `shift`. `preserve` is recognized but rejected as unavailable; it is never approximated silently.
- Decode, sample-rate conversion, whole-region analysis/preparation, and WAV encoding remain off the realtime callback. The original asset remains unchanged, and generated source plus bytes publish through the existing atomic History transaction.
- No generated asset automatically replaces or disables source material. Placement is a separate checked and auditioned decision.

## Consequences

Prepared bytes can be reopened and consumed by the existing deterministic sampler in playback and render. Exact duration and pitch are independently requested, but engineering availability does not certify exposed-vocal quality or formant preservation. Additional quality modes need new algorithm identities and listening evidence.

A routed Track/Group pitch processor remains a separate decision. It must justify native realtime permanence with an effect-heavy transition that prepared assets cannot satisfy, and must define fixed latency, state, tail, bypass, resource, and supported-quantum behavior through the ordinary processor seam.

## Rejected alternatives

- Overloading bounce owner/tap/tail fields for sample transforms was rejected because it would publish false provenance.
- A separate unrelated provenance block was rejected because it would duplicate generated-media validation and publication semantics.
- Authored stretch ratio as the durable duration contract was rejected because implicit rounding would weaken exact identity.
- Cross-platform exact-byte claims and formant preservation were rejected for the first algorithm because they have not been proven.
