---
name: crump-song-arrangement
description: Compose and arrange Crump Songs using Patterns, Clips, Variations, Grooves, tempo and meter maps, Locators, sections, transitions, builds, drops, and deterministic generation. Use for musical structure, sequencing, or Timeline changes.
compatibility: Crump complete-production composition and Timeline capability set.
---

# Crump Song Arrangement

Before making a durable Project change, load [`../crump-project-workflow/SKILL.md`](../crump-project-workflow/SKILL.md) and follow its Candidate, validation, publication, History, and completion contracts.

This skill owns musical material and Timeline structure. Sound design belongs to `crump-instrument-design`; media transforms belong to `crump-sample-production`; final signal flow belongs to `crump-mixing-and-delivery`.

## Translate the brief into an arrangement model

Identify:

- style, tempo range, meter, and duration;
- emotional arc and energy contour;
- primary motif, bass relationship, groove, and rhythmic identity;
- section roles and contrast;
- density, register, space, and stereo changes by section;
- transition vocabulary; and
- deterministic variation policy.

Create named Locators before detailed automation so navigation, audition, bounce, and render ranges share stable identities.

## Material hierarchy

Use the shallowest representation that preserves intent:

- **Events** for concrete notes, hits, and samples;
- **Patterns** for finite reusable musical material;
- **Variations** for lineage-preserving rotate, reverse, transpose, velocity, probability, and transformation;
- **Grooves** for reusable timing and excitation feel;
- **Clips** for placement, loop, source offset, transpose, gain, fades, and local treatment;
- **Automation** for Song-scoped absolute movement;
- **Parameter locks** for Pattern-relative step changes; and
- **Modulation** for continuous reusable motion.

Do not duplicate flattened Events when a readable transformation preserves lineage. Do not hide a concrete musical decision inside random generation.

## Section construction

For each section define:

- active roles and Track hierarchy;
- motif statement or variation;
- bass and kick relationship;
- foreground/background exchange;
- register and octave plan;
- rhythmic subdivision and groove;
- effect/routing state;
- transition in and out; and
- expected listening focus.

Create contrast by changing a few dimensions clearly rather than changing everything simultaneously.

## Tempo and meter

Use explicit tempo and meter maps. Confirm that:

- Event scheduling;
- Clip loops and offsets;
- Automation points;
- synchronized LFOs and Delays;
- Locators, seek, and audition loops;
- bounce/render ranges; and
- fragmentary bars

all resolve through the same map. Do not encode a tempo transition as an unexplained sample stretch when the Song clock should change.

## Builds, drops, and transitions

Choose a transition mechanism by role:

- density removal or staggered re-entry;
- filter and space automation;
- pitch rise/fall or tape movement;
- repeated/stuttered source fragments;
- reverse incoming material;
- reverb/delay swells;
- sub drops, booms, risers, impacts, and noise;
- hard cuts, mutes, and effect-by-effect sequences; or
- tempo/meter movement.

Keep transition material source-backed. Bounce only when printing a transformed result creates useful new material.

## Deterministic generation

Seeded generation may create:

- melodic permutations and counter-lines;
- chord voicings;
- drum fills and hat rolls;
- sample/chop Patterns;
- probability variants;
- call-and-response placement; and
- transition fragment selections.

Store or derive stable seeds. Inspect generated Events before publication. A generated result becomes ordinary readable authored material or explicit procedural lineage; it is never hidden runtime state.

## Automation planning

Plan movement at three scales:

1. **Song arc:** section levels, filters, send amounts, tempo, and Master transition state.
2. **Phrase:** call-and-response, fills, effect throws, and repeated swells.
3. **Event/note:** Parameter locks, note-relative envelopes, sample offset, and retriggered modulation.

Avoid conflicting motion at all three scales. Inspect base, Automation, lock, Modulation, and constrained effective value at representative times.

## Arrangement review

Audition:

- each section alone;
- every transition with one section before and after;
- the complete arc without stopping;
- a low-volume balance pass;
- mono and headphones where relevant; and
- repeated deterministic playback/render.

Ask whether every section has a clear purpose, whether the drop earns its contrast, whether ear candy distracts from the motif, and whether the ending resolves deliberately.

Use [arrangement patterns](references/arrangement-patterns.md) as starting structures, not mandatory genre templates.
