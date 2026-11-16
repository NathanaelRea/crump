# Crump project skills

These project skills live under `.agents/skills/` for agents that discover skills recursively. Each skill has one production responsibility so agents load focused guidance instead of one oversized instruction bundle.

| Skill | Responsibility |
| --- | --- |
| [`crump-project-workflow`](crump-project-workflow/SKILL.md) | Source authority, Candidate/Active state, validation, publication, History, audition, rendering, and completion reports |
| [`crump-instrument-design`](crump-instrument-design/SKILL.md) | Native synthesis, sampler/wavetable/multisample/granular/spectral Instruments, modulation, voicing, and Faceplates |
| [`crump-sample-production`](crump-sample-production/SKILL.md) | Import, slicing, reverse, looping, stretching, pitch/formants, bounce, resampling, vocoder, and audio-derived workflows |
| [`crump-song-arrangement`](crump-song-arrangement/SKILL.md) | Patterns, Clips, Grooves, deterministic generation, tempo/meter structure, builds, drops, and transitions |
| [`crump-mixing-and-delivery`](crump-mixing-and-delivery/SKILL.md) | Routing, processing, Automation, sidechains, mastering, stems, analysis, and delivery |
| [`crump-production-recipes`](crump-production-recipes/SKILL.md) | Cross-domain producer effects, vocal treatments, ear candy, and cookbook-style techniques |

`crump-project-workflow` owns the durable editing and publication contract. Every specialist skill loads it before changing a Project rather than duplicating those rules.

The skills describe Crump's complete production interface. Skill text is not capability acceptance evidence.
