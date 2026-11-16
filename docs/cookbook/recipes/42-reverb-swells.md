---
recipe_id: 42
slug: reverb-swells
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/42-reverb-swells
---

# Reverb Swells

## 1. Musical intent and suitable material

Move a source between focused and spacious states by raising and lowering Reverb mix over a phrase. Sparse percussion, pads, vocals, and melodic tails make the shape easy to judge.

## 2. Source kinds

Use a Track or Group whose Reverb can be safely automated. A Return is preferable when several sources should share exactly one continuing Reverb state.

## 3. Required capabilities and minimum version

Consult recipe 42 in [`../feasibility.json`](../feasibility.json). It requires Reverb, point Automation on a physical ratio target, exact Song time, and transient processor bypass for monitoring comparisons.

## 4. Current behavior and limits

The matrix marks the recipe `direct`. Native Reverb settings are not a reconstruction of Ableton's decay controls, so judge the Crump tail on its own terms.

## 5. Ordered source/routing/processor actions

1. Insert or select one Reverb with a stable processor ID.
2. Set its base mix to the lane's opening value.
3. Create one Automation lane targeting `processor/ID/mix`.
4. Add a smooth rise toward the intended crest and return to the dry value before an abrupt boundary.
5. Keep processor order and downstream routing unchanged while comparing the lane.

## 6. Tempo-relative calculations and physical-unit ranges

Place points at exact musical positions. Mix remains within ratio `[0,1]`; shape movement with `linear` or `power(p)` interpolation. Tail length comes from explicit Reverb behavior plus Project Render Tail, not hidden silence detection.

## 7. Automation or Modulation shape

Use one or more smooth rise-and-fall curves. Avoid duplicate positions and abrupt high-mix steps unless a deliberate cut is audible and click-safe.

## 8. Audition and A/B procedure

Loop the complete swell and enough post-range time to hear the tail. Compare the authored lane with transient Reverb bypass, then compare against a constant-mix version at matched apparent level.

## 9. Failure modes and gain staging

A long, loud wet path can mask attacks, accumulate energy, or end beyond the authored Render Tail. Keep mix bounded, check the crest in context, and set Render Tail explicitly for delivery.

## 10. Listening questions

Does the swell lead attention into the next section? Is the crest spacious rather than simply louder? Does the dry source remain intelligible? Is the rendered tail complete?

## 11. Validation and publication

Validate lane order, domains, and interpolation; inspect effective mix at the opening, crest, and end; audition the range; then publish one explained Automation change.

## 12. Reuse boundary

The ambience chain uses ordinary processors. Its Automation lane is Song-scoped, and Crump has no Effect Module syntax for this chain.
