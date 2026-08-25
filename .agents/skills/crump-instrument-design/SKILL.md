---
name: crump-instrument-design
description: Design and revise expressive Crump graph, sampler, multisample, wavetable, granular, spectral, and hybrid Instruments with Faceplates, modulation, voicing, and reusable modules. Use for synthesis, sound design, timbre, playable instrument, or instrument architecture requests.
compatibility: Crump complete-production Instrument and graph capability set.
---

# Crump Instrument Design

Before making a durable Project change, load [`../crump-project-workflow/SKILL.md`](../crump-project-workflow/SKILL.md) and follow its Candidate, validation, publication, History, and completion contracts.

This skill owns sound-source and Instrument topology. Song structure belongs to `crump-song-arrangement`; channel finishing belongs to `crump-mixing-and-delivery`.

## Start from a sound brief

Translate ordinary language into testable dimensions:

- role and register;
- transient shape and sustain;
- harmonic density and brightness;
- pitch stability, glide, and movement;
- mono/stereo behavior;
- velocity, key, note-age, and gate response;
- desired variation across a phrase; and
- four to eight musician-facing controls.

Do not begin by selecting the most complex source. Choose the smallest architecture capable of the requested behavior.

## Source selection

| Need | Preferred source |
| --- | --- |
| Stable subtractive tone | Analog/virtual-analog oscillator plus filter and envelopes |
| Wide evolving harmonic source | Wavetable with bounded frame morph and modulation |
| Bells, metallic transients, complex bass | FM/PM or waveshaping operators |
| Noise, impacts, risers | Noise/excitation plus envelopes, filters, pitch, and effects |
| One owned recording | Sample source with explicit region, root, direction, loop, and gate law |
| Key/velocity layers | Multisample with readable zones, articulation, and deterministic round robin |
| Clouds, pads, textures | Granular source with bounded position, grain, density, window, spread, and seed |
| Frozen or transformed spectra | Spectral source/process with explicit analysis identity, FFT law, latency, and state |
| Carrier/modulator timbre | Vocoder with typed carrier and modulator Audio inputs |
| Complex production instrument | Hybrid graph combining several sources through explicit mixers |

Every sample-derived source uses a declared exact-hash Project asset. Generated derivatives retain their source identity and algorithm provenance.

## Build a deep Instrument module

1. **Define the external interface first.** Choose a small Faceplate of meaningful physical-unit Parameters.
2. **Choose voicing.** Declare mono/poly mode, polyphony, note priority, stealing, legato/retrigger, glide, unison, and replacement behavior.
3. **Construct source layers.** Keep gain and phase relationships explicit.
4. **Shape amplitude and timbre.** Use ADSR/AHDSR or custom envelopes with clear retrigger behavior.
5. **Filter and distort deliberately.** Select topology/model for a stated audible role; avoid accidental loudness wins.
6. **Add modulation.** Use typed sources and routes with explicit polarity, mapping, depth, order, and bypass.
7. **Use the shared post-voice graph for coherent Track-owned effects and tails.** Keep per-voice processing only where note-relative state is musically required.
8. **Expose only high-leverage controls.** Leave deeper Parameters inspectable without crowding the Faceplate.

The Instrument interface should hide implementation complexity while preserving source readability and deep inspection.

## Modulation design

Available sources include:

- envelope and graph Control ports;
- global, Instrument, and per-voice LFOs;
- sine, triangle, saw, square, sample-and-hold, and custom shapes;
- free or synchronized rate, phase, swing, direction, loop/one-shot, and retrigger policy;
- note velocity, key, gate, and age;
- deterministic random hold;
- Parameters and macro controls; and
- audio-derived followers where Track-owned movement is intended.

For every route state:

- source scope and polarity;
- target unit/domain;
- mapping curve;
- physical-unit depth;
- fold order;
- clamp behavior; and
- whether a topology or allocation change requires restart.

Prefer one musically legible movement over several unrelated LFOs.

## Expressive checks

Test at minimum:

- soft and hard velocity;
- low, middle, and high register;
- short and long notes;
- legato and retriggered phrases;
- dense polyphony and voice replacement;
- mono fold-down and stereo width;
- control minima, defaults, and maxima;
- repeated deterministic render; and
- publication while notes/tails are active.

Listen for clicks, stuck voices, level jumps, aliasing, weak control ranges, phase cancellation, unstable sub energy, and modulation that masks the musical role.

## Reuse

Save a successful graph plus Faceplate as a readable Instrument Module. Preserve exact asset declarations and derivative identities. A reusable module must remain valid outside the original Song and should not depend on hidden Track routing or Master processing.

Use [instrument architecture patterns](references/instrument-patterns.md) when selecting a starting topology.
