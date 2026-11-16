---
recipe_id: 4
slug: bass-booster
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/04-bass-booster
---

# Bass Booster

## 1. Musical intent and suitable material

Add audible low-mid weight in parallel without replacing the source bass. Use a bass Instrument, low percussion, or Group whose dry path already has controlled sub energy.

## 2. Source kinds

Suitable matrix assumptions are a native Instrument or drums routed from a Track into a Return. The fixture routes its low source through a Group so the parallel branch and main hierarchy remain explicit.

## 3. Required capabilities and minimum version

Use the capabilities and minimum Crump version in recipe 4 of [`../feasibility.json`](../feasibility.json): Send/Return routing, EQ, Saturation, and source-backed gain.

## 4. Current behavior and limits

The matrix marks this recipe `direct`. The captions conflict about whether the demonstrated filter is high-pass or low-pass, so Crump does not infer that missing detail. The fixture uses an explicit bounded low band as one safe example, not a reconstruction of the proprietary chain.

## 5. Ordered source/routing/processor actions

1. Keep the source's main output on its normal Group or Master route.
2. Create one Return with EQ before Saturation.
3. Set explicit high- and low-frequency bounds appropriate to the source; do not copy the fixture blindly.
4. Drive Saturation fully wet on the Return and compensate its output.
5. Add a post-fader Send from the source and raise it from silence while listening.

## 6. Tempo-relative calculations and physical-unit ranges

No tempo conversion is required. Start with a narrow low band around the bass's audible harmonics rather than boosting subsonic energy. Keep Return drive, output dB, Send dB, and Return level explicit in source.

## 7. Automation or Modulation shape

Automation is optional. If the boost belongs only in a drop, automate the Send level or durable processor bypass with step boundaries; do not create hidden section state.

## 8. Audition and A/B procedure

Loop a bass-heavy range through the Master path. Compare with transient Return-processor bypass, then lower the dry path only for a level-matched comparison. Also audition on a mono-compatible path and at low monitoring level.

## 9. Failure modes and gain staging

Parallel low bands can create clipping, phase-dependent cancellation, or an uncontrolled sub. Avoid compensating loudness only by turning up the Return. Keep the Master limiter as protection, not as the recipe's loudness mechanism.

## 10. Listening questions

Does the bass read more clearly on small speakers? Is the sub still stable in mono? Does the branch add harmonics rather than only level? Does the kick retain space?

## 11. Validation and publication

Validate the complete Candidate, inspect Return order and effective settings, audition the selected range, then publish one explained change. Render the Active Revision only after the musician accepts the balance.

## 12. Reuse boundary

This recipe uses ordinary Return processors. Crump has no Effect Module identity for the chain.
