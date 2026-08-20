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
crump --project ./song --json inspect
crump --project ./song --json check
```

- `status` succeeds even for an invalid Candidate and reports Candidate/Active identity and state.
- `validate` exits `2` when blocking diagnostics exist.
- `inspect` returns the recoverable typed Candidate model, source-file inventory, diagnostics, and revision state.
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
```

These commands:

- load one exact Candidate revision;
- resolve a stable domain ID and source value site;
- verify the expected original literal;
- replace only that literal;
- validate the complete staged source set;
- atomically replace files; and
- publish one append-only History Entry.

They fail instead of rebasing when source changed concurrently. Values are CLI base-unit numbers (`3200` hertz, `-8` dB, `-0.15` pan); workspace exact entry accepts unit suffixes.

Arbitrary graph topology, arrangement construction, and multi-file edits may be made directly in source. The Agent must then run `validate`, inspect diagnostics, and call `publish`; file writes alone never alter Active playback.

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
- immutable source snapshot.

Agent work is not silently elevated. A running workspace identifies a valid unpublished Candidate, shows Agent-origin entries in History, and lets the musician inspect exact diffs or restore any state.

## Candidate and Active discipline

The filesystem is the Candidate. Playback and rendering load exact Active snapshots from `.crump/history`, not current unchecked files.

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

Restore publishes the selected exact snapshot as a new entry. It does not erase later work.

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
```

`--plan` is non-mutating and reports the exact Active render range, destination state, and whether publication would replace or conflict. Rendering reports Candidate identity, rendered Active identity, whether they match, and the complete manifest. If the Candidate differs or is invalid, the exact prior Active Revision is still rendered. `--no-replace` turns an existing destination into a conflict instead of replacing it.

## Security boundary

Crump source is declarative data, but external Agents are separate processes with the operating-system authority granted by their launcher. Crump does not sandbox an Agent or arbitrary editor. Use normal account, filesystem, network, and secret controls.

Crump itself constrains imported source to canonical paths under its declared Project or personal-library root, rejects traversal and symlink escape, validates all domain references, uses bounded native graphs, and never evaluates arbitrary code. It does not execute text from summaries or explanations.
