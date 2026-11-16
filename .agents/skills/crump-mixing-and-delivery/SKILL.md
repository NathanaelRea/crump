---
name: crump-mixing-and-delivery
description: Mix and finish Crump Songs with routing, Groups, Sends, Returns, sidechains, processors, Automation, modulation, meters, mastering, stems, and delivery renders. Use for balance, effects, mix decisions, mastering, export, or release preparation.
compatibility: Crump complete-production mixer, processor, analysis, and delivery capability set.
---

# Crump Mixing and Delivery

Before making a durable Project change, load [`../crump-project-workflow/SKILL.md`](../crump-project-workflow/SKILL.md) and follow its Candidate, validation, publication, History, and completion contracts.

This skill owns channel signal flow, processing, balance, Master behavior, and delivery. It does not rewrite composition merely to avoid making a mix decision; when arrangement is the cause, load `crump-song-arrangement`.

## Establish the mix intent

Define:

- foreground and supporting roles;
- low-end hierarchy;
- transient hierarchy;
- tonal balance and density;
- depth/space plan;
- stereo and mono-compatibility goals;
- movement and effect-throw vocabulary;
- reference listening conditions; and
- delivery outputs.

Trace every important source to Master before adding processors.

## Gain staging

1. Begin with channel levels and panning.
2. Preserve headroom through every serial and parallel branch.
3. Level-match bypass and alternate chains.
4. Inspect peak, average/loudness behavior, clipping, and non-finite telemetry.
5. Compensate nonlinear and dynamics output explicitly.
6. Treat the Master limiter as final control, not a substitute for balance.

Avoid hidden normalization and universal loudness targets.

## Routing

Use explicit roles:

- Tracks own sources and insert chains;
- Groups own hierarchy and shared processing;
- Returns own reusable parallel state;
- Sends choose exact pre/post-fader taps;
- sidechains name auxiliary detector/carrier/modulator inputs; and
- Master owns final authored processing.

Keep routing acyclic unless a bounded stateful processor explicitly owns feedback. Use one shared Return when sources should inhabit the same continuing delay/reverb state.

## Processor choice

| Need | Processor family |
| --- | --- |
| Gain, polarity, mono, width | Utility and panner |
| Corrective or tonal balance | Static/dynamic EQ and filters |
| Level control, punch, density | Compressor, multiband dynamics, transient shaper |
| Rhythmic opening/cleanup | Gate/expander and envelope follower |
| Echo and movement | Stereo/ping-pong Delay, modulation delay, chorus, flanger, phaser |
| Space | Algorithmic Reverb, convolution, spectral/granular ambience |
| Harmonics and edge | Saturation, distortion, clipping, crusher/decimator |
| Pitch/formant movement | Routed pitch/formant processor |
| Rhythmic repetition | Stutter/Beat Repeat and explicit Event edits |
| Carrier/modulator effects | Vocoder |
| Peak control | True-peak-aware Limiter |

Use one processor family per actual problem. Do not stack devices merely because a named recipe does.

## Dynamics and sidechains

For Compressor, Gate, multiband, dynamic EQ, or follower routes, declare:

- detector source/tap;
- detector mode and filtering;
- threshold, ratio/range, knee/hysteresis;
- attack, hold, release, and lookahead;
- wet/dry and makeup/output;
- latency and compensation; and
- expected gain-reduction range.

Audition the detector relationship and the resulting program Audio separately.

## Space and delay

- Filter feedback and wet paths deliberately.
- Choose tempo divisions from the groove rather than defaults.
- Separate slap, rhythmic echo, ambience, and transition tails by role.
- Use predelay and damping to protect intelligibility.
- Automate Sends or wet values for throws; avoid washing the complete source continuously.
- Verify tail policy at section boundaries and render end.

## Automation and modulation

Use Automation for intentional Song movement and Modulation for reusable continuous motion. Inspect exact effective values and avoid simultaneous changes that merely increase loudness.

Common mix movement:

- Send throws on selected words/hits;
- reverb/delay swells;
- filter reveals;
- sidechain depth changes;
- stereo movement above a stable mono low end;
- processor bypass/order transitions; and
- Master transition effects with exact step boundaries.

## Mastering

Master only after the mix survives low-volume, mono, and section-to-section review.

A Master chain may include corrective EQ, controlled saturation/clip, glue dynamics, stereo policy, and final limiting. Every stage must have a stated role and level-matched bypass result.

Report measurements; do not turn them into an automatic quality score. Distribution targets are explicit Project/delivery policy rather than hidden normalization.

## Stems and delivery

Before rendering, declare:

- exact Active Revision and range;
- tail policy;
- Master format/rate/depth/dither;
- requested Track/Group/Return stems and taps;
- downstream processing inclusion;
- alignment and latency padding;
- naming/collision policy; and
- analysis and hashes.

Verify finalized files, not only in-memory buses. Use [mix workflows](references/mix-workflows.md) for repeatable production patterns.
