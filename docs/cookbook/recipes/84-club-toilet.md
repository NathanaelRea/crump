---
recipe_id: 84
slug: club-toilet
support: direct
minimum_crump_version: 0.1.0
fixture: fixtures/cookbook/84-club-toilet
---

# Club Toilet

## 1. Musical intent and suitable material

Make a complete section sound distant and bandwidth-limited, as if heard through a wall, before restoring the normal mix. Use this on a Master only for a deliberate arrangement transition.

## 2. Source kinds

The source assumption is the complete Master route. A Group is safer when only one family of Tracks should receive the treatment.

## 3. Required capabilities and minimum version

Use recipe 84 in [`../feasibility.json`](../feasibility.json). It requires Master EQ, Reverb, ordered processors, durable bypass/Automation for section control, and monitoring-only A/B.

## 4. Current behavior and limits

The matrix marks this recipe `direct`. The title describes a production metaphor, not a calibrated acoustic simulation. Native EQ/Reverb do not claim equivalence with the demonstrated DAW devices.

## 5. Ordered source/routing/processor actions

1. Put a low-pass EQ before Reverb on Master or the intended Group.
2. Lower the cutoff until the source feels obstructed while remaining rhythmically legible.
3. Add a moderate room-like Reverb after the filter.
4. Keep final limiting after the creative chain.
5. Use durable bypass Automation to confine the treatment to the intended section when applying it to a real song.

## 6. Tempo-relative calculations and physical-unit ranges

Filter cutoff is authored in hertz; Reverb mix and decay are native physical ratios. The fixture's values are starting points for a short example, not universal room or wall measurements. Place section boundaries in exact Musical Time.

## 7. Automation or Modulation shape

Use step bypass for a hard reveal or a short continuous cutoff/mix ramp for a transition. Preserve the order EQ → Reverb → final Limiter.

## 8. Audition and A/B procedure

Loop material before, during, and after the effect. Compare transient bypass of the EQ/Reverb pair at matched apparent level. Audition the Master path; Soloing one Track cannot establish the complete-mix effect.

## 9. Failure modes and gain staging

Master processing affects every source and can obscure kick, bass, and vocal cues. Reverb may create a large tail at the reveal. Check downstream limiter reduction and ensure Render Tail captures the intended release.

## 10. Listening questions

Does the section feel distant without becoming unintelligible? Is the return to full bandwidth musically clear? Does the tail support or blur the next section? Is the comparison level-matched?

## 11. Validation and publication

Validate Master processor settings/order and any bypass lanes, inspect effective values around the transition, audition the complete range, and publish one explained Master change only after listening.

## 12. Reuse boundary

The EQ/Reverb blocks remain readable and directly addressable. Crump has no Effect Module, opaque preset, or Rack for this treatment.
