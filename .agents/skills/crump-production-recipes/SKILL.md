---
name: crump-production-recipes
description: Apply intent-led Crump production recipes and complex effect workflows by combining arrangement, sampling, sound design, routing, Automation, resampling, and mixing. Use for named producer effects, vocal treatments, ear candy, transitions, and cookbook-style requests.
compatibility: Crump complete-production workflow and effect capability set.
---

# Crump Production Recipes

Before making a durable Project change, load [`../crump-project-workflow/SKILL.md`](../crump-project-workflow/SKILL.md) and follow its Candidate, validation, publication, History, and completion contracts.

This skill orchestrates production outcomes. It does not create one opaque plugin per recipe or duplicate the specialist skills’ primitive details.

## Load specialists by operation

- Musical structure, Events, Clips, builds, drops: [`crump-song-arrangement`](../crump-song-arrangement/SKILL.md)
- Instrument/source topology and playable controls: [`crump-instrument-design`](../crump-instrument-design/SKILL.md)
- Slicing, pitch/formants, bounce, resampling, granular/spectral media: [`crump-sample-production`](../crump-sample-production/SKILL.md)
- Routing, processors, Automation, sidechains, Master, delivery: [`crump-mixing-and-delivery`](../crump-mixing-and-delivery/SKILL.md)

Load only the specialists needed for the chosen construction.

## Start from intent, not a device name

Translate a request into:

1. **Material:** voice, drums, bass, melody, pad, full mix, or generated noise.
2. **Outcome:** slower, larger, haunting, glitchy, distant, rhythmic, rising, talking, crushed, widened, or transformed.
3. **Construction:** source edit, Instrument graph, realtime processor chain, routed parallel effect, offline generated asset, arrangement operation, or a combination.
4. **Control:** static Parameter, Automation, Pattern lock, Modulation, or deterministic Events.
5. **Commit point:** live and editable, reusable module, or explicitly bounced/resampled derivative.
6. **Evidence:** what to audition, level-match, inspect, render, and ask the musician to decide.

Named tutorial effects are references for musical intent, not proprietary compatibility claims.

## Recipe construction loop

### 1. Inspect the source in context

Trace the source through its Instrument, Track, Groups, Returns, and Master. Identify the exact phrase/region and locate any existing movement, latency, or tail state.

### 2. Decompose into reusable primitives

Prefer combinations of:

- Event/Clip edits;
- pitch, time, formant, loop, and traversal laws;
- graph sources and modulators;
- filters, dynamics, nonlinear processing, delay, modulation, and reverb;
- granular, spectral, convolution, and vocoder processing;
- explicit auxiliary routing and feedback owned inside a bounded processor;
- Automation and Parameter locks; and
- bounce/resample when stable Audio becomes useful source material.

If a construction recurs, save it as a transparent reusable Effect Module or Instrument Module with a small Faceplate.

### 3. Stage the chain

A reliable order is:

1. source/arrangement transformation;
2. corrective cleanup;
3. main audible transformation;
4. dynamics and gain compensation;
5. spatial or parallel treatment;
6. Automation/Modulation; and
7. output safety.

Change this order only for an intentional interaction, such as distortion into filtering, reverse-wet-print-reverse, delay into pitch, or compression keyed from another source.

### 4. Preserve editability

Keep readable source and exact asset lineage. Use realtime structures while Parameters should remain adjustable. Bounce only for iterative resampling, exact prepared transforms, CPU/latency policy, or when the Audio itself becomes new material.

### 5. Audition matched alternatives

Compare:

- dry versus complete wet construction;
- live versus printed result when both exist;
- solo versus full context;
- section entry/exit and full tail;
- mono versus stereo; and
- conservative versus exaggerated settings.

Match perceived level and ask one concrete listening question.

## Musical guardrails

- One strong identity effect usually beats several unrelated novelties.
- Protect lyric intelligibility, bass phase, and decisive transients unless destroying them is the goal.
- Let shared Returns own shared space and continuing state.
- Keep random choices seeded and inspectable.
- Tie transitions to exact Locators and section purpose.
- Do not flatten source automatically after a successful effect.
- Report approximations whenever the intended law differs from the named reference.

## Catalogs

Use:

- [`docs/cookbook/`](../../../docs/cookbook/README.md) for executable recipes and fixtures;
- [`docs/research/100-producer-effects-video-outline.md`](../../../docs/research/100-producer-effects-video-outline.md) for the 100-effect source catalog; and
- [recipe families](references/recipe-families.md) to choose a construction quickly.

The cookbook feasibility matrix is implementation evidence, not a creative restriction or a second support contract inside this skill.
