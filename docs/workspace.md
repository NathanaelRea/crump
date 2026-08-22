# Production Workspace

Run `crump` with no command in an interactive terminal, or use `crump workspace`. Ratatui is the first-party client; the Project model, validator, history, engine, renderer, and real-time adapter do not depend on it.

The workspace watches Project source and history approximately twice per second. External file or Agent changes therefore appear without restarting the application.

## Revision header

The top line always distinguishes:

- **ACTIVE** — Candidate bytes equal the Active Revision.
- **VALID CANDIDATE** — disk source is valid but has not been published.
- **INVALID CANDIDATE** — disk source has blocking diagnostics; the prior Active Revision remains audible.

Short Candidate and Active hashes are shown side by side. During playback the same line shows the bar/beat playhead, PipeWire state, current monitor peak, active voice count, and MIDI connection indicator. An invalid external edit automatically opens Diagnostics; a valid unpublished edit prompts the musician to inspect and press `P`.

## Global keyboard map

| Key | Action |
| --- | --- |
| `1`–`9`, `0` | Select Timeline, Mixer, Processing, Event Grid, Faceplate, Parameters, Automation, Graph, Source, or Diagnostics |
| `H` / `?` | Open History / keyboard Help |
| `Tab` / `Shift-Tab` | Next / previous view |
| `↑` / `↓` | Move the current selection |
| `Page Up` / `Page Down` | Move by eight rows |
| `←` / `→` | Make the view's bounded adjustment or change the Source/Graph context |
| `[` / `]` | Previous / next Pattern, Instrument, or Automation lane where applicable |
| `Enter` | Open an exact-value editor or selected History diff |
| `Space` | Start, pause, or resume the armed Audition Plan against the exact Active Revision |
| `S` | Stop playback while retaining the armed target, range, path, transient bypasses, Solo, and cue |
| `A` in Timeline/Event Grid | Arm the selected Track, Pattern, or sample occurrence for audition |
| `L` in Event Grid | Arm or disable the selected Pattern's exact half-open loop |
| `P` | Publish an inspected valid Candidate with a History summary |
| `r` | Reload Project and History immediately |
| `e` in Source | Suspend the workspace and open the selected file in `$VISUAL`, `$EDITOR`, or `vi` |
| `?` | Open keyboard help |
| `q` / `Ctrl-C` | Exit safely |

Mouse wheel selection is available as a convenience. No required workflow needs a pointer.

## Timeline

Timeline lists presentation-ordered Tracks, assigned Instruments, source-backed level/pan, and Clip ranges. Arrangement context lists named Locators and reusable Groove Template IDs. Every Clip row keeps its durable ID, start/duration, traversal mode, source offset, transpose, gain, fades, and optional Groove assignment visible, making shared placement and local treatment distinguishable without opening source. The title displays the current playhead.

- `↑` / `↓` selects a Track and follows its Instrument in instrument views.
- `←` / `→` changes level by 1 dB through a checked source edit.
- `Enter` or `l` opens exact level entry.
- `p` opens exact pan entry.

Every accepted change replaces only the selected literal, validates the complete Project, writes atomically, and appends a human-origin History Entry. If source changed since the view was loaded, Crump rejects the operation rather than editing a stale range.

## Mixer

Mixer enumerates stable Track, Group, Return, Send, processor, and Master targets with exact physical base values, legal domains, and Automation/Modulation eligibility. Each target is associated with its owning channel where that owner is singular. During playback the table reports that channel's current peak and exponential average activity. `M/S/C` shows source-backed Mute, transient Solo, and transient cue state.

- `↑` / `↓` selects a stable target.
- `←` / `→` changes continuous values by 1% of the legal domain; boolean Mute selects `false`/`true`.
- `Enter` opens exact physical or boolean source-backed entry.
- `m` toggles the owning Track/Group/Return Mute through a checked source edit and History publication.
- `s` toggles transient Solo for the owning Track, Group, or Return.
- `c` toggles a post-fader cue; `C` toggles a pre-fader cue. Only one cue tap is active at a time.

Solo and cue are callback-safe Audition State owned by the same `AuditionPlan` as target, path, range, and processor overrides. They are retained across compatible workspace publications, affect monitoring only, and never alter source, History, or offline renders. A processor shared by multiple Instrument instances has no singular mixer channel for these commands.

## Processing and unified audition

Processing displays the ordered compiled processor route for the armed Audition Plan. The header keeps Active Revision, durable target, monitoring path, exact resolved half-open frame range, loop state, transport, and pause state visible. Each processor row shows owner and stable ID, kind, durable/transient/effective bypass, click-guard blend, input/output activity, current/maximum gain reduction, complete output latency, and every resolved setting. Bypassed processors continue processing silently and the dry path retains complete output latency.

- `A` enters `song`, `track/ID`, `clip/[TRACK/]ID`, `pattern/ID[/TRACK]`, `asset/ID[/START/END]`, `mapping/INSTRUMENT/MAPPING[/TRACK]`, or `occurrence/PATTERN/ID[/TRACK]`.
- `a` cycles paths valid for the target: raw asset, sampler mapping, selected pre/post-fader Track channel, or complete Master routing.
- `F` enters exact `FROM TO` Musical Time or Locator IDs; blank restores the target's natural bounds.
- `L` toggles the current exact half-open range loop.
- `b` toggles a monitoring-only transient bypass override. It changes neither source, Active identity, History, nor offline rendering.
- `B` toggles durable source-backed `bypass`; `Enter` edits an exact `KEY VALUE`; `K`/`J` moves the complete processor block one position earlier/later in the same owner chain. These checked operations validate, publish, and record History.

Target/range/path changes prepare and deterministically preroll a replacement runtime off the callback, then use the bounded publication transition. Compatible source publication retains the durable target, range, transient bypass/Solo/cue state, and relative transport frame when still valid. If a referenced ID or range disappears, the workspace reports the rejection and leaves the prior runtime audible.

## Event Grid

Event Grid shows one Pattern's source Events with exact onset, type, pitch/voice or durable sample identity/mapping, duration, and velocity. A separate procedural panel shows Pattern Variation lineage and every authored transform, or durable Parameter-lock ID/time/key/value rows for a concrete Pattern. A Variation remains visibly procedural and is never presented as flattened authored Events. Currently sounding note/sample ranges and recent hit windows turn green during playback.

- `[` / `]` selects a Pattern.
- `↑` / `↓` selects an Event.
- `←` / `→` transposes a pitched Event by one semitone or cycles a percussion voice.
- `Enter` enters an exact scientific pitch, percussion voice, or sample duration.
- `t` enters an exact onset for every Event kind.
- `u` enters an exact duration for a pitched or sample Event.
- `v` enters an exact velocity for every Event kind.
- `x` edits a sample occurrence's half-open original-source `START:END` region.
- `f` edits sample source-edge fade frames as `IN:OUT`; `g` edits occurrence gain; `p` edits occurrence pan.
- `A` arms the selected sample occurrence through its mapping path, or otherwise arms the selected Pattern through Master.
- `L` arms or disables an exact Audition Plan spanning the selected Pattern's natural occurrence range. `Space` starts it; pressing `L` during playback publishes the newly prepared loop through the same bounded seam.

At each half-open loop boundary schedule cursors rewind and active notes release, while bounded shared processor state continues so effect tails can decay naturally. An Event Grid or processor publication recompiles off-thread and preserves the plan when its durable IDs remain valid. Looping changes no source or History and is never used by offline rendering.

Percussion hits have no pitch or duration. Sample occurrences use durable IDs and non-destructive regions rather than pitch stepping; place, duplicate, split, and slip are also available through checked CLI commands. Groove, Variation, and Parameter-lock creation or transformation use the checked `edit groove-*`, `pattern-variation-*`, and `parameter-lock-*` CLI commands. Every displayed value remains source-backed.

## Instrument Faceplate

Faceplate exposes the Instrument author's shallow controls with exact physical values, legal ranges, and contextual explanations. It is the preferred beginner surface.

- `[` / `]` selects an Instrument.
- `↑` / `↓` selects a control.
- `←` / `→` makes a bounded 1% domain step.
- `Enter` replaces the selected value with exact physical input.

The same Instrument receives transient computer-keyboard and PipeWire MIDI audition.

## Exact Parameters

Parameters exposes every authored Parameter, including its durable key, unit, current base value, inclusive domain, default, and Automation/Modulation eligibility. Controls are identical to Faceplate, but this view is intentionally denser for experienced producers.

The exact input field initially selects the existing literal: typing replaces it, `Ctrl-A` reselects it, Backspace clears/removes characters, Enter applies, and Escape cancels.

## Automation

Automation shows one Song-scoped absolute lane at a time. Its header names the stable `ParameterTarget`, source location, base → Automation → Parameter lock → Modulation → effective provenance at the visible transport frame, and whether domain constraint occurred. The point table keeps durable point IDs, exact Song times, physical values, outgoing curves, and source locations visible.

- `[` / `]` selects a lane.
- `↑` / `↓` selects a durable point.
- `←` / `→` changes the selected value by 1% of the target's physical domain through a checked source edit.
- `Enter` opens exact physical-value entry.
- `t` opens exact absolute musical-time entry.
- `c` selects `step`, `linear`, or `power(P)` interpolation leaving the point.

The workspace intentionally stays shallow rather than becoming a dense desktop curve editor. Lane/point creation and deletion are available through readable source and the checked `edit automation-*` CLI commands. Checked creation inserts a point at its chronological source position, and a time edit that crosses neighbors relocates the whole durable point directive. Locator IDs are accepted as section-level time operands. Invalid moves—such as moving the required first point away from zero or colliding at one sample frame—are rejected before publication.

## Graph

Graph is an inspectable topology view. It lists selectable stable Nodes, typed Bus connections, Parameters, and Faceplate mappings in processing order, including Node kinds and setting/Parameter bindings. `↑` / `↓` selects any item; `Enter` opens its exact authoritative source location. `[` / `]` or `←` / `→` changes Instrument. Topology editing is intentionally source-only in the first release; automated whole-graph Structured Source Edits would exceed the accepted slice.

## Source

Source shows exact authoritative bytes with line numbers. `←` / `→` changes imported file, `↑` / `↓` scrolls, and Home returns to line one. Source is not hidden behind generated UI state, and Production View edits retain unrelated whitespace and comments.

Press `e` to suspend raw mode and the alternate screen and open the selected authoritative file with `$VISUAL`, then `$EDITOR`, then `vi`. The variable must identify an executable; configure editor-specific wait behavior in that executable or wrapper. Native playback continues, transient held notes are released first, and Candidate source is reloaded when the editor exits. A valid edit remains a Candidate until `P`; invalid source opens Diagnostics while the exact prior Active Revision stays audible.

## Diagnostics

Diagnostics shows all recoverable parse and whole-Project validation findings with stable code, severity, source file, line, column, message, and help. A Candidate with errors cannot be published, compiled, or substituted into playback.

## History

History is a durable append-only action log, not a transient undo stack. It shows Active status, sequence ID, Human/Agent/System provenance, actor, summary, changed-file count, and timestamp.

- `Enter` or `d` opens the exact parent diff.
- `R` asks for confirmation, restores the selected exact source-and-media snapshot atomically, and records the restore as a new History Entry.

Restoring never deletes or rewrites earlier entries.

## Playback and publication

Space compiles and prerolls the armed Audition Plan from the Active snapshot before creating PipeWire streams. While running, Space pauses or resumes without destroying transport, voices, processor tails, loop state, or prepared runtime state; `S` explicitly stops. Candidate source or media is never used implicitly. Exact declared media is stored in Active snapshots and sampler Instruments decode/resample it off the callback before playback; Media Track playback remains deferred. When another process publishes a revision while playback is running, the workspace:

1. loads and validates the exact Active snapshot from History;
2. resolves the retained audition target/range and deterministically initializes a replacement runtime off the audio thread from the latest observed frame offset;
3. reapplies transient path, bypass, Solo, and cue state and publishes through a bounded lock-free queue; and
4. at the audio block boundary, uses the callback's exact current offset or loop phase to realign into the newly resolved bounds (clamped only when the range became shorter), then begins a bounded 10 ms protective transition.

Tracks with unchanged schedule/topology semantics retain running voice state when publication stays at the same absolute timeline point. A range-relative jump does not import processor state from a different old timeline point. A compatible compiled mix graph otherwise retains shared processor and compensation state and executes once during publication. In a wholly compatible scalar publication, only changed continuous targets receive a monitoring-only 10 ms handoff ramp; unrelated linear/step Automation stays exact and boolean Mute stays discrete. Configuration/topology changes use the aligned equal-power old/new transition. If publication cannot be queued immediately, the old runtime continues. Invalid source never enters this path. If PipeWire stops or disconnects after playback began, the workspace reports the failure and offers `Space` to restart.

## Note Audition

In Faceplate, Parameters, and Graph views, `A W S E D F T G Y H U J K` plays C4–C5 on a graph Instrument. On stock drums, `A/S/D/F` (or `Z/X/C/V`) trigger kick/snare/hat/clap. Terminal key-release events send Note Off; a bounded timeout recovers terminals that report key presses only.

Audition is transient: it does not change source, Active Revision, or History. PipeWire MIDI follows the selected Instrument and uses normal Note On/Off velocity. See [Audio and MIDI](audio-and-midi.md).

## Terminal recovery

Crump enables raw mode, the alternate screen, and mouse reporting only for the workspace session. Normal exit restores all three. If the process is killed in a way that prevents cleanup, run:

```bash
reset
```

or close and reopen the terminal.
