# Professional DAW Authoring Workflow Research

Accessed: 2026-07-26

## Scope and method

This report supports the wayfinder ticket **Prototype the Code and TUI Authoring Balance**. It records current, documented authoring primitives in Ableton Live 12, then compares Logic Pro 12.3 and REAPER 7.78. Only first-party manuals published by Ableton, Apple, and Cockos are used. This is workflow evidence, not a design for Crump or a feature-parity recommendation.

## Ableton Live 12 baseline

### Arrangement and navigation

Live's Arrangement View is a linear song timeline with vertically stacked tracks and clips in each track's main lane. Its overview and beat-time ruler support scrolling and zooming; a selection can be zoomed to and restored; the play position can be set spatially or entered numerically in bars, beats, and sixteenths. Arrangement editing is selection-based: users select clips, a time point, or a timespan, then move, resize, nudge, split, duplicate, delete, or apply commands. A configurable musical grid supplies snapping, with a temporary bypass. Locators, a loop brace, and direct transport commands provide repeatable navigation and audition ranges. [A1]

Live exposes keyboard focus targets for the control bar, Arrangement View, Clip View, Device View, browser, mixer, and other areas; `Tab`/`Shift-Tab` can move among focusable controls. Its shortcuts cover view visibility, focus movement, value increment/decrement, typed value entry, track selection, arrangement selection and nudging, zoom, transport, mixer focus, and automation mode. [A2]

### Contextual properties

Live keeps selected-clip properties in Clip View and track/device controls outside the timeline lane. Within Arrangement View, mixer controls can appear either beside tracks or in a dedicated mixer. Both presentations share the same underlying values. This proves a distinction between spatial placement in the timeline and exact or contextual control of the selected clip, track, or device, without requiring every property to remain visible at once. [A1][A4]

### Mixing and routing

The mixer provides per-track meters, volume, pan, mute, solo/cue, and record arm. It supports multi-track selection so one adjustment changes the same control on all selected tracks while preserving relative differences. Group tracks act as summing containers with mixer controls and effects; return tracks receive sends from multiple tracks; the Main track is the default final destination. [A4]

Routing is edited through each track's In/Out section: source, monitoring mode, and destination are explicit selectors. Internal routing can express many-to-one and one-to-many relationships and exposes pre-FX, post-FX, and post-mixer tap points. Send controls specify amount, while pre/post selection specifies where the branch is tapped. The I/O section is hideable, so ordinary level work need not display the full patchbay continuously. [A5]

### Automation

Live can record control movement during playback or edit automation directly as breakpoint envelopes. Automation Mode reveals the selected parameter over the track content; a parameter can instead be moved into a separate lane, and multiple lanes can be shown simultaneously or hidden without disabling their envelopes. Device and parameter choosers select the target and can filter to automated parameters only. [A3]

Direct editing includes creating, deleting, moving, selecting, and typing exact breakpoint values; drawing stepped or freehand data; curving segments; transforming selected ranges; and applying grid snapping or bypass. Automation can move with clips or be locked to song time. Manual movement of an automated control creates an explicit override state, and a re-enable command restores written automation. Recording and editing therefore remain distinguishable operations, and automation state remains visible. [A3]

## Comparison: Logic Pro 12.3

Logic's Tracks area confirms the same timeline primitives: tracks are rows on a left-to-right time axis; regions are arranged against a musical or standard-time ruler; the playhead can be positioned or scrubbed; regions can snap to a grid or alignment guides; and users can scroll and zoom between overview and precise editing. [L1]

Logic makes contextual inspection especially explicit. Its Inspector changes with selection and key focus. The Region inspector edits non-destructive playback properties of selected regions, the Track inspector applies parameters to all regions on a track, and adjacent channel strips expose level, pan, sends, inserts, effects, and routing without opening the full Mixer. [L2] This is evidence for three distinct scopes: selected object, containing track, and signal channel.

The full Mixer shows track, aux, output, and master channel strips together. It exposes relative level and pan, effects, mute/solo, buses, sends, and grouped control; view and filter controls reduce the visible channel set for the current task. [L5] Send routing splits a channel's signal into a parallel bus-to-aux path while the main path continues to its output. Send level controls branch amount, and the tap can be Post Pan, Post Fader, or Pre Fader. [L6]

Every Logic track has volume and pan automation curves, with additional curves for plug-in settings. The documented point workflow is to choose track or region automation, show a target curve, add points around the intended change, and adjust those points. [L3] Read, Touch, Latch, and Write modes distinguish playback from progressively more persistent control capture; Trim offsets existing automation and Relative adds a separately editable offset curve. Modes are available from track headers or channel strips. [L4]

Logic's Key Commands window provides searchable commands, filters assigned and unassigned commands, and supports custom assignments and reusable command sets. [L7] This corroborates command search and assignable keyboard operations as professional workflow primitives, rather than requiring every action to be permanently visible.

## Comparison: REAPER 7.78

REAPER similarly separates Arrange View from Mixer View. Its official guide describes Arrange View as the primary place to build a project by recording, arranging, and editing media items, while the Mixer duplicates track-panel functions and adds mixing-oriented detail. The Track Control Panel supplies track naming, record arm, routing, effects, automation access, pan, mute, and solo; available controls vary with panel size and layout. [R1, secs. 1.12-1.14]

Keyboard commands move to next/previous tracks, scroll, zoom horizontally and vertically, zoom to a time selection, and change track height. REAPER's Action List allows keyboard and mouse assignments to be changed for the workflow. [R1, secs. 2.6, 15.3-15.5] The arrangement supports track and item selection, movement, copying, deletion, and multi-selection, while volume, pan, mute, and solo can operate on selected tracks. [R1, secs. 5.1-5.2]

The track Route button opens a per-track routing window listing sends and receives; a send has level and pan and can carry audio, MIDI, or both. Tap choices include post-fader/post-pan, pre-fader/post-FX, and pre-fader/pre-FX. Sends may be adjusted inline when panel size permits. A Routing Matrix provides a project-wide structured view where connections can be added, removed, and inspected at source/destination intersections. [R1, secs. 2.21-2.23]

REAPER exposes automation either over media or in a separate lane. The separate lane is easier to edit but consumes space; either presentation can be toggled. A track Envelopes window filters and manages track, send, and effect parameters; it controls visibility, arming, and automation mode. Automation can be captured through Write, Touch, or Latch, or manually edited with points and freehand curves in Trim/Read. Read, Trim/Read, Touch, Latch, Latch Preview, and Write make playback, offset, preview, and destructive capture behavior explicit. [R1, secs. 18.1-18.11]

## Minimum proven workflow primitives

The following is the smallest cross-DAW set supported by the official workflows above. "Proven" means explicitly present in Ableton Live and corroborated by Logic Pro or REAPER; it does not imply a required Crump interaction.

| Responsibility surface | Minimum proven primitives | Evidence |
| --- | --- | --- |
| Source editing | No reviewed DAW documents editable source text as an authoring surface. The evidence establishes no professional baseline primitive here; any source responsibility remains outside this comparison. | [A1-A5][L1-L7][R1] |
| Structured inspector | Follow the current selection and expose properties at explicit scopes: item/region, track, device/effect, or channel. Support exact value entry or bounded adjustment, reveal routing and parameter targets by name, and avoid requiring all details to be visible simultaneously. | Live numeric position and parameter choosers [A1][A3]; Logic's scoped Inspector [L2]; REAPER's routing and Envelopes windows [R1] |
| Timeline view | Show tracks against a shared linear musical-time axis; represent arranged units as selectable spans; place the playhead; select points, ranges, items, and tracks; move, resize, copy, split, delete, and nudge; provide grid snapping with bypass; scroll, zoom to context/selection, restore or zoom out; define loop and navigation ranges. | [A1][A2][L1][R1] |
| Mixer controls | For each audible or routing channel, expose meter, level, pan/balance, mute, solo/audition, and where applicable arm/monitor. Apply a control to a multi-selection, expose groups/buses, returns/auxes and master/output channels, and permit a focused subset of channel strips. | [A4][L5][R1] |
| Routing controls | Name source and destination, distinguish main output from parallel sends, expose send amount, and expose tap position relative to effects/fader/pan. Provide both local per-channel inspection and a whole-project connection overview when routing becomes non-local. | [A5][L6][R1] |
| Automation lanes | Choose a target parameter; show its value over shared timeline time; add/select/move/delete points and segments; support exact values and curve/step shapes; show one target inline or multiple separate lanes; hide without disabling; distinguish read/playback, manual edit, and capture modes; make override/arming/write state explicit; define whether automation follows content or absolute song time. | [A3][L3][L4][R1] |
| Keyboard/command layer | Move focus among major work areas, navigate tracks and time, operate transport and selection, adjust or type values, show/hide detail, and find or assign commands. Commands are contextual to the focused surface and complement rather than replace structured views. | [A2][L7][R1] |

## Boundary established by the evidence

Across all three DAWs, the same project state is deliberately reachable from more than one task-oriented view: compact track controls and a full mixer, inline and separate automation lanes, local routing controls and broader routing views, or timeline selection and a contextual inspector. The recurring primitive is not duplicate ownership but synchronized presentations at different scopes. The manuals prove the need for spatial overview, scoped exact inspection, continuous channel control, explicit signal-flow editing, time-varying parameter editing, and keyboard command access. They do not prove how authoritative text should participate, how a TUI should render these surfaces, or which operation should generate source; those remain prototype questions.

## Official sources

All sources accessed 2026-07-26.

- **[A1] Ableton, Live 12 Manual, "Arrangement View."** https://www.ableton.com/en/live-manual/12/arrangement-view/
- **[A2] Ableton, Live 12 Manual, "Live Keyboard Shortcuts."** https://www.ableton.com/en/live-manual/12/live-keyboard-shortcuts/
- **[A3] Ableton, Live 12 Manual, "Automation and Editing Envelopes."** https://www.ableton.com/en/live-manual/12/automation-and-editing-envelopes/
- **[A4] Ableton, Live 12 Manual, "Mixing."** https://www.ableton.com/en/live-manual/12/mixing/
- **[A5] Ableton, Live 12 Manual, "Routing and I/O."** https://www.ableton.com/en/live-manual/12/routing-and-i-o/
- **[L1] Apple, Logic Pro 12.3 User Guide, "Tracks area overview."** https://support.apple.com/guide/logicpro/tracks-area-overview-lgcpf7c0d5d8/12.3/mac/15.6
- **[L2] Apple, Logic Pro 12.3 User Guide, "Inspector interface."** https://support.apple.com/guide/logicpro/inspector-interface-lgcpe9cc3b1d/12.3/mac/15.6
- **[L3] Apple, Logic Pro 12.3 User Guide, "Overview of automation."** https://support.apple.com/guide/logicpro/automation-overview-lgcpb1a1ea03/12.3/mac/15.6
- **[L4] Apple, Logic Pro 12.3 User Guide, "Choose automation modes."** https://support.apple.com/guide/logicpro/choose-automation-modes-lgcpb1a6ab26/12.3/mac/15.6
- **[L5] Apple, Logic Pro 12.3 User Guide, "Mixer interface."** https://support.apple.com/guide/logicpro/mixer-interface-lgcpe9cc43f6/12.3/mac/15.6
- **[L6] Apple, Logic Pro 12.3 User Guide, "Route audio via send effects."** https://support.apple.com/guide/logicpro/route-audio-via-send-effects-lgcp8ea0091c/12.3/mac/15.6
- **[L7] Apple, Logic Pro 12.3 User Guide, "Browse, import, and save key commands."** https://support.apple.com/guide/logicpro/browse-import-and-save-key-commands-lgcp68d9ced1/12.3/mac/15.6
- **[R1] Cockos, REAPER User Guide 7.78 (official PDF linked from the REAPER User Guide page).** https://www.reaper.fm/userguide/ReaperUserGuide778.pdf (landing page: https://www.reaper.fm/userguide.php)
