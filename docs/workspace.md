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
| `1`–`9` | Select Timeline, Event Grid, Faceplate, Parameters, Graph, Source, Diagnostics, History, or Help |
| `Tab` / `Shift-Tab` | Next / previous view |
| `↑` / `↓` | Move the current selection |
| `Page Up` / `Page Down` | Move by eight rows |
| `←` / `→` | Make the view's bounded adjustment or change the Source/Graph context |
| `[` / `]` | Previous / next Pattern or Instrument where applicable |
| `Enter` | Open an exact-value editor or selected History diff |
| `Space` | Start or stop the exact Active Revision |
| `L` in Event Grid | Arm or disable the selected Pattern's looping Audition Phrase |
| `P` | Publish an inspected valid Candidate with a History summary |
| `r` | Reload Project and History immediately |
| `e` in Source | Suspend the workspace and open the selected file in `$VISUAL`, `$EDITOR`, or `vi` |
| `?` | Open keyboard help |
| `q` / `Ctrl-C` | Exit safely |

Mouse wheel selection is available as a convenience. No required workflow needs a pointer.

## Timeline

Timeline lists presentation-ordered Tracks, assigned Instruments, source-backed level/pan, and Clip ranges. The title displays the current playhead.

- `↑` / `↓` selects a Track and follows its Instrument in instrument views.
- `←` / `→` changes level by 1 dB through a checked source edit.
- `Enter` or `l` opens exact level entry.
- `p` opens exact pan entry.

Every accepted change replaces only the selected literal, validates the complete Project, writes atomically, and appends a human-origin History Entry. If source changed since the view was loaded, Crump rejects the operation rather than editing a stale range.

## Event Grid

Event Grid shows one Pattern's source Events with exact onset, type, pitch/voice, duration, and velocity. Currently sounding note ranges and recent hit windows turn green during playback.

- `[` / `]` selects a Pattern.
- `↑` / `↓` selects an Event.
- `←` / `→` transposes a pitched Event by one semitone or cycles a percussion voice.
- `Enter` enters an exact scientific pitch or percussion voice.
- `t` enters an exact onset for either Event kind.
- `u` enters an exact duration for a pitched Event.
- `v` enters an exact velocity for either Event kind.
- `L` arms or disables an Audition Phrase spanning one full selected Pattern occurrence at its first Timeline Clip. `Space` starts an armed phrase; pressing `L` during playback changes mode through the same bounded runtime-publication seam.

The header labels `LOOP:<pattern>` while this non-authoritative transport mode is active. At each half-open loop boundary schedule cursors rewind and active notes release, while bounded DSP effect state can decay naturally. An Event Grid or Faceplate publication recompiles off-thread and preserves the phrase. The loop changes no source or History and is never used by offline rendering.

Percussion hits have no pitch or duration. Their voice, onset, and velocity remain source-backed and editable.

## Instrument Faceplate

Faceplate exposes the Instrument author's shallow controls with exact physical values, legal ranges, and contextual explanations. It is the preferred beginner surface.

- `[` / `]` selects an Instrument.
- `↑` / `↓` selects a control.
- `←` / `→` makes a bounded 1% domain step.
- `Enter` replaces the selected value with exact physical input.

The same Instrument receives transient computer-keyboard and PipeWire MIDI audition.

## Exact Parameters

Parameters exposes every authored Parameter, including its durable key, unit, current value, inclusive domain, default, and modulation eligibility. Controls are identical to Faceplate, but this view is intentionally denser for experienced producers.

The exact input field initially selects the existing literal: typing replaces it, `Ctrl-A` reselects it, Backspace clears/removes characters, Enter applies, and Escape cancels.

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
- `R` asks for confirmation, restores the selected exact source snapshot atomically, and records the restore as a new History Entry.

Restoring never deletes or rewrites earlier entries.

## Playback and publication

Space compiles the Active snapshot before creating PipeWire streams. Candidate source is never used implicitly. When another process publishes a revision while playback is running, the workspace:

1. loads and validates the exact Active snapshot from History;
2. compiles and initializes a replacement runtime off the audio thread at the current transport frame;
3. publishes it through a bounded lock-free queue; and
4. realigns it to the callback's exact current frame and applies a 10 ms equal-power old/new transition at an audio block boundary.

Unchanged Tracks retain their running state; affected Tracks restart at the aligned transport position behind the transition. If publication cannot be queued immediately, the old runtime continues. Invalid source never enters this path. If PipeWire stops or disconnects after playback began, the workspace reports the failure and offers `Space` to restart.

## Note Audition

In Faceplate, Parameters, and Graph views, `A W S E D F T G Y H U J K` plays C4–C5 on a graph Instrument. On stock drums, `A/S/D/F` (or `Z/X/C/V`) trigger kick/snare/hat/clap. Terminal key-release events send Note Off; a bounded timeout recovers terminals that report key presses only.

Audition is transient: it does not change source, Active Revision, or History. PipeWire MIDI follows the selected Instrument and uses normal Note On/Off velocity. See [Audio and MIDI](audio-and-midi.md).

## Terminal recovery

Crump enables raw mode, the alternate screen, and mouse reporting only for the workspace session. Normal exit restores all three. If the process is killed in a way that prevents cleanup, run:

```bash
reset
```

or close and reopen the terminal.
