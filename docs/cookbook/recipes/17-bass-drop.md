---
recipe_id: 17
slug: bass-drop
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/17-bass-drop
---

# Bass Drop

## 1. Musical intent and suitable material

Withhold bass bandwidth before a section boundary, then reveal the full source exactly at the drop. This is most useful when the bass already supports the arrangement after the reveal.

## 2. Source kinds

Use a bass Instrument Track or a Group containing bass layers. The fixture uses a low synthesized percussion source solely to keep the example compact.

## 3. Required capabilities and minimum version

Consult recipe 17 in [`../feasibility.json`](../feasibility.json). The recipe requires EQ, stable processor bypass identity, point Automation, and an exact Locator or musical-time boundary.

## 4. Current behavior and limits

The matrix marks this recipe `direct`. The fixture demonstrates a narrow band and bypass reveal; the correct pre-drop band is source- and arrangement-dependent.

## 5. Ordered source/routing/processor actions

1. Insert an EQ on the bass owner with explicit high-pass and low-pass bounds.
2. Keep the processor active before the drop.
3. Create a Song Automation lane targeting `processor/ID/bypass`.
4. Change bypass to `true` exactly at the drop so the latency-aligned dry path reveals the full bass.
5. Retain the source Clip, Instrument, and processor ID.

## 6. Tempo-relative calculations and physical-unit ranges

Resolve the drop to exact Musical Time or a durable Locator. Use step interpolation for Boolean bypass. Choose filter frequencies by the source fundamental and intended withheld bandwidth, not by copying a genre-independent number.

## 7. Automation or Modulation shape

The essential shape is one discrete step: active EQ before the boundary, bypassed EQ at and after it. Do not use a gradual curve on Boolean bypass.

## 8. Audition and A/B procedure

Loop at least one bar before and after the drop through the Master path. First compare the authored lane, then transiently bypass the EQ for the whole loop to isolate whether anticipation—not a simple level increase—creates the effect.

## 9. Failure modes and gain staging

A narrow band can remove all useful bass or exaggerate one resonance. The reveal can clip if other arrangement layers enter simultaneously. Check pre-drop audibility, drop peak, and downstream limiter activity.

## 10. Listening questions

Is the pre-drop bass still rhythmically legible? Does the reveal feel larger without a distracting level jump? Does the kick/bass relationship improve at the boundary?

## 11. Validation and publication

Validate that the bypass lane is Boolean and step-only, inspect it at times on both sides of the Locator, audition the loop, then publish one explained transition change.

## 12. Reuse boundary

This Project-level arrangement recipe owns a Song-time lane and section boundary outside any reusable serial processor chain.
