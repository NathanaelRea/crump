---
name: crump-sample-production
description: Import, slice, loop, reverse, stretch, pitch/formant-process, bounce, resample, granulate, vocode, convolve, and transform Project-owned audio with explicit provenance. Use for vocal chops, textures, drum extraction, sample instruments, and audio-derived effects.
compatibility: Crump complete-production media and generated-audio capability set.
---

# Crump Sample Production

Before making a durable Project change, load [`../crump-project-workflow/SKILL.md`](../crump-project-workflow/SKILL.md) and follow its Candidate, validation, publication, History, and completion contracts.

This skill owns media identity, non-destructive sample treatment, and generated assets. Instrument topology belongs to `crump-instrument-design`; Timeline structure belongs to `crump-song-arrangement`.

## Asset-first workflow

1. Import the lawful source into the Project.
2. Verify exact hash, container, channels, source rate, duration, and frame count.
3. Keep all source coordinates in exact half-open original frames.
4. Decide whether the result should remain a playback treatment or become a generated asset.
5. Preserve the original bytes and record every generated derivative’s provenance.
6. Audition raw, mapped, through its channel, and through Master before committing placement.

Never author an absolute external path or mutate a declared source file in place.

## Choose the correct operation

| Need | Operation |
| --- | --- |
| Select a word/hit/region | Exact slice or occurrence region |
| Reverse a phrase | Reverse traversal; no new asset unless later processing must be printed |
| Sustain a stable region | Bounded loop with reviewed crossfade |
| Tempo-align without pitch change | Prepared time stretch with explicit algorithm/quality |
| Pitch without duration change | Independent pitch transform with explicit formant policy |
| Tape-style linked pitch/time | Variable-rate playback or routed pitch/time processor |
| Print a wet chain | Exact-range bounce from owner/tap with explicit tail |
| Re-edit a whole passage | Bounce, import atomically, then place/chop separately |
| Build playable slices | Transient analysis or reviewed exact slice map |
| Produce clouds/textures | Granular source or routed grain processor |
| Freeze/smear harmonic content | Spectral process with declared analysis identity |
| Apply another sound as response | Convolution with exact-hash impulse identity |
| Make a source articulate another | Vocoder/carrier-modulator with typed auxiliary Audio |

## Transform policy

For time, pitch, warp, or formant work, author:

- source asset/hash and exact region;
- output duration or frame count;
- pitch displacement;
- formant behavior;
- transient/tonal/texture quality mode;
- phase/channel policy;
- algorithm and version;
- deterministic domain; and
- output format/hash.

Use deliberate artifact modes only for an audible purpose. Do not label one algorithm as a proprietary DAW mode unless its law is actually implemented.

## Bounce and resampling

A bounce identifies:

- exact Active Revision;
- Track/Group/Return/Master owner and tap;
- exact time or Locator range;
- included processor/routing policy;
- tail duration or termination rule;
- output format and algorithm identity; and
- destination asset ID/name.

Publish the WAV, provenance, and declaration atomically. Bounce never places the result, disables the source, or replaces material automatically.

Common source-first resampling loop:

1. preserve the editable source;
2. bounce the selected result;
3. audition raw and in context;
4. create slices/mappings or a new Track;
5. rearrange, reverse, stretch, granulate, or process the derivative; and
6. keep lineage visible through every generation.

## Vocal workflows

For vocal effects:

- review consonant and vowel boundaries rather than relying blindly on transient detection;
- preserve intelligibility on the dry path when the vocal carries meaning;
- use explicit pitch/formant policy for doubles, demons, harmonies, and rewinds;
- de-ess or filter feedback paths where repeats exaggerate sibilance;
- align reverse reverbs and pre-echoes to the destination onset;
- keep vocoder carrier and modulator routes explicit;
- level-match processed layers against the lead; and
- check mono, tails, and phrase boundaries.

## Drum and loop workflows

- Slice using reviewed transients and preserve source attack.
- Use choke groups for mutually exclusive articulations.
- Keep kick/sub tuning and phase relationships explicit.
- Use deterministic round robin or seeded variation rather than hidden randomness.
- Crossfade bounded loops and inspect seam energy.
- Separate pitch-linked repitch from independent tempo alignment.

## Dynamic sample control

Sample offset, loop bounds, direction, tune, grain position, size, density, and rate may be automated or modulated according to their declared latching/continuous policy. Do not assume a control changes an already active voice when the Parameter is note-latched; retrigger deliberately when needed.

Use [sample transform workflows](references/sample-workflows.md) for common multi-step constructions.
