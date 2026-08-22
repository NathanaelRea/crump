---
recipe_id: 35
slug: hat-roll
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/35-hat-roll
---

# Hat Roll

## 1. Musical intent and suitable material

Turn a short hat or bright one-shot into an accelerating or repeated transition layer with controlled high-frequency space. The source should tolerate rapid retriggering without a dominant low-frequency tail.

## 2. Source kinds

Use stock synthesized hats or a declared exact-hash WAV in a Sampler Instrument. The fixture exercises the portable sample path with durable sample occurrences.

## 3. Required capabilities and minimum version

Use recipe 35 of [`../feasibility.json`](../feasibility.json). Required capabilities are Pattern Events, sampler regions/fades, EQ, Reverb, tempo-synchronized Delay, and Clip fades.

## 4. Support state and known approximation

The matrix marks the recipe `direct`. Crump expresses the roll as explicit Events rather than an opaque roll generator; event spacing and source choice remain musical decisions.

## 5. Ordered source/routing/processor actions

1. Import and declare the WAV, or select stock hats.
2. Define a bounded sampler region with short source fades.
3. Author repeated Events at exact subdivisions and vary velocity or pan deliberately.
4. Place EQ, Reverb, then Delay in an inspectable Track chain.
5. Fade the Clip into the transition and keep the source occurrences editable.

## 6. Tempo-relative calculations and physical-unit ranges

Choose Event spacing from exact beat fractions such as `1/4beat` or finer values appropriate to tempo. Keep sample region/fades in original source frames. Choose Delay division in musical time and Reverb/Delay mix as ratios.

## 7. Automation or Modulation shape

The fixture uses explicit velocity growth and a Clip fade. A longer roll may automate filter cutoff or processor mix, but repeated Events should remain the primary rhythm when they express the task clearly.

## 8. Audition and A/B procedure

Audition the asset raw, then the mapping, Track channel, and complete Master path. Loop the transition and compare the Track processors with transient bypass. Check that every occurrence remains distinct enough at tempo.

## 9. Failure modes and gain staging

Dense overlapping one-shots can build unexpected level and harshness. Keep polyphony bounded, use region fades, trim the sample, and monitor the high-frequency Return/Delay tail after the Clip ends.

## 10. Listening questions

Does the roll increase momentum? Are attacks crisp without clicks? Does stereo movement remain intentional? Does the ambience support rather than wash out the transition?

## 11. Validation and publication

Validate exact asset hash, regions, fades, Event times, and processor domains. Audition every path, publish the complete intended roll once, reopen, and render the Active Revision when needed.

## 12. Related Effect Modules

The filtered Reverb/Delay tail may later inform a module candidate. Event creation, sample identity, Clip fades, and arrangement placement remain Project-level recipe actions.
