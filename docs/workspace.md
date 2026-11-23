# Production Workspace

Run `crump workspace`, or `crump` with no command in an interactive terminal that has no graphical display (where a display is available, a bare `crump` opens the desktop window below instead). Ratatui is the reference client; the Project model, validator, history, engine, renderer, and real-time adapter do not depend on it.

## Desktop window

Default builds also have `crump gui`, which a bare interactive `crump` uses when a Wayland or X11 display is available. It opens the same workspace in a desktop window ([ADR 0027](adr/0027-optional-desktop-client.md)). It has the same views and every key below works in it. On top of that:

- Click a view tab, a table row, or a toolbar button; each button's hover text names its key. Double-click a row for its exact edit.
- In the Timeline, click to place the cursor and select a Track, drag to select a range, Ctrl-scroll to zoom, Shift-scroll to pan, double-click a Clip to open its Pattern, and right-click for more.
- Instrument views show clickable Note Audition keys while playback runs.
- `e` in Source opens the file with the desktop's default application through `xdg-open` rather than `$EDITOR`; saved changes reload automatically.
- Closing the window exits, as does Ctrl-C.

`crump gui` needs a Project: pass `--project`, run it in a Project directory, or start it from a terminal to get the Project picker.

The workspace probes Project source, declared media, and Active History metadata several times per second. A metadata change invalidates the relevant cache and triggers exact-byte reconstruction; metadata never defines Candidate or Media Asset identity. External file or Agent changes therefore appear without restarting the application, while an unchanged Project is not reparsed on the UI thread.

## Revision header

The top line always distinguishes:

- **ACTIVE** — Candidate bytes equal the Active Revision.
- **VALID CANDIDATE** — disk source is valid but has not been published.
- **INVALID CANDIDATE** — disk source has blocking diagnostics; the prior Active Revision remains audible.

Short Candidate and Active hashes are shown side by side. During playback the same line shows the bar/beat playhead, PipeWire state, current monitor peak, active voice count, and MIDI connection indicator. An invalid external edit automatically opens Diagnostics; a valid unpublished edit prompts the musician to inspect and press `P`.

## Global keyboard map

| Key | Action |
| --- | --- |
| `1`–`9`, `0` | Select Timeline, Mixer, Processing, Event Grid, Faceplate, Instrument, Parameters, Automation, Graph, or Source; Tab reaches Diagnostics/History/Help |
| `H` / `?` | Open History / keyboard Help |
| `F12` | Toggle live workspace latency diagnostics; print the bounded summary after exit |
| `Tab` / `Shift-Tab`, `gt` / `gT` | Next / previous view |
| `j` / `k` or `↑` / `↓` | Move the current selection in every view |
| `gg` / `G` | Move to the first / last selection in the current view |
| `Ctrl-U` / `Ctrl-D`, `Page Up` / `Page Down` | Move by four / eight rows |
| `←` / `→` | Make the view's bounded adjustment, change an Instrument section, or change Source/Graph context |
| `[` / `]` | Previous / next Pattern, Instrument, or Automation lane where applicable |
| `Enter` | Open an exact-value editor or selected History diff |
| `Space` | Start, pause, or resume the armed Audition Plan against the exact Active Revision |
| `S` | Stop playback while retaining the armed target, range, path, transient bypasses, Solo, and cue |
| `A` in Timeline/Event Grid | Arm the selected Track, Pattern, or sample occurrence for audition |
| `i` in an Instrument-related view | Enter explicit Note Audition mode; `Esc` returns to Normal mode |
| `L` in Event Grid | Arm or disable the selected Pattern's exact half-open loop |
| `P` | Publish an inspected valid Candidate with a History summary |
| `r` | Reload Project and History immediately |
| `e` in Source | Suspend the workspace and open the selected file in `$VISUAL`, `$EDITOR`, or `vi` |
| `Esc` / `Ctrl-[` | Cancel the current prefix, input, range, or Note mode; never exit |
| `Ctrl-C` | The only workspace exit shortcut |
| `q` | Remain in the workspace and report the Ctrl-C exit contract |

Mouse wheel selection is available as a convenience. No required workflow needs a pointer.

## Timeline

Timeline is a DAW-style arrangement overview over presentation-ordered Tracks and the complete finite Song. It fits the whole Song by default and uses an exact meter-aware bar/beat ruler, Locator markers, Clip backgrounds and boundaries, and an overlaid playhead. Blue spans are authored Clip ranges, cyan glyphs summarize canonically materialized note/hit onsets and sustained notes, and yellow glyphs are verified source-waveform envelopes for scheduled sample occurrences. Overlapping Clips are visibly distinct. Track rows remain device-defined rather than fixed MIDI/audio kinds: a note that excites a graph sample, multisample, granular, or spectral Node is still shown as musical Event activity, not mislabeled as a Media Track waveform.

The selected-Track inspector retains assigned Instrument, source-backed level/pan, arrangement context, and every Clip's durable ID, start/duration, traversal mode, source offset, transpose, gain, fades, and optional Groove assignment. Sample waveforms resolve through the Track's sampler mapping and occurrence region after canonical Pattern Variation, Clip traversal/cropping, and Groove scheduling. Reverse, bounded-loop, and loop-crossfade traversal affect source projection. Exact-asset min/max pyramids use the versioned `crump-timeline-waveform-v1` display algorithm and are cached by asset hash outside the draw tick. They are source previews, not post-processor Track renders; Media Tracks remain unsupported.

Timeline has a transient two-dimensional cursor: `j` / `k` (or `↑` / `↓`) changes Track, while the following Normal-mode commands move exact musical time. A time motion seeks the current Active audition while stopped, paused, or playing; while stopped it also arms that position as the next `Space` start. If the cursor falls outside the armed target's half-open range, Crump reports the exclusion instead of silently clamping to another frame; select a containing range before playback. The cursor, navigation grid, ranges, marks, search state, jump history, follow mode, and viewport never edit Project source or History.

### Timeline motion and jumps

| Key | Action |
| --- | --- |
| `h` / `l`, `<` / `>` | Move by the current exact navigation grid |
| `[` / `]` | Choose a finer / coarser grid from 1/16 beat through 4 bars |
| `w` / `b` | Next / previous Clip or Locator boundary on the selected Track |
| `e` / `%` | End of the Clip under the cursor / toggle that Clip's edges |
| `^` / `$` | First / last arranged material on the selected Track |
| `g0` / `g$` | Exact Song start / end |
| `*` / `#` | Next / previous Clip using the Pattern under the cursor |
| `{` / `}` | Previous / next Locator |
| `:8`, `:8:3` | Go to displayed bar, or displayed bar and beat |
| `:chorus` | Go to a durable Locator ID |
| `:+2bar`, `:-1beat` | Move by a relative musical duration |
| `/` | Search Locator, Track, Clip, Pattern, and Instrument names or IDs |
| `n` / `N` | Repeat the search forward / backward |
| `ma`, `'a` | Set transient mark `a` / jump to it |
| `''`, `Ctrl-O` / `Ctrl-I` | Return to the preceding position / move backward or forward in jump history |
| `gd` / `gf` | Open the Pattern definition / exact Clip source under the cursor |
| `gi` / `gm` / `gp` | Open the selected Track's Instrument / Mixer channel / Processing route |

Crump requests progressive keyboard disambiguation, so compatible terminals report `Ctrl-I`, `Tab`, `Ctrl-[`, and `Esc` distinctly. Legacy terminals may encode `Ctrl-I` exactly like `Tab` and `Ctrl-[` exactly like `Esc`; Crump gives those indistinguishable encodings the documented Tab and Escape behavior.

### Timeline ranges and viewport

`V` enters RANGE mode at the cursor. Time motions adjust the active edge; `o` swaps edges. `L` commits the normalized half-open range as a loop, while `Enter` commits it for one-shot playback. `Esc` cancels the active selection, and `gv` restores the previous selection. `L` outside RANGE mode toggles the currently armed loop. These are transient Audition State operations and use the same exact musical-time/Locator resolver as Processing.

- `zf` fits the selected or armed range; `zz`, `zs`, and `ze` center or align the cursor.
- `zh` / `zl` and `,` / `.` pan by one-quarter viewport; `Ctrl-B` / `Ctrl-F` pan by a full viewport.
- `+` / `-` zooms up to 64×; `Home` restores whole-Song fit.
- `f` toggles follow-playhead mode.
- `M` asks for `ID NAME` and creates a durable Locator at the cursor through a checked source edit and History publication.
- `A` arms the selected Track, `←` / `→` changes its level by 1 dB, `Enter` opens exact level entry, and `p` opens exact pan entry.
- Set `CRUMP_TIMELINE_ASCII=1` to use ASCII arrangement glyphs; `C` and `POSIX` locales and `TERM=dumb` select this fallback automatically.

Every accepted durable change replaces only the selected literal or inserts the Locator, validates the complete Project, writes atomically, and appends a human-origin History Entry. If source changed since the view was loaded, Crump rejects the operation rather than editing a stale range.

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

Target/range/path changes prepare a replacement runtime off the callback and locate it with a cleared seek ([ADR 0028](adr/0028-interactive-playback-locates-by-cleared-seek.md)), then use the bounded publication transition. Compatible source publication retains the durable target, range, transient bypass/Solo/cue state, and relative transport frame when still valid. If a referenced ID or range disappears, the workspace reports the rejection and leaves the prior runtime audible.

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
- `f` edits sample source-edge fade frames as `IN:OUT`; `Alt-G` edits occurrence gain; `p` edits occurrence pan.
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

## Generated Instrument sections

Instrument is a sound-neutral projection of one graph Instrument's authoritative definition. Optional source-backed `presentation` metadata supplies stable section IDs, labels, order, and item references. The Workspace always exposes Sources/Oscillators, Source Mixer, Filters, Envelopes/LFOs, Modulation, Voicing/Unison/Glide, Routing, and Shared Effects: missing sections are generated, and Nodes, Buses, Parameters, sources, routes, or processors omitted by metadata are appended deterministically instead of being hidden. Custom/reordered topology therefore remains visible rather than being forced into fixed synth slots.

Each row shows object class, stable identity, effective authored contract, and exact metadata or object source location. The Voicing section also synthesizes one policy row from the authoritative mono/poly, retrigger, priority, stealing, seed, and glide declaration. A wavetable asset row shows exact relative path/hash, mono container shape, authored frame length/count/order, normalization and morph law, plus a deterministic per-frame waveform preview decoded from the verified Candidate inventory. Sample/multisample/granular/spectral asset rows show exact path/hash, channels, source rate/frame count, and deterministic whole-source previews; their Node rows expose selected slices, complete key/velocity/root/region/layer/round-robin mappings, bounded grain pool/window/edge/scan controls, or the spectral algorithm/peak/scan/region/transient contract. CLI inspection additionally exposes the spectral sample/cache identities, fixed 2,048/256 geometry, frame/peak/transient counts, and admitted payload size without exposing peak arrays. Associated Node rows retain scalar playback settings, while Modulation rows retain their live normalized and physical contribution telemetry. No generated row owns DSP or cache state.

- `[` / `]` selects an Instrument.
- `←` / `→` selects a generated section; `↑` / `↓` selects a row.
- `Enter` opens exact checked input for a Parameter or route depth. Other rows open their exact source location.
- `b` toggles a selected Instrument Modulation route's durable bypass through the checked Production View transaction.

Edits use expected-revision/source-site guards, whole-Project validation, atomic publication, and History. Presentation-only edits are excluded from the audio semantic hash and do not restart voices. Topology creation/deletion/reconnection remains source or Agent work.

## Exact Parameters

Parameters exposes every authored Parameter, including its durable key, unit, current base value, inclusive domain, default, and Automation/Modulation eligibility. Controls are identical to Faceplate, but this view is intentionally denser for experienced producers. Normalized macro Parameters use the same checked value editor. Instrument route depth and bypass are available through the shared Production View executor and `edit instrument-route-depth` / `edit instrument-route-bypass` CLI adapters; all use exact sites, validation, publication, and History.

The exact input field initially selects the existing literal: typing replaces it, `Ctrl-A` reselects it, Backspace clears/removes characters, Enter applies, and Escape cancels.

## Automation

Automation shows one Song-scoped absolute lane at a time. Its header names the stable `ParameterTarget`, source location, base → Automation → Parameter lock → Instrument route tier → Song route tier → effective provenance at the visible transport frame, and whether domain constraint occurred. Instrument route labels include durable bypass and scope: global contributions are singular, Instrument contributions are identified as per Track runtime instance, and per-voice contributions remain explicitly note-relative rather than being collapsed into a false Song-time scalar. The point table keeps durable point IDs, exact Song times, physical values, outgoing curves, and source locations visible.

- `[` / `]` selects a lane.
- `↑` / `↓` selects a durable point.
- `←` / `→` changes the selected value by 1% of the target's physical domain through a checked source edit.
- `Enter` opens exact physical-value entry.
- `t` opens exact absolute musical-time entry.
- `c` selects `step`, `linear`, or `power(P)` interpolation leaving the point.

The workspace intentionally stays shallow rather than becoming a dense desktop curve editor. Lane/point creation and deletion are available through readable source and the checked `edit automation-*` CLI commands. Checked creation inserts a point at its chronological source position, and a time edit that crosses neighbors relocates the whole durable point directive. Locator IDs are accepted as section-level time operands. Invalid moves—such as moving the required first point away from zero or colliding at one sample frame—are rejected before publication.

## Graph

Graph is an inspectable topology view. It lists selectable stable Nodes and typed Bus connections in separate **Voice** and **Shared Post-Voice** sections, followed by Parameters and Faceplate mappings. Both domains use deterministic processing order and show Node kinds, setting/Parameter bindings, activity, source files, and lines; the shared heading also exposes its single `voice_sum.audio` boundary. `↑` / `↓` selects any item, including a shared Node or Bus; `Enter` opens its exact authoritative source location. `[` / `]` or `←` / `→` changes Instrument. Topology creation, deletion, and reconnection are source or Agent operations; whole-graph Structured Source Edits are unsupported. Diagnostics and processing resource inspection separately report shared instance, work, and state admission facts.

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

Space compiles the armed Audition Plan from the Active snapshot and locates it at the armed cursor or range start with a cleared seek before creating PipeWire streams, so a mid-Song start is as quick as one from bar 0; notes and tails begun before that point are not heard. While running, Space pauses or resumes without destroying transport, voices, processor tails, loop state, or prepared runtime state; `S` explicitly stops. Candidate source or media is never used implicitly. Exact declared media is stored in Active snapshots and sampler Instruments decode/resample it off the callback before playback; Media Track playback is unsupported. When another process publishes a revision while playback is running, the workspace:

1. loads and validates the exact Active snapshot from History;
2. resolves the retained audition target/range and deterministically initializes a replacement runtime off the audio thread from the latest observed frame offset;
3. reapplies transient path, bypass, Solo, and cue state and publishes through a bounded lock-free queue; and
4. at the audio block boundary, uses the callback's exact current offset or loop phase to realign into the newly resolved bounds (clamped only when the range became shorter), then begins a bounded 10 ms protective transition.

Tracks with unchanged schedule/topology semantics retain running voice and Track-owned shared post-voice state when publication stays at the same absolute timeline point. A range-relative jump does not import processor state from a different old timeline point. A compatible compiled mix graph otherwise retains shared processor and compensation state and executes once during publication. In a wholly compatible scalar publication, only changed continuous targets receive a monitoring-only 10 ms handoff ramp; unrelated linear/step Automation stays exact and boolean Mute stays discrete. Configuration/topology changes use the aligned equal-power old/new transition. If publication cannot be queued immediately, the old runtime continues. Candidate, committed Active, queued publication, and audible inspection are distinct: generated live views change provenance only after callback telemetry confirms the matching publication generation. Invalid source never enters this path. If PipeWire stops or disconnects after playback began, the failed player is retired and `Space` starts a newly prepared runtime.

## Note Audition

In Faceplate, Instrument, Parameters, and Graph views, press `i` to enter **NOTE** mode. In that explicit mode, `A W S E D F T G Y H U J K` plays C4–C5 on a graph Instrument. On stock drums, `A/S/D/F` (or `Z/X/C/V`) trigger kick/snare/hat/clap. `Esc` or `Ctrl-[` releases held notes and returns to Normal mode, where `j` / `k` always navigate. Terminal key-release events send Note Off; a bounded timeout recovers terminals that report key presses only.

Audition is transient: it does not change source, Active Revision, or History. PipeWire MIDI follows the selected Instrument and uses normal Note On/Off velocity. See [Audio and MIDI](audio-and-midi.md).

## Workspace performance diagnostics

Press `F12` to show the most recent and maximum draw and event-read-to-draw times plus refresh/reload counts. The workspace prints a bounded summary after terminal restoration when diagnostics were enabled at least once. Start with diagnostics visible by setting `CRUMP_WORKSPACE_DIAGNOSTICS=1`.

The exit summary reports count, last, mean, p50, p95, p99, and maximum duration for background filesystem probes, exact Candidate loads, History loads, derived-view rebuilds, Active preparation, durable mutation, result adoption, terminal draws, and event-read-to-draw latency. Percentiles cover the most recent 128 samples. Event timing starts when Crossterm reports readiness, immediately before decoding the event, so it excludes time already queued behind other UI-thread work; it also ends at application draw submission and does not prove that a terminal emulator or display presented pixels. Use a PTY or terminal-emulator probe for queue-inclusive input latency.

Run the ignored fixed-size hot-path profile against a representative Project with:

```bash
CRUMP_WORKSPACE_PROFILE_PROJECT=/path/to/project \
  cargo test --release workspace::tests::profile_workspace_hot_paths \
  -- --ignored --nocapture
```

It reports terminal-thread queue cost and background completion for unchanged probes and forced refreshes, plus per-view draw time, allocation count, and allocated bytes. The persistent capacity-one builder coalesces reload and preparation intent; a bounded reclaimer destroys replaced snapshots and runtimes away from both terminal and audio threads. Use the same Project, terminal dimensions, Cargo profile, and host when comparing results. See the [2026-08-25 latency investigation](evidence/workspace-latency-2026-08-25.md) for a production-scale example and hardware lower-bound comparison.

## Qualification boundary

Generated views, callback telemetry, deterministic renders, and `qualify` reports expose engineering facts but do not assign musical credibility. The offline qualifier opens no PipeWire device and records no xruns. Use the [native-synthesis qualification runbook](qualification/native-synthesis.md) for the separate repository, independent-producer, and named-hardware evidence paths; the workspace must not present one class as another.

## Terminal recovery

Crump enables raw mode, the alternate screen, and mouse reporting only for the workspace session. Normal exit restores all three. If the process is killed in a way that prevents cleanup, run:

```bash
reset
```

or close and reopen the terminal.
