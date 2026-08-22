---
recipe_id: 44
slug: double-delays
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/44-double-delays
---

# Double Delays

## 1. Musical intent and suitable material

Blend two complementary echo rhythms around one source so repeats interlock rather than stack at one division. Sparse vocals, percussion, or melodic hits reveal the relationship clearly.

## 2. Source kinds

Use one or more Tracks feeding two Returns. Shared Returns let several sources contribute to the same continuing Delay states without duplicating processor instances.

## 3. Required capabilities and minimum version

Use recipe 44 in [`../feasibility.json`](../feasibility.json). It requires two Returns, durable Sends, tempo-synchronized Delay divisions, explicit feedback/mix, and stable routing.

## 4. Support state and known approximation

The matrix marks this recipe `direct`. Device division labels differ across DAWs; Crump uses the exact source spelling and its documented straight/dotted/triplet calculation.

## 5. Ordered source/routing/processor actions

1. Create one Return for the straight Delay and one for the contrasting triplet or dotted Delay.
2. Keep each Delay nearly or fully wet and control blend with Return/Send levels.
3. Add separate Sends from the source Track to both Returns.
4. Route both Returns to Master and preserve acyclic routing.
5. Adjust divisions before raising feedback.

## 6. Tempo-relative calculations and physical-unit ranges

Choose explicit divisions such as `1/4` and `1/4t`; Crump resolves both from Song tempo. Keep feedback below the legal maximum and Delay mix below `1`. Use dB for Send and Return balance.

## 7. Automation or Modulation shape

Static Sends prove the core recipe. For throws, automate Send level or durable bypass at phrase boundaries; do not hide a Return state change outside source.

## 8. Audition and A/B procedure

Loop a sparse phrase through the Master path. Mute neither dry source nor both Returns during the first pass. Compare each Return alone, then both together, then transiently bypass both Delays while retaining routing.

## 9. Failure modes and gain staging

Similar divisions can reinforce into an unintended loud repeat, while high feedback can obscure the source and tail beyond the render range. Start Returns low and verify the explicit Render Tail.

## 10. Listening questions

Can each rhythm be identified? Do they create useful motion rather than clutter? Is the stereo image stable? Do repeats leave space for the next phrase?

## 11. Validation and publication

Validate unique Send/Return IDs, route acyclicity, division spellings, and Delay domains. Inspect effective Delay times, audition and level-match, then publish one routing change with rationale.

## 12. Related Effect Modules

This remains a Project-level recipe because it spans two Returns and multiple Sends. A serial filtered Delay inside either Return could later become an Effect Module, but dual routing is not one module.
