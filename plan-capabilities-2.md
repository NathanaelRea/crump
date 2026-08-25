# Capability Plan 2: close the remaining producer-workflow gaps

Status: implementation plan based on the current working tree, not a declaration that the capabilities are already accepted.

## Purpose

Crump already has enough source-backed primitives to construct most of the 100 audited producer effects. The next step is not 100 dedicated plugins. It is to stabilize the synthesis work already present, add a small set of deep reusable production modules, and attach recipe-level evidence to every support claim.

The project-local skills under `.agents/skills/` describe the complete intended production workflow. This plan remains factual about implementation state. A skill description is never evidence that a parser, validator, compiler, runtime, fixture, or listening path exists.

## Current audit

The tracked cookbook matrix remains:

| State | Count |
| --- | ---: |
| Direct | 64 |
| Approximation | 21 |
| Blocked | 14 |
| Intentional non-goal | 1 |

A strict current-working-tree audit, including uncommitted source-v2 work, is provisionally:

| State | Count | Recipe IDs |
| --- | ---: | --- |
| Attested direct | 64 | Unchanged from the tracked matrix |
| Approximation | 17 | 3, 5, 31, 46, 47, 51, 57, 59, 72, 82, 90, 92, 95, 96, 97, 99, 100 |
| Blocked | 11 | 8, 19, 21, 24, 32, 34, 40, 65, 67, 70, 89 |
| In-flight/unverified | 7 | 12, 27, 43, 56, 62, 63, 93 |
| Intentional non-goal | 1 | 88 |

The provisional counts total 100 but must not replace `docs/cookbook/feasibility.json` until the large synthesis change is stabilized and each promoted recipe has executable evidence. Recipe 63 moved from blocked to in-flight after the deterministic spectral source landed; it still lacks a producer fixture and listening evidence.

Audit snapshot note: `cargo test --test cookbook` passed at an earlier snapshot. Later focused spectral test attempts could not compile while concurrent Instrument Module CLI edits changed `ModuleCommand` variants ahead of their command handlers. No new capability is promoted while that unrelated in-flight work prevents a current passing test result.

## Evidence required for a capability

A capability is accepted only when all applicable layers agree:

1. readable source model and stable IDs;
2. parser/formatter or checked CLI operation;
3. validation and actionable diagnostics;
4. compiler/runtime behavior, including latency, tails, hot replacement, and deterministic state;
5. focused unit/integration tests;
6. at least one runnable cookbook fixture for the claimed producer outcome;
7. language/cookbook/product documentation; and
8. a complete relevant test-suite result plus a named human listening question.

A reserved model name, a partial node, or an aspirational skill does not satisfy this gate.

## Do not rebuild capabilities that are already present

The next plan should not add another generic sampler, wavetable source, modulation delay, crusher, gate, bounded stutter, seeded random source, LFO foundation, granular **source**, or deterministic spectral **source** merely because stale matrix labels mention them. Reuse the source-v2 work and narrow the remaining gaps precisely.

## Phase 0 — stabilize and recount the source-v2 work

### 0.1 Bound and review the current change

- Inventory the modified and untracked source-v2 files without resetting unrelated work.
- Split review by model/parser, validation, compiler/runtime, and user-facing contract.
- Run formatting, LSP/static diagnostics, focused synthesis tests, and the complete repository suite.
- Record any nondeterminism, unsafe hot replacement, hidden state, latency, or tail regressions.

### 0.2 Qualify the seven in-flight recipes

Add runnable cookbook pages and fixtures for:

- 12 **LFO Synth**;
- 27 **Grainy Vocals**;
- 43 **Slow Stutters**;
- 56 **Glitches**;
- 62 **LFO Vibes**;
- 63 **Specter**; and
- 93 **Granulator 3**.

Each fixture must prove the complete source-to-Master construction, not merely node instantiation. Include deterministic rendering and a focused listening comparison. The Specter fixture must specifically prove exact vocal-asset preparation, the octave-layer construction, freeze/retrigger interpretation, fades, low-pass filtering, and the matched listening decision.

### 0.3 Repair stale matrix labels

After acceptance:

- recipe 51: replace `bounded-stutter` with variable stutter rate/shape Automation;
- recipe 56: remove `seeded-random-modulation`;
- recipe 82: remove `rhythmic-gate`, retaining the multiband limitation;
- recipes 27/93: remove the generic granular gap;
- recipe 40: narrow to routed grain-delay processing; and
- recipe 63: remove the generic spectral gap after a successful source-based fixture/listening pass, or narrow it to `routed-spectral-audio-processing` only if that pass proves the source topology insufficient.

Recompute README and JSON counts in one change and keep `tests/cookbook.rs` checking metadata/page/fixture consistency.

**Exit:** source-v2 passes the complete suite; all seven rows are either promoted with evidence or left honestly unverified.

## Phase 1 — establish transparent reusable Effect Modules

Add one source-level reusable Effect Module seam before adding many new processors.

The module interface should expose:

- typed main and auxiliary Audio/Control ports;
- a small Faceplate of physical-unit Parameters;
- internal routing and processor graph hidden behind that interface;
- declared latency, tail, channel, deterministic-state, and replacement behavior;
- exact-hash asset dependencies where applicable; and
- readable expansion/inspection rather than opaque preset blobs.

Compile modules through existing graph/processor/runtime abstractions. Do not introduce a second hidden DSP runtime. Use the deletion test: removing the module seam should force repeated topology and validation complexity back into every recipe.

Initial reusable modules should exercise different pressure points:

1. filtered tempo delay throw;
2. reverse-wet transition construction;
3. parallel dynamics/sidechain utility; and
4. one generated-asset workflow.

**Exit:** one module declaration can be reused across Projects, inspected from source, validated independently, hot-replaced safely, rendered deterministically, and controlled from a compact Faceplate.

## Phase 2 — high-leverage routed Audio processors

### 2.1 Realtime pitch and formant processor

Implement a Track/Group/Return/Master processor with:

- semitone/cents pitch control;
- independent formant preserve/shift policy;
- wet/dry and output gain;
- bounded quality/latency modes;
- mono/stereo phase policy;
- reported latency and replacement state; and
- automatable controls with explicit smoothing/latching law.

Use one processor seam for ordinary pitch layers, feedback pitch effects, and Master transitions rather than recipe-specific devices.

Target recipes: 5, 21, 31, 46, 47, 59, 67, and 70.

### 2.2 Vocoder/carrier-modulator processor

Add a typed auxiliary Audio contract distinct from a dynamics sidechain:

- explicit carrier and modulator owners/taps;
- band/resolution, attack/release, unvoiced contribution, formant, and output controls;
- channel, latency, and tail rules;
- cycle-safe routing diagnostics; and
- a reusable Instrument/Effect Module Faceplate.

Target recipes: 8, 24, and 34.

**Exit:** complete fixtures prove routed pitch on Track and Master, formant-aware vocals, and a vocoder with explicit source routing.

## Phase 3 — time/frequency and response processing

### 3.1 Routed granular processor

Keep this distinct from the exact-asset granular Instrument source. Add a bounded grain-delay processor for arbitrary routed Audio with:

- owned capture horizon;
- position/delay, size, density, window, spray, direction, pitch, feedback, wet/dry, and seed;
- deterministic capture/retrigger behavior;
- bounded memory and feedback;
- explicit latency/tail and publication replacement rules.

Target recipe: 40, plus routed variants of 27/93.

### 3.2 Spectral source qualification and conditional routed processing

ADR 0022 already implements bounded deterministic spectral peak analysis/resynthesis as an exact-asset Instrument source with scan, rate, freeze, pitch, frequency-region, transient, and phase controls. Reuse its accepted analysis/cache and resource-admission infrastructure; do not rebuild or rename that source.

First qualify recipe 63 with the existing source. Only if its producer fixture/listening pass fails, or broader routed use is independently accepted, add a separate routed spectral Audio processor with:

- typed Audio input and an owned bounded capture/buffering law;
- wet/dry plus freeze, smear/hold, shift, and output controls;
- declared FFT/window/hop and channel-link policy;
- latency, tail, deterministic state, and safe hot replacement; and
- validation against unsupported sizes, captures, and routes.

Target: recipe 63 source-based qualification first; routed spectral variants only when evidence requires them.

### 3.3 Convolution

Implement exact-hash impulse-response use with:

- import/preparation and provenance;
- rate/channel conversion policy;
- trim, normalization, predelay, wet/dry, and output;
- partition/latency policy;
- bounded or explicit tail handling; and
- deterministic render qualification.

Target recipe: 32 and reusable convolution spaces/textures.

**Exit:** each required processor has a focused runtime test, latency/tail test, module example, cookbook fixture, and matched listening comparison; no conditional spectral processor is added without evidence.

## Phase 4 — Timeline and prepared-transform completeness

### 4.1 Tempo and meter maps

Treat this as a cross-cutting model change, not a scalar Parameter. Update together:

- source syntax and validation;
- beat/bar/time conversion;
- Event and Clip scheduling;
- Automation evaluation;
- synchronized LFO/Delay/Stutter behavior;
- Locators, seek, loop, audition, bounce, and render ranges;
- fragmentary bars and meter boundaries; and
- History diff/restore semantics.

Target recipe: 65.

### 4.2 Prepared-transform policy

Extend prepared transforms with explicit:

- formant preserve/shift/independent adjustment;
- transient, tonal, texture, and deliberate artifact quality modes;
- phase/channel policy;
- exact output frame contract; and
- algorithm/version provenance.

Do not use proprietary warp-mode names as capability claims. Define Crump’s own audible and mathematical contracts.

Target recipes: 5, 19, 72, 89, and 95.

### 4.3 Long-tail policy

Decide whether the present 30-second bound is a product safety limit or a missing production requirement. If longer tails are accepted, retain bounded resource planning while allowing explicit Project/render tail budgets and deterministic termination.

Target recipe: 3.

**Exit:** tempo-map and transform fixtures verify exact frame/event alignment across seek, loop, bounce, and render.

## Phase 5 — checked generation, analysis, and dynamic sample control

### 5.1 Analysis and generation operations

Add checked operations that create reviewable source:

- transient candidate detection with exact-frame results;
- slice-map generation that always permits manual correction;
- seeded chop and Pattern generation;
- checked melodic shuffle/variation; and
- deterministic provenance/seed reporting.

Generated results must become ordinary readable Events, slices, or explicit lineage. Never hide them as runtime-only randomness.

Target recipes: 90, 92, 96, and 97.

### 5.2 Continuously variable sample transport

Specify separately from note-latched sample start state:

- active-voice loop start/end or length modulation;
- active-voice sample-offset/scan behavior;
- smoothing and bounds;
- forward/reverse/ping-pong edge behavior;
- crossfade and click prevention; and
- deterministic interaction with voice replacement.

Target recipes: 99 and 100.

**Exit:** fixtures demonstrate deterministic generated material and click-free active sample movement with exact effective-value inspection.

## Phase 6 — evidence-driven lower-priority additions

Only add these after listening proves existing primitives insufficient:

### Multiband dynamics and transient specialization

Design a bounded multiband dynamics module, stable crossover/recombination law, latency, detector behavior, and gain-reduction telemetry. Consider a transient shaper only when envelopes/compression/source editing cannot express the required outcome.

Target recipes: 82 and 97.

### Vinyl/crackle source or processor

Prefer an imported exact-hash noise/crackle asset unless a generated source provides meaningful reusable control. If implemented, expose deterministic dust/crackle/rumble/wow/flutter behavior without pretending to emulate a named product.

Target recipe: 57.

## Intentional non-goal

Recipe 88 depends on live microphone/capture input. Keep it an intentional non-goal unless Crump explicitly expands its source-first product contract. A prerecorded control asset is a supported alternative construction, but it is not direct support for the demonstrated live-input recipe.

## Recommended execution order

1. Stabilize source-v2 and qualify the seven in-flight rows.
2. Add the transparent Effect Module seam.
3. Implement routed pitch/formants and vocoder.
4. Implement routed granular and convolution processors; add routed spectral processing only when producer evidence requires it.
5. Add tempo maps and complete prepared-transform policies.
6. Add checked analysis/generation and active sample transport.
7. Add multiband/transient/vinyl capabilities only after evidence demands them.
8. Leave live microphone input out of the 100/100 target.

## Completion definition

This plan is complete when:

- every one of the 100 recipes has an honest state and precise missing labels;
- every direct recipe has a runnable fixture or an explicitly documented shared-fixture rationale;
- cookbook JSON, README counts, recipe pages, fixtures, and tests agree;
- reusable modules cover repeated topology without opaque recipe-specific plugins;
- new DSP reports latency, tails, deterministic state, provenance, and effective controls;
- the complete suite passes; and
- human listening decisions are recorded separately from machine qualification.
