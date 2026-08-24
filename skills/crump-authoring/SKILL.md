---
name: crump-authoring
description: Author and revise Crump Musical Sketch projects through readable .crump source and the schema-versioned crump CLI. Use for arranging Patterns and Clips, building native synthesis graphs, automating supported Parameters, adjusting balance, diagnosing validation failures, publishing Agent-authored revisions with provenance, comparing History, and rendering deterministic masters.
---

# Crump Authoring

Work beside Crump through Project files and `crump --json`; never scrape or automate the terminal UI. Crump contains no model runtime and grants no hidden editing path.

## Authority and scope

1. Read the target Project's `project.crump` and every declared `import` before changing it.
2. Read the installed Project Language and CLI guides (`docs/language.md` and `docs/cli-and-agents.md` in a source checkout; `/usr/share/doc/crump/docs/language.md` and `/usr/share/doc/crump/docs/cli-and-agents.md` in the Debian package).
3. For production-recipe requests, use the shared cookbook matrix and linked page (`docs/cookbook/feasibility.json` and `docs/cookbook/README.md` in a source checkout; `/usr/share/doc/crump/docs/cookbook/` in the Debian package). Do not maintain a private recipe or support list in this skill.
4. Use documented current behavior: portable sampler Instruments, the compiled mix graph and processors, expressive native devices, checked Clip/Locator/arrangement operations, Groove Templates, Pattern Variations, Pattern Parameter Locks, point Automation, routed LFO Modulation, and the Envelope Follower. Do not invent plug-ins, recording, Media Tracks, undocumented warp modes, formant preservation, realtime routed pitch, other routed control sources, or Rust extensions.
5. Source is authoritative. IDs are durable references, physical units are explicit, graph Buses are typed, and bounds are enforced.

## Cookbook recipe preflight

Before applying a named or analogous production recipe:

1. Resolve the exact row in `feasibility.json`; use its recipe ID to avoid confusing duplicate titles such as the two **EZ Chops** chapters.
2. Confirm the Project material and intended owner match the row's `source_assumptions`. A recipe for a vocal, sample, native Instrument, drums, Track, Return, Group, or Master is not automatically transferable to another source kind.
3. For `direct`, open the linked cookbook page when present and author its explicit Project changes through the normal source path. An unlinked `direct` row is feasibility evidence only: do not invent its ordered procedure; require explicit musician instructions.
4. For `approximation`, state the missing direct behavior and proposed workaround before editing, and continue only when the musician accepts that difference.
5. For `blocked`, stop and report the row's `missing_primitives`; do not invent a plug-in, transform, setting, generated asset, or unsupported command.
6. For `intentional-non-goal`, explain the product boundary and offer only a genuinely different supported task, such as importing an already owned recording.
7. Check `minimum_crump_version`, follow the page's audition/A-B and gain-staging procedure, and ask the musician to judge the level-matched result. Recipe support never establishes proprietary-device equivalence or musical quality.

The matrix owns support metadata; linked pages own recipe actions and listening checks. This skill owns only safe orchestration, validation, publication provenance, and honest diagnostics handling.

## Safe authoring loop

Set the Project directory explicitly on every command:

```bash
crump --project /path/to/project --json status
crump --project /path/to/project --json inspect --at 0bar
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
crump --project /path/to/project --json edit automation-create lead-cutoff \
  instrument/lead/cutoff 800hz --summary "Add the dark filter start"
crump --project /path/to/project --json edit automation-point-create \
  lead-cutoff open 4bar 8khz --curve 'power(2)' --summary "Open the filter into the lift"
crump --project /path/to/project --json inspect --at 2bar
```

For structural work, edit the smallest relevant `.crump` ranges directly. Preserve unrelated comments, whitespace, declaration order, and literals. Then run:

```bash
crump --project /path/to/project --json validate
crump --project /path/to/project --json check
crump --project /path/to/project --json status
```

`validate` proves syntax, references, units/domains, graph topology/types, and resource limits. `check` proves objective Musical Sketch shape. Neither proves musical taste, expressive range, control quality, or sound quality; ask the musician to listen.

## Sample transport, bounce, and preparation

Reverse traversal, bounded loops, deterministic slice maps, and exact-range bounce are ordinary source-backed operations. Inspect the asset's verified frame count and the target occurrence before issuing them:

```bash
crump --project /path/to/project --json edit sample-direction phrase reverse-word reverse \
  --summary "Reverse the selected word"
crump --project /path/to/project --json edit sample-loop phrase pad-bed 9600 38400 \
  --crossfade 256 --summary "Loop the stable vocal tail"
crump --project /path/to/project --json edit sample-loop-clear phrase pad-bed \
  --summary "Restore mapping loop inheritance"
crump --project /path/to/project --json edit sample-slice-map sampler stem chops C5 \
  --boundaries 0,12000,24000,36000 --direction reverse \
  --summary "Map reviewed stem boundaries"
```

A loop must remain inside the occurrence or mapping's half-open source region, and its crossfade cannot exceed half the loop. Reverse changes traversal only; it does not stretch time. Slice boundaries are exact original-source frames and must be reviewed rather than guessed.

Bounce only when `status` shows the Candidate equals the exact Active Revision. It renders the named Master or `OWNER:TAP`, publishes a new generated WAV and readable provenance atomically, and never places or replaces a Clip:

```bash
crump --project /path/to/project --json asset bounce \
  --source track/vocal:post_fader --from verse --to chorus --tail-seconds 1.5 \
  --id vocal-tail --name "Vocal tail" --origin agent --actor "$AGENT_NAME" \
  --summary "Render the approved wet vocal tail" \
  --explanation "Create a reversible generated asset for audition; retain every source Clip and processor"
```

After bounce, inspect the generated hash, source Revision, owner/tap, range, tail, algorithm, and build. Audition the asset raw and in context. Place it only through a separate checked edit after the musician chooses it.

For independent offline duration and pitch, use only the versioned prepared-asset path, again from an exact Active Candidate:

```bash
crump --project /path/to/project --json asset prepare --source phrase \
  --from-frame 0 --to-frame 48000 --output-frames 96000 \
  --pitch-semitones 0 --formants shift --quality effect-v1 \
  --id slowed-phrase --name "Slowed phrase" --origin agent --actor "$AGENT_NAME" \
  --summary "Prepare a two-second phrase" \
  --explanation "Create an exact-duration effect-quality asset while retaining the original bytes"
```

Treat `output-frames` as authoritative; calculate it explicitly from the intended duration and Project sample rate. The available algorithm is `crump-wsola-effect-v1` in deterministic domain `x86_64-linux`, bounded to ±24 semitones, with formants `shift` only. `preserve`, named DAW warp modes, realtime Track/Group pitch Automation, and cross-platform byte identity are unsupported. Inspect source asset/hash/range, output frames, pitch, policy, quality, algorithm, domain, build, and output hash. Preparation never places or replaces a Clip; audition and use a separate checked edit.

Never describe reverse, loop, slice map, bounce, or preparation as formant preservation, a routed pitch shifter, destructive freeze, automatic replacement, or proprietary warp equivalence.

## Audio-derived movement and bounded repeat

Use `gate` for explicit program- or sidechain-driven cleanup/rhythmic opening. Name at most one sidechain input and route, then set detector, threshold, hysteresis, hold, attack, release, range, ratio/mode, and mix in physical units. Do not substitute Compressor threshold behavior for Gate semantics.

An Envelope Follower must be authored as the documented `follower` source directive. It is additive physical-unit Modulation: frame N Audio affects frame N+1 controls. Confirm source/tap and destination `modulatable` capability, use finite floor below ceiling, inspect normalized/contribution telemetry, and never promise same-frame feedback.

Use `stutter` only when explicit repeated Events are not clearer. `slice` and `interval` are structural exact musical durations; `interval >= slice`, repeats are finite, crossfade is at most 50%, and `trigger` is rising-edge. Seeded chance is absolute-grid deterministic. Warm-up and active-repeat triggers are ignored and reported; they do not restart or queue. This is preceding-audio capture, not lookahead or proprietary transient warping. Ask the musician to judge the result; deterministic timing does not establish musical usefulness.

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
# When the human requests delivery stems:
crump --project /path/to/project --json render --output render/delivery \
  --stem track/lead:post_fader --stem group/music:post_fader --plan
```

Use `--no-replace` when an existing destination must be treated as a conflict rather than replaced. Repeat `--stem OWNER:TAP` only for requested `track/ID` or `group/ID` owners. Read the documented policy: pre-fader includes upstream routing and owner processing; post-fader also includes owner gain/panner/Mute; neither includes outgoing Sends or downstream Group/Return/Master processing. All files share the Master range and frame count and receive disclosed latency-alignment padding. Do not promise that an arbitrary incomplete stem selection sums to the processed Master.

Report `rendered_active_revision`, Candidate/Active match state, frame count, duration, each artifact's finalized-WAV `analysis` metrics (sample/true peak, available loudness/LRA, DC, clipping, and non-finite count), per-file WAV hashes, manifest hash, processing/alignment policy, and output paths. Do not claim identical playback/render audio from a render performed while the intended Candidate is unpublished.

When offline workload measurement is explicitly requested, plan an exact range before running it:

```bash
crump --project /path/to/project --json qualify --quantum 128 --from drop --to outro --plan
crump --project /path/to/project --json qualify --quantum 128 --from drop --to outro \
  --output evidence/drop-q128.json
```

Treat this as an offline profile only. Report its `passed`, p99.9, missed-deadline, range, quantum, and build/resource identities, but preserve `pipewire_xruns: null` and the listed external evidence. Never infer target-hardware behavior, realtime safety, or 30-minute endurance behavior from `qualify`.

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
