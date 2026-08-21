# CLI and External Authoring Agents

Crump does not embed a model, provider SDK, prompt runtime, network client, or autonomous background editor. An external Authoring Agent works at the same boundary as any editor or script:

1. read ordinary `.crump` source;
2. inspect the typed model and revision with `crump --json`;
3. write a Candidate directly or invoke a checked structured command;
4. validate the complete Candidate;
5. explain and explicitly publish accepted work; and
6. leave a durable Agent-origin History Entry for human inspection.

The core remains fully usable without a model. A provider-neutral installable instruction bundle lives at [`skills/crump-authoring/SKILL.md`](../skills/crump-authoring/SKILL.md) and is installed by the Debian package at `/usr/share/crump/skills/crump-authoring/SKILL.md`; copy that directory into a harness's normal skills location or provide the file as authoritative task context. It adds no executable integration or privilege.

## Stable response envelope

Every non-interactive command accepts the global `--json` option and writes exactly one compact JSON value followed by a newline to stdout. Success:

```json
{"schema_version":1,"ok":true,"command":"status","data":{}}
```

Failure:

```json
{"schema_version":1,"ok":false,"command":"publish","error":{"code":"invalid_candidate","message":"...","diagnostics":[]}}
```

Consumers must:

- require the supported `schema_version` before interpreting command data;
- branch on `ok`, then on the stable command/error code;
- use revision IDs as opaque SHA-256 identity tokens;
- tolerate additional object members in schema version 1; and
- use process status as the coarse class, not parse human stderr.

The Ratatui `workspace` is interactive and rejects `--json` before emitting terminal control bytes.

## Exit classes

| Status | Meaning |
| ---: | --- |
| `0` | Command completed successfully |
| `1` | Operational failure: I/O, PipeWire, compilation, render, or host failure |
| `2` | Request blocked or invalid: diagnostics, unsupported value, missing rationale, or acceptance failure |
| `3` | Conflict: stale checked edit, existing destination, or already-Active publication |

Clap usage errors also use status `2`. With `--json`, usage errors and help/version requests use the same single-response JSON discipline (`usage_error` for invalid syntax); without it, Clap writes normal human-facing help. Agents should construct commands from `--help`, not probe invalid syntax.

## Read and inspect

```bash
crump --project ./song --json status
crump --project ./song --json validate
crump --project ./song --json inspect --at 2bar
crump --project ./song --json check
```

- `status` succeeds even for an invalid Candidate and reports Candidate/Active identity and state.
- `validate` exits `2` when blocking diagnostics exist.
- `inspect` returns the recoverable typed Candidate model, source-file inventory, verified portable-media metadata/hashes, diagnostics, revision state, and effective Parameter provenance at `--at TIME` (default Song time zero) when the Candidate is valid. The requested frame must lie at or before the inclusive Render Range boundary; out-of-range requests are rejected rather than silently clamped.
- `check` applies the narrower objective 30–60 second Musical Sketch acceptance fixture.

Diagnostics include stable code, severity, source location, byte range, message, and optional help. An Agent should report those facts rather than replacing them with a vague parse failure.

## Checked edits

Prefer supported semantic commands for routine Production View mutations:

```bash
crump --project ./song --json edit parameter lead cutoff 3200 \
  --summary "Open lead filter"

crump --project ./song --json edit track-level lead -8 \
  --summary "Raise lead one decibel"

crump --project ./song --json edit track-pan lead -0.15
crump --project ./song --json edit note-pitch melody 3 F#4
crump --project ./song --json edit event-time melody 3 3beat
crump --project ./song --json edit note-duration melody 3 1/2beat
crump --project ./song --json edit event-velocity melody 3 0.7
crump --project ./song --json edit hit-voice drums 7 clap

# Durable sample-occurrence operations use IDs, not presentation indexes.
crump --project ./song --json edit sample-place vocal breath phrase \
  2beat 1beat 8400 22800 --gain-db -3 --pan 0.15 --fade-in 32 --fade-out 64
crump --project ./song --json edit sample-move vocal breath 5/2beat
crump --project ./song --json edit sample-duplicate vocal breath breath-copy 3beat
crump --project ./song --json edit sample-split vocal breath-copy breath-tail 7/2beat 15600
crump --project ./song --json edit sample-trim vocal breath 9000 22000
crump --project ./song --json edit sample-slip vocal breath -240
crump --project ./song --json edit sample-fade vocal breath 48 96
crump --project ./song --json edit sample-gain vocal breath -4.5
crump --project ./song --json edit sample-pan vocal breath -0.2

crump --project ./song --json edit automation-create lead-cutoff \
  instrument/lead/cutoff 800hz
crump --project ./song --json edit automation-point-create \
  lead-cutoff open 4bar 8khz --curve 'power(2)'
crump --project ./song --json edit automation-point-time lead-cutoff open 9/2bar
crump --project ./song --json edit automation-point-value lead-cutoff open 7.5khz
crump --project ./song --json edit automation-point-curve lead-cutoff open linear
crump --project ./song --json edit automation-point-delete lead-cutoff open
crump --project ./song --json edit automation-delete lead-cutoff

# Named section positions and exact audition ranges.
crump --project ./song --json edit locator-create intro "Intro" 0bar
crump --project ./song --json edit locator-create build "Build" 8bar
crump --project ./song --json edit locator-move build 15/2bar --snap --grid 1bar
crump --project ./song --json edit automation-point-time lead-cutoff open build
crump --project ./song --json play --from intro --to build --loop-range --plan

# Reusable Groove Templates and placement-local Clip treatment.
crump --project ./song --json edit groove-create shuffle \
  1/2beat 0.625 1/32beat 0.05 0.95 41
crump --project ./song --json edit clip-create lead verse-b melody build 4bar \
  --mode loop --offset 1beat --transpose 7 --gain-db -3 \
  --fade-in 1/8beat --fade-out 1/8beat --groove shuffle
crump --project ./song --json edit clip-move verse-b 12bar --snap --grid 1bar
crump --project ./song --json edit clip-resize verse-b 8bar
crump --project ./song --json edit clip-duplicate verse-b return-b 24bar
crump --project ./song --json edit clip-split return-b outro-b 28bar
crump --project ./song --json edit clip-offset verse-b 2beat
crump --project ./song --json edit clip-transpose verse-b 12
crump --project ./song --json edit clip-gain verse-b -4.5
crump --project ./song --json edit clip-fade verse-b 1/4beat 1/2beat
crump --project ./song --json edit clip-groove verse-b shuffle
crump --project ./song --json edit clip-delete outro-b

# Procedural Pattern lineage and durable physical-unit Parameter locks.
crump --project ./song --json edit pattern-variation-create answer melody \
  1beat 7 0.8 false 0.9 77
crump --project ./song --json edit parameter-lock-create melody open 3/16beat \
  cutoff 4200hz --snap --grid 1/4beat

# Atomic arrangement-time transforms require an Automation movement policy.
crump --project ./song --json edit time-insert build 1bar --automation follow
crump --project ./song --json edit time-delete 16bar 1bar --automation locked
crump --project ./song --json edit time-duplicate 8bar 4bar 24bar reprise \
  --automation follow
```

These commands:

- load one exact Candidate revision;
- resolve a stable domain ID and source value site;
- verify the expected original literal;
- replace only that literal;
- validate the complete staged source set;
- atomically replace files; and
- publish one append-only History Entry.

They fail instead of rebasing when source changed concurrently. Ordinary Parameter/Track numeric commands use CLI base-unit numbers (`3200` hertz, `-8` dB, `-0.15` balance). Automation and Parameter-lock operations use explicit physical literals because the stable target determines the required unit. Lane creation always writes the required durable `start` point at Song time zero. Point creation inserts chronologically; a time edit that crosses neighbors relocates the whole durable directive. Value/curve edits and whole point/lane deletion remain checked source transformations.

Automation accepts every stable target path documented in the language guide: Instrument Parameters; Track, Group, and Return channel controls; Send level; processor settings; and Master level. `step`, `linear`, and finite-positive `power(P)` are supported. Validation rejects duplicate target lanes, point collisions/order errors, resource-changing targets, and values outside the target's physical domain. A Locator ID can replace an exact `TIME` operand in checked timing commands, including Automation-point creation/movement, so section labels remain durable while canonical source stores exact musical time.

Timing edits are exact and grid-free by default. `--free` makes that intent explicit; `--snap --grid TIME` uses exact rational snapping with forward ties. Clip, Locator, Automation, Event, sample-occurrence, arrangement-time, and Parameter-lock timing commands share this behavior. Clip operations preserve durable identity and procedural Pattern references. Split preserves loop phase and rejects unsafe boundaries; multi-field fades and multi-declaration arrangement transforms publish atomically.

`time-insert`, `time-delete`, and `time-duplicate` model Automation movement through `--automation follow|locked`; the CLI defaults to the visible `follow` policy. `follow` moves section Automation with transformed Timeline material; `locked` leaves points at absolute Song times. Boundary-crossing Clips must be split before deletion or duplication, and duplicated Clips receive the supplied ID suffix.

Groove Templates, Pattern Variations, and Parameter locks remain readable procedural source. Variations require all transform operands and cannot be deleted while referenced. A lock resolves its Parameter key against each consuming Track's Instrument, overrides Automation as a step value, still receives additive Modulation, and resets at the Clip end. Conflicting overlapping lock-bearing Clips are rejected rather than assigned hidden precedence. `inspect --at TIME` exposes the active `parameter_lock` value in effective-Parameter provenance.

Sample place, move, duplicate, split/chop, trim, slip, fade, gain, and pan commands resolve durable occurrence IDs and publish checked source transformations. Multi-literal region/fade edits commit atomically. The Event Grid also exposes exact duration, velocity, source-region (`x`), fade (`f`), gain (`g`), and pan (`p`) dialogs for sample occurrences.

Arbitrary graph topology and other unsupported multi-file transforms may be made directly in source. The Agent must then run `validate`, inspect diagnostics, and call `publish`; file writes alone never alter Active playback.

## Portable WAV assets

Portable media declarations feed sampler mappings and durable non-destructive sample occurrences. Media Tracks and time stretching remain out of scope.

```bash
crump --project ./song --json asset import ../recordings/phrase.wav \
  --id phrase \
  --name "Spoken phrase" \
  --summary "Import lawful vocal fragment"
crump --project ./song --json asset collect \
  --summary "Normalize portable media paths"
crump --project ./song --json asset list
```

`asset import` accepts an external regular WAV only as input. It captures exact bytes, computes lowercase SHA-256, chooses `media/ID.wav`, refuses to replace any existing destination, inserts the source declaration, validates the source-and-binary overlay, and publishes both sides under one recovery journal. Invalid WAVs, stale revisions, unsafe/symbolic-link paths, duplicate IDs/paths, and limit violations leave the declaration and destination unchanged. An external absolute path is never persisted.

`asset collect` moves every valid declaration to `media/ASSET_ID.wav` in one checked source-and-binary History transaction while preserving exact bytes and hashes. It refuses undeclared destination conflicts. `asset list` reports only verified Project-relative identity, exact content hash/length, and WAV container metadata. Missing or changed media remains visible through stable validation diagnostics. Distinct paths with identical content produce a warning rather than an implicit merge.

## Agent publication provenance

An Agent publication must name the actor and provide a non-empty explanation:

```bash
crump --project ./song --json publish \
  --origin agent \
  --actor arranger-v2 \
  --summary "Add contrasting middle section" \
  --explanation "Reused the lead motif one octave higher and changed the drum Pattern; no topology changes"
```

The stored entry records:

- monotonic sequence and entry ID;
- exact source revision;
- UTC timestamp;
- `agent` origin and actor name;
- concise summary and rationale;
- exact parent revision and per-file hashes/diff; and
- immutable source plus content-addressed media snapshot.

Agent work is not silently elevated. A running workspace identifies a valid unpublished Candidate, shows Agent-origin entries in History, and lets the musician inspect exact diffs or restore any state.

## Candidate and Active discipline

The Project source and declared media files on the filesystem form the Candidate. Playback and rendering load exact Active snapshots from `.crump/history`, not current unchecked files. Sampler Instruments decode/resample the preserved Active media off the callback; Media Track playback remains deferred.

A robust Agent loop is:

```text
status → inspect → retain candidate_revision
write or checked edit → validate → inspect diff/state
publish with provenance → verify returned active revision
```

If another writer wins the race, re-read source and intent. Do not overwrite metadata, delete `.crump`, or synthesize a replacement revision token.

An invalid Candidate may coexist with a valid Active Revision. This is intentional. Repair the Candidate or restore History; do not expect playback to reflect invalid bytes.

## Project History

```bash
crump --project ./song --json history list
crump --project ./song --json history show 000000000004
crump --project ./song --json history diff 000000000004
crump --project ./song --json history restore 000000000002 \
  --actor musician \
  --explanation "Return to the sparse arrangement"
```

Restore publishes the selected exact source-and-media snapshot as a new entry. It does not erase later work.

## Instrument Modules

```bash
crump module export lead --name neon-lead
crump module install ./shared/bass.crump --name pulse-bass
crump module list --json
crump module inspect pulse-bass --json
```

Module install/export validates before atomically replacing the destination. Names are restricted to ASCII letters, digits, `_`, and `-`. Project-local imports remain relative and source-readable; personal-library use is explicit as `import "library:name"`. Active snapshots contain the exact imported bytes, so later library changes do not alter playback or rendering of the committed revision.

## Rendering

```bash
crump --project ./song --json render --output render/master --plan
crump --project ./song --json render --output render/master
crump --project ./song --json render --output render/delivery \
  --stem track/lead:post_fader \
  --stem group/music:post_fader
```

`--plan` is non-mutating and reports the exact Active render range, destination state, requested stems, alignment padding, processing policy, and whether publication would replace or conflict. Repeat `--stem OWNER:TAP` with a durable `track/ID` or `group/ID` and `pre_fader`/`post_fader`; duplicate, missing, Return, or Master selections are rejected.

Every stem is stereo float WAV with the same start, frame count, tail, and sample rate as Master. Both taps include upstream main routing and processing through the selected owner's inserts. `post_fader` additionally includes that owner's gain, panner, and Mute. Neither tap folds in outgoing Sends or downstream Group, Return, or Master processing. Deterministic zero padding aligns the tap with compiled Master output latency, and the manifest records the exact delay and policy. Arbitrary selected stems are not claimed to reconstruct Master when omitted branches or downstream processing make that impossible.

Rendering reports Candidate identity, rendered Active identity, whether they match, and the complete manifest. Master and every stem carry independent hashes and nested finalized-WAV analysis. Each `analysis` object includes sample/true peak, integrated and maximum short-term loudness, loudness range, DC offset, clipping counts, non-finite count, analyzed frames, and algorithm identities; inapplicable loudness values are JSON `null`. If the Candidate differs or is invalid, the exact prior Active Revision is still rendered. `--no-replace` turns an existing destination into a conflict instead of replacing it.

## Offline workload qualification

```bash
crump --project ./song --json qualify --quantum 64 --from drop --to outro --plan
crump --project ./song --json qualify --quantum 64 --from drop --to outro \
  --maximum-p999-utilization 0.8 \
  --output evidence/drop-q64.json
```

`qualify` compiles the exact Active Revision, performs deterministic untimed preroll to the optional `--from` Locator, and wall-clock profiles the half-open range through the optional `--to` Locator. Without Locators it profiles the complete render range. The only admitted full-block quanta are 64, 128, and 256 frames. A non-aligned range ends with one disclosed `final_block_frames` short block; every block is evaluated against the deadline for the exact number of frames it processed. `--plan` is non-mutating: it reports the exact range, full/final block sizes, compile-time resource facts, threshold, output conflict consequence, and evidence that remains external without processing Audio.

A schema-1 report records Project Revision, package and executable identities, host OS/kernel/CPU facts, requested and actual **offline** full quantum, final block size, exact range, block count, wall time, realtime factor, mean/peak/fixed-histogram p99.9 utilization, missed deadlines, finite-output facts, sample peak, and compile-time admission resources. Histogram overflow uses the observed peak rather than understating the quantile. The optional report file uses no-replace publication by default; `--replace` must be explicit, and neither mode may target Project source, declared media, or `.crump` History. A failed utilization threshold is represented by `passed: false` in a successfully captured report so automation does not lose the evidence.

This command is an offline workload profiler, not a PipeWire qualification. It deliberately reports `pipewire_xruns: null` and lists required external evidence for the target graph, named hardware, 30-minute realtime endurance, and callback allocation/blocking/I/O audit. Do not convert a fast offline report into an xrun, realtime-safety, or supported-hardware claim.

## Security boundary

Crump source is declarative data, but external Agents are separate processes with the operating-system authority granted by their launcher. Crump does not sandbox an Agent or arbitrary editor. Use normal account, filesystem, network, and secret controls.

Crump itself constrains imported source and declared media to normalized Project paths, rejects traversal and symlink escape, validates exact media hashes and all domain references, uses bounded native graphs, and never evaluates arbitrary code. It does not execute text from summaries or explanations.
