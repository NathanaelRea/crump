# Finalized-audio analysis standards review

**Accessed:** 2026-08-21  
**Code reviewed:** `src/engine/analysis.rs`, `src/engine/oversampling.rs`, and `src/engine/render.rs`

## Scope and conclusion

Crump analyzes the decoded bytes of each finalized stereo float WAV. Its loudness path follows the BS.1770 K-weighting and integrated-gating structure, and its short-term/Loudness Range path now follows the EBU 3-second/10 Hz and two-stage LRA-gating structure. Automated tests pass the official EBU steady-tone loudness tolerance and EBU true-peak steady-tone cases 15–19.

This demonstrates algorithm compatibility, not blanket standards conformance. Crump has not run the complete official EBU test-signal set, its custom 4×/129-tap reconstruction filter is not the Annex 2 example filter, and transient/phase-offset true-peak cases 20–23 are unverified. Product documentation names the exact Crump analysis schema and reconstruction kernel and does not claim “EBU Mode conformant” or “BS.1770-5 conformant.”

## Primary-source requirements

### Programme loudness

ITU-R BS.1770-5 Annex 1 defines:

- the K-weighting pre-filter and RLB high-pass stage;
- equal channel weights of `1.0` for ordinary left and right stereo channels;
- the `−0.691 + 10 log10(...)` loudness calibration;
- 400 ms gating blocks with 75% overlap, hence a 100 ms hop;
- an absolute gate at `−70 LKFS`; and
- a relative gate 10 LU below the mean of the absolute-gated blocks.

EBU R 128 adopts the gated BS.1770 Programme Loudness method for its broadcast-normalization context. That context is not evidence for one universal target for every creative master or distribution platform.

### Short-term loudness and Loudness Range

EBU Tech 3341 §2.2 specifies ungated Short-term Loudness over a sliding rectangular 3-second window. A live EBU Mode meter updates it at least 10 times per second and can display its maximum.

EBU Tech 3342 §3.1 and Appendix 1 specify Loudness Range from Short-term Loudness values computed at **at least 10 Hz**:

1. retain values at or above `−70 LUFS`;
2. convert that absolute-gated population back to power and calculate its mean loudness;
3. set a relative threshold 20 LU below that absolute-gated short-term mean;
4. retain values at or above the relative threshold; and
5. subtract the nearest 10th-percentile value from the nearest 95th-percentile value.

The LRA relative threshold is not `final integrated programme loudness − 20 LU`; integrated Programme Loudness uses different 400 ms blocks and a different relative gate.

### True-peak estimation

ITU-R BS.1770-5 Annex 2 describes 4× oversampling from 48 kHz to 192 kHz, low-pass interpolation, absolute peak detection, and dBTP reporting. It supplies one 48th-order, four-phase FIR coefficient set that satisfies its guidance and allows methods with similar or superior results. A 4× ratio alone does not establish equivalent response or error.

EBU Tech 3341 Table 1 cases 15–23 provide published true-peak expectations and tolerances. Crump exercises the steady-tone cases 15–19 (`+0.2/−0.4 dBTP`). Cases 20–23 use a synthesized transient and four downsampling offsets and have not been executed.

### Product-defined metrics

The cited standards do not prescribe Crump's JSON representation, sample full-scale-crossing counts, arithmetic per-channel DC mean, or non-finite publication policy. These remain explicit product contracts:

- `null` represents silence, an empty gated population, or insufficient samples for a required window;
- `clipped_sample_count` counts finite decoded samples with magnitude at least `1.0` in a float WAV;
- `dc_offset` is the arithmetic mean of finite decoded samples per channel; and
- any non-finite sample rejects render publication.

None of these facts implies hidden normalization or a universal loudness target.

## Current Crump behavior

| Area | Current behavior | Supported wording |
| --- | --- | --- |
| Artifact identity | Reopens finalized stereo 48 kHz IEEE float WAV bytes; Render schema 2 hashes and revalidates every artifact and recomputes its analysis before replacement. | Finalized-file analysis; exact-artifact evidence. |
| K-weighting/stereo | BS.1770 48 kHz coefficients, shelf then high-pass, equal L/R energy sum. | BS.1770-derived K-weighted stereo loudness. |
| Integrated loudness | 400 ms windows at 100 ms hops, `−70` absolute gate, absolute-gated mean `−10 LU` relative gate. | BS.1770-derived gated integrated loudness; official EBU tone case 1 passes `±0.1 LU`. |
| Short-term maximum | Ungated 3-second windows evaluated every 100 ms. | Maximum sampled 3-second Short-term Loudness at a 10 Hz hop. |
| Loudness Range | 3-second/10 Hz population, `−70 LUFS` absolute gate, absolute-gated short-term mean `−20 LU` relative gate, nearest 10th/95th percentiles. | Tech 3342-derived LRA algorithm; complete EBU test-signal conformance has not been demonstrated. |
| Reconstructed peak | Custom fixed 4×, 129-tap Blackman-windowed-sinc kernel, named in every analysis object. | Crump 4× reconstructed peak estimate / dBTP-style measurement; cases 15–19 pass. Annex 2 conformance is not claimed. |
| DC/clipping/non-finite | Explicit deterministic Crump metrics and failure policy. | Product diagnostics, not BS.1770/R 128 conformance measures. |

Analysis schema 2 identifies the corrected 10 Hz Short-term sampling and Tech 3342 LRA gate/percentile behavior. The manifest's `loudness_method` string names those mechanics rather than claiming EBU conformance.

## Official sources

- ITU-R, [Recommendation BS.1770-5, *Algorithms to measure audio programme loudness and true-peak audio level* (11/2023)](https://www.itu.int/rec/R-REC-BS.1770-5-202311-I/en), especially Annex 1 §§2–4 and Annex 2 §§1–3. [Official PDF](https://www.itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.1770-5-202311-I!!PDF-E.pdf).
- EBU, [R 128, *Loudness normalisation and permitted maximum level of audio signals*](https://tech.ebu.ch/docs/r/r128.pdf), §§2–3.
- EBU, [Tech 3341, *Loudness Metering: “EBU Mode” metering to supplement loudness normalisation in accordance with EBU R 128* (2023)](https://tech.ebu.ch/docs/tech/tech3341.pdf), §§2.1–2.3 and Table 1.
- EBU, [Tech 3342, *Loudness Range: A measure to supplement loudness normalisation in accordance with EBU R 128* (2023)](https://tech.ebu.ch/docs/tech/tech3342.pdf), §3.1, §4, and Appendix 1.
