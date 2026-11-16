---
recipe_id: 48
slug: split-delays
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/48-split-delays
---

# Split Delays

## 1. Musical intent and suitable material

Give low and high frequency regions different echo rhythms so repeats remain separated in time and spectrum. Broad-band percussion, vocals, and harmonically rich melodies are suitable.

## 2. Source kinds

Use a Track with two Sends feeding two Returns. The source must contain useful material on both sides of the selected crossover.

## 3. Required capabilities and minimum version

Consult recipe 48 in [`../feasibility.json`](../feasibility.json). It requires Returns, Sends, EQ, tempo-synchronized Delay, explicit processor order, and stable dB balance.

## 4. Current behavior and limits

The matrix marks the recipe `direct`. Filter slope and named-device behavior are not proprietary equivalents; the fixture demonstrates complementary native EQ bounds and distinct Delay divisions.

## 5. Ordered source/routing/processor actions

1. Create low-delay and high-delay Returns.
2. Put EQ before Delay on each Return.
3. Low-pass one Return and high-pass the other around one explicit crossover region.
4. Choose distinct Delay divisions and keep each wet path controlled by its Send/Return level.
5. Feed both Returns from the same source and route them to Master.

## 6. Tempo-relative calculations and physical-unit ranges

Write crossover frequency in hertz and Delay divisions in exact musical notation. Treat `1khz` as a source-specific starting point only. Keep feedback below the legal bound and Delay mix below `1`.

## 7. Automation or Modulation shape

Static filters and divisions prove the recipe. Phrase throws may automate Send levels. Avoid continuously moving both crossover filters without first checking overlap or a spectral hole.

## 8. Audition and A/B procedure

Loop a full-band source. Compare the low Return alone, high Return alone, both Returns, and the dry source. Then transiently bypass EQ on one Return to verify that spectral separation—not only different timing—does useful work.

## 9. Failure modes and gain staging

Overlapping filters can double a band; separated filters can leave a hole. Wet Returns can add substantial energy even when each sounds quiet alone. Start Sends low and inspect Master peaks and tails.

## 10. Listening questions

Are low repeats clear without mud? Do high repeats stay smooth rather than harsh? Does the crossover feel continuous? Do the two rhythms support the arrangement?

## 11. Validation and publication

Validate EQ domains, Delay divisions, processor order, unique Send IDs, and route acyclicity. Inspect both Returns, audition in context, then publish one explained routing state.

## 12. Reuse boundary

Each filtered Delay is an ordinary serial processor chain. The two-Return topology and source Sends remain explicit Project routing.
