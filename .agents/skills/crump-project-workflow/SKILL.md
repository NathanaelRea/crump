---
name: crump-project-workflow
description: Safely inspect, edit, validate, publish, compare, restore, audition, and render Crump Projects through readable .crump source and the JSON CLI. Use for any task that changes a Crump Project, generated asset, Active Revision, or History.
compatibility: Crump complete-production CLI and readable Project Language.
---

# Crump Project Workflow

This is the foundational skill for durable Crump work. Specialist skills own musical and technical decisions; this skill owns how every change enters Authoritative Source safely.

## Authority

Before changing anything:

1. Read the target `project.crump` and every declared import.
2. Read [`docs/language.md`](../../../docs/language.md) and [`docs/cli-and-agents.md`](../../../docs/cli-and-agents.md).
3. Treat Project source and declared exact-hash assets as authoritative.
4. Treat runtime state, compiled graphs, meters, previews, and generated caches as disposable derivatives.
5. Use stable durable IDs, physical units, explicit routing, and exact musical time.

Never scrape or automate the terminal UI. Humans, Production Views, and Agents use the same Project model and CLI publication seam.

## Candidate-to-Active loop

### 1. Inspect

Run commands with an explicit Project path and JSON output:

```bash
crump --project /path/to/project --json status
crump --project /path/to/project --json inspect --at 0bar
crump --project /path/to/project --json validate
```

Check both process exit status and top-level `ok`. Record Candidate and Active Revision IDs before editing.

### 2. Plan the smallest coherent change

Name:

- durable objects to add or change;
- source files and ranges involved;
- expected routing, control, latency, and tail consequences;
- audition range and comparison method; and
- whether the operation generates an asset.

Prefer a checked edit for one supported literal or structural operation. For direct source editing, preserve unrelated comments, whitespace, declaration order, IDs, and literals.

### 3. Validate the complete Candidate

```bash
crump --project /path/to/project --json validate
crump --project /path/to/project --json check
crump --project /path/to/project --json status
```

Read every diagnostic. Repair the smallest coherent set and rerun validation. Invalid source must never replace the prior Active Revision.

### 4. Audition and compare

Audition the material at the depth that answers the question:

- raw asset;
- sampler mapping or prepared asset;
- Track pre/post-fader;
- Group or Return;
- complete Master route.

Use exact time or Locator ranges and looping. Use transient bypass, Solo, and cue only for monitoring comparisons. They must not alter source, History, or rendering.

Level-match effect comparisons. A louder result is not automatically a better result.

### 5. Publish one intended action

```bash
crump --project /path/to/project --json publish \
  --origin agent \
  --actor "$AGENT_NAME" \
  --summary "Concise musical change" \
  --explanation "What changed, why, and what the musician should compare"
```

An Agent explanation is mandatory. One intended save is one History action; do not publish every keystroke.

### 6. Verify

After publication:

- confirm the expected Active Revision;
- inspect changed durable objects and effective controls;
- repeat the intended audition;
- render only from the exact Active Revision; and
- ask the musician to listen whenever taste, expression, or balance is involved.

## Generated assets

Bounce, preparation, analysis, convolution preparation, spectral preparation, and other offline transforms must:

- consume an exact Active Revision or verified source asset;
- declare source owner/tap/range and algorithm/version;
- retain the original source;
- produce a Project-owned exact-hash asset;
- publish bytes, provenance, and source atomically; and
- require a separate checked placement or replacement decision.

Never silently freeze, flatten, replace Clips, disable processors, or overwrite the only source.

## History and recovery

Use Crump History rather than Git as the musician-facing undo/recovery mechanism:

```bash
crump --project /path/to/project --json history list
crump --project /path/to/project --json history show ENTRY_ID
crump --project /path/to/project --json history diff ENTRY_ID
crump --project /path/to/project --json history restore ENTRY_ID \
  --actor "$AGENT_NAME" --explanation "Restore the musician-approved state"
```

Restore creates a new entry. Never hand-edit `.crump/history/`, state files, snapshots, locks, or transaction markers.

## Rendering and delivery

Plan renders before writing them. Report:

- rendered Active Revision;
- exact range and tail policy;
- sample rate, format, and frame count;
- Master and requested stem paths/taps;
- WAV and manifest hashes;
- sample peak, reconstructed peak, available loudness/LRA, DC, clipping, and non-finite counts; and
- any deterministic-domain or external-device limitation.

Do not claim that arbitrary stems sum to a processed Master. Do not infer realtime hardware acceptance from offline qualification.

## Qualification and support evidence

For native-synthesis qualification, follow [`docs/qualification/native-synthesis.md`](../../../docs/qualification/native-synthesis.md). Keep three evidence classes separate:

- repository checks and offline reports establish only their named deterministic, resource, finite-output, callback-allocation, portability, or recovery facts;
- an independent Experienced Producer owns listening, workflow, and credibility judgments; and
- a named Linux/PipeWire/device endurance run owns xrun, callback-margin, and supported-hardware facts.

Never fill an independent-producer or hardware attestation on someone else's behalf. A blank template, `play --plan`, deterministic render, or fast offline `qualify` report is not producer or named-device acceptance. Preserve failed observations and leave the applicable support gate pending.

## Error response

| Exit | Meaning | Response |
| ---: | --- | --- |
| `0` | Success | Continue and inspect the returned data. |
| `1` | Operational failure | Repair environment or I/O; do not rewrite source blindly. |
| `2` | Invalid or blocked request | Read diagnostics and repair the request or Candidate. |
| `3` | Revision conflict | Reload current identity/state; never overwrite blindly. |

## Completion report

Use [the completion template](references/completion-report.md). At minimum report revisions, durable IDs/files changed, validation/check results, publication entry, audition/render evidence, unresolved issues, and the requested listening decision.
