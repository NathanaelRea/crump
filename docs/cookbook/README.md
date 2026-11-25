# Crump Production Cookbook

This cookbook turns production knowledge into readable Project actions that a musician or external Authoring Agent can perform through the same source and publication path. It does not add engine capabilities or claim sonic equivalence with named products.

## Canonical feasibility matrix

[`feasibility.json`](feasibility.json) is the self-contained machine-readable source of truth for recipe identity, current support, source assumptions, required capability gaps, and links. Its 100 rows retain the reviewed chapter identities from the [source video](https://www.youtube.com/watch?v=D5dc7mQ30hU). Local transcript research may help a reviewer re-audit those rows, but it is optional input rather than a packaged or test-time dependency.

The matrix contains:

| Support state | Count | Meaning |
| --- | ---: | --- |
| `direct` | 64 | Current Crump source and native processing can express the recipe without external preprocessing. |
| `approximation` | 21 | Current tools can approach the intent, but the demonstrated transform or workflow is not direct. |
| `blocked` | 14 | A missing primitive is central to the result. |
| `intentional-non-goal` | 1 | Recipe 88 requires unsupported live microphone input. |

`direct` does not mean one-click, source-independent, or sonically identical to Ableton or a named plug-in. It means the current engine has no known missing primitive for the stated source assumptions. Caption uncertainty remains uncertainty; recipes never repair an unclear control or value by invention.

## How to use a recipe

1. Open `feasibility.json` and select the exact recipe row.
2. Confirm the material matches every relevant source assumption. A native Instrument, imported sample, vocal phrase, drum source, Track, Return, Group, and Master can require different actions.
3. Apply a linked `direct` page through explicit source edits. An unlinked `direct` row is feasibility evidence, not an Agent procedure; author it only from the musician's explicit instructions. For `approximation`, explain the mismatch and obtain the musician's agreement before authoring. Stop on `blocked` or `intentional-non-goal` unless the musician chooses a different supported task.
4. Follow the page's gain staging and A/B instructions. Match apparent level before judging an effect.
5. Validate the Candidate, audition it, publish one intended change with provenance, render the exact Active Revision when useful, and ask the musician to listen.

No page grants hidden Agent authority. Human and Agent work both produce ordinary processors, routing, Automation, Clips, and assets in Authoritative Source.

## Direct recipes and fixtures

| Recipe | Primary coverage | Page | Runnable Project |
| --- | --- | --- | --- |
| 4 Bass Booster | Group, Return, Send, EQ, Saturation | [page](recipes/04-bass-booster.md) | [`fixtures/cookbook/04-bass-booster`](../../fixtures/cookbook/04-bass-booster/) |
| 10 Distortion Fun | Track processor order | [page](recipes/10-distortion-fun.md) | [`fixtures/cookbook/10-distortion-fun`](../../fixtures/cookbook/10-distortion-fun/) |
| 17 Bass Drop | step Automation of durable bypass | [page](recipes/17-bass-drop.md) | [`fixtures/cookbook/17-bass-drop`](../../fixtures/cookbook/17-bass-drop/) |
| 35 Hat Roll | portable sample origin, Events, EQ, Reverb, Delay | [page](recipes/35-hat-roll.md) | [`fixtures/cookbook/35-hat-roll`](../../fixtures/cookbook/35-hat-roll/) |
| 42 Reverb Swells | continuous processor Automation | [page](recipes/42-reverb-swells.md) | [`fixtures/cookbook/42-reverb-swells`](../../fixtures/cookbook/42-reverb-swells/) |
| 44 Double Delays | two Returns and tempo divisions | [page](recipes/44-double-delays.md) | [`fixtures/cookbook/44-double-delays`](../../fixtures/cookbook/44-double-delays/) |
| 48 Split Delays | complementary filtered Returns | [page](recipes/48-split-delays.md) | [`fixtures/cookbook/48-split-delays`](../../fixtures/cookbook/48-split-delays/) |
| 84 Club Toilet | Master EQ and Reverb | [page](recipes/84-club-toilet.md) | [`fixtures/cookbook/84-club-toilet`](../../fixtures/cookbook/84-club-toilet/) |

The [convolution hall recipe](recipes/convolution-hall.md) shows how to reuse a WAV impulse response on a filtered Return.

Fixtures are short runnable examples. They validate, compile, render deterministically, and exercise current source syntax. Tests publish only disposable copies; checked-in fixture directories contain no History state.

## Reuse boundary

Recipes use ordinary processor blocks and routing. Crump has no general Effect Module syntax or parallel Rack runtime.

## Installation

A source checkout keeps this index at `docs/cookbook/README.md`, the matrix at `docs/cookbook/feasibility.json`, and runnable examples under `fixtures/cookbook/`. The Debian package installs the same tree under `/usr/share/doc/crump/docs/cookbook/` and `/usr/share/doc/crump/fixtures/cookbook/`.
