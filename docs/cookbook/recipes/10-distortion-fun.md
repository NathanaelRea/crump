---
recipe_id: 10
slug: distortion-fun
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/10-distortion-fun
---

# Distortion Fun

## 1. Musical intent and suitable material

Create an aggressive rhythmic smear by feeding a short tempo-related echo into heavy Saturation. Percussion, a pluck, or a sparse melodic phrase leaves the echo pattern easiest to hear.

## 2. Source kinds

Use a Track carrying a native Instrument, drums, or a portable sample. Keep the original source lawful and Project-owned when media is involved.

## 3. Required capabilities and minimum version

Use recipe 10 in [`../feasibility.json`](../feasibility.json) for current support and version facts. The chain needs ordered Delay and Saturation processors plus physical-unit output compensation.

## 4. Current behavior and limits

The matrix marks the recipe `direct`. Crump's native Saturation is not Waves OneKnob Driver and the result is not a claim of proprietary-device equivalence.

## 5. Ordered source/routing/processor actions

1. Put Delay before Saturation on the selected Track.
2. Choose a triplet Delay division and set feedback to zero for one rhythmic smear.
3. Raise Delay mix only enough to expose the offset layer.
4. Increase Saturation drive, select an explicit native mode, and reduce output gain.
5. Preserve processor IDs and order when revising settings.

## 6. Tempo-relative calculations and physical-unit ranges

Use an authored Delay division such as `1/8t`; the engine resolves it from Song tempo. Keep feedback at `0`, begin mix near one third, and compensate the Saturation output in dB. Treat those as starting regions, not universal defaults.

## 7. Automation or Modulation shape

For section accents, automate Delay mix or processor bypass with explicit step or curved points. Do not automate a structural Delay mode or division as if it were continuous.

## 8. Audition and A/B procedure

Loop a phrase, transiently bypass both processors together, then compare each processor separately. Level-match the processed chain against the dry source before deciding that more drive is better.

## 9. Failure modes and gain staging

Heavy drive can mask the rhythmic source, increase DC-like asymmetry, and make the limiter do all gain control. Reduce Saturation output, inspect peaks, and keep feedback at the intended value.

## 10. Listening questions

Is the triplet motion clear? Does distortion add useful density without erasing attacks? Does the chain work in the complete mix rather than only in Solo?

## 11. Validation and publication

Validate the Candidate, inspect effective tempo-derived Delay time and processor order, audition through Track and Master paths, then publish one explained chain change.

## 12. Reuse boundary

This treatment uses directly authored Track processors. Crump has no Effect Module identity for the chain.
