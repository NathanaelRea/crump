---
name: crump-authoring
description: Author and revise Crump Musical Sketch projects through readable .crump source and the schema-versioned crump CLI. Use for arranging Patterns and Clips, building native synthesis graphs, adjusting Parameters and balance, diagnosing validation failures, publishing Agent-authored revisions with provenance, comparing History, and rendering deterministic masters.
---

# Crump Authoring

Crump is agent-ready, not agent-owning. Work beside the application through Project files and `crump --json`; never scrape or automate the terminal UI. Crump contains no model runtime and grants no hidden editing path.

## Authority and scope

1. Read the target Project's `project.crump` and every declared `import` before changing it.
2. Read the installed Project Language and CLI guides (`docs/language.md` and `docs/cli-and-agents.md` in a source checkout; `/usr/share/doc/crump/docs/language.md` and `/usr/share/doc/crump/docs/cli-and-agents.md` in the Debian package).
3. Preserve the first-release scope: a 30–60 second, no-sample electronic Musical Sketch with native graph synthesis and/or stock synthesized percussion. Do not invent unsupported samples, plugins, recording, automation, advanced routing, or Rust extensions.
4. Source is authoritative. IDs are durable references, physical units are explicit, graph Buses are typed, and bounds are enforced.

## Safe authoring loop

Set the Project directory explicitly on every command:

```bash
crump --project /path/to/project --json status
crump --project /path/to/project --json inspect
crump --project /path/to/project --json validate
```

Each command writes exactly one JSON value to stdout. Check both the process exit code and top-level `ok`; never infer success from prose.

For one supported literal change, prefer a checked operation:

```bash
crump --project /path/to/project --json edit parameter lead cutoff 2400 \
  --summary "Soften lead brightness"
crump --project /path/to/project --json edit track-level lead -8 \
  --summary "Bring lead forward"
crump --project /path/to/project --json edit track-pan lead -0.15 \
  --summary "Move lead slightly left"
crump --project /path/to/project --json edit note-pitch lead_a 2 F4 \
  --summary "Resolve the phrase downward"
crump --project /path/to/project --json edit event-time lead_a 2 3beat \
  --summary "Delay the response by one beat"
crump --project /path/to/project --json edit note-duration lead_a 2 1/2beat \
  --summary "Shorten the response"
crump --project /path/to/project --json edit event-velocity lead_a 2 0.7 \
  --summary "Soften the response"
crump --project /path/to/project --json edit hit-voice drums_a 3 clap \
  --summary "Change the final snare to a clap"
```

For structural work, edit the smallest relevant `.crump` ranges directly. Preserve unrelated comments, whitespace, declaration order, and literals. Then run:

```bash
crump --project /path/to/project --json validate
crump --project /path/to/project --json check
crump --project /path/to/project --json status
```

`validate` proves syntax, references, units/domains, graph topology/types, and resource limits. `check` proves objective Musical Sketch shape. Neither proves musical taste, expressive range, control quality, or sound quality; ask the musician to listen.

## Publication boundary

A valid disk state is only a Candidate. Do not claim a change is Active until explicit publication succeeds:

```bash
crump --project /path/to/project --json publish \
  --origin agent \
  --actor "$AGENT_NAME" \
  --summary "Add a contrasting middle section" \
  --explanation "Reused the bass motif, raised the lead register, and opened the filter for four bars so the musician can compare a clear B section"
```

The explanation is mandatory for Agent origin. Make it concrete enough for a musician to understand intent and compare the parent state. One intended save/publication is one action-level History Entry; do not publish every keystroke.

Invalid or unpublished source must remain Candidate-only. Never modify `.crump/history/`, `.crump/state.json`, snapshots, or transaction markers by hand. Never use Git commits or branches as a substitute for Crump History.

## Diagnose and recover

Exit classes are stable:

| Code | Meaning | Response |
| --- | --- | --- |
| `0` | Success | Continue |
| `1` | Operational failure | Inspect `error.code` and `error.message`; fix environment/I/O |
| `2` | Invalid or blocked request | Repair diagnostics or command input |
| `3` | Conflict | Reload identity/state; do not overwrite blindly |

On validation failure:

1. Read every diagnostic, not only the first.
2. Use `file`, `line`, `column`, stable code, and help text to locate exact source.
3. Repair the smallest coherent set.
4. Rerun `validate`; do not publish while any error remains.
5. Confirm `status` still names the prior Active Revision.

For comparison and recovery:

```bash
crump --project /path/to/project --json history list
crump --project /path/to/project --json history show ENTRY_ID
crump --project /path/to/project --json history diff ENTRY_ID
crump --project /path/to/project --json history restore ENTRY_ID \
  --actor "$AGENT_NAME" --explanation "Restore the musician-approved balance"
```

Restore creates a new entry; it never rewrites earlier history.

## Render for review

Render always consumes the exact Active Revision, never an unpublished Candidate. Preview the finite range and destination consequences without writing first:

```bash
crump --project /path/to/project --json render --output render/agent-review --plan
crump --project /path/to/project --json render --output render/agent-review
```

Use `--no-replace` when an existing destination must be treated as a conflict rather than replaced.

Report `rendered_active_revision`, Candidate/Active match state, frame count, duration, peak, WAV hash, manifest hash, and output paths. Do not claim identical playback/render audio from a render performed while the intended Candidate is unpublished.

## Instrument Modules

Use only validated declarative Modules:

```bash
crump --project /path/to/project --json module export lead --name warm-lead
crump --project /path/to/project --json module list
crump --project /path/to/project --json module inspect warm-lead
```

Do not hand-edit the personal library during a transaction. Installation/export uses validated atomic replacement and rejects an existing name unless the human explicitly permits replacement.

## Completion report

Return a concise record containing:

- Candidate and Active revision IDs;
- files and durable IDs changed;
- validation and Musical Sketch check results;
- publication History Entry ID and rationale;
- render paths/hashes when rendered;
- any unresolved diagnostics, conflicts, or unsupported requests; and
- an explicit request for musician listening where taste or playability is at issue.
