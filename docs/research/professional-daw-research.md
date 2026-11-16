# Professional DAW Research

Access date for all web sources: **2026-07-26**.

This report records observed workflow and engineering behavior from Ableton Live 12, Bitwig Studio, Ardour, CLAP, PipeWire, and ITU-R documentation. Only first-party manuals, specifications, and platform documentation are used. These observations describe the cited systems; they do not define Crump behavior or require feature parity.

## Arrangement and editing

- Live's Arrangement View is a linear timeline with an overview, beat and elapsed-time rulers, locators, loop selection, selection-based edits, fixed/adaptive grids, clip move/resize/split, and whole-arrangement insert/delete/duplicate-time operations. Meter markers may occur off a barline and create fragmentary bars. `S1`
- Live keeps clip fades separate from automation. Consolidation creates new audio before track effects and mixing. `S1`
- Live exposes keyboard commands for focus movement, value entry, transport, grid changes, selection, split, duplicate, time insertion/deletion, track folding, zoom, and export. `S8`

## Mixing and routing

- Live provides peak and RMS metering, volume, two stereo pan modes, mute, solo/cue, nested Group Tracks, Return Tracks fed by Sends, and one Main track. Sends can be pre- or post-mixer; Return and Main channels host effects. `S2`
- Live's routing UI exposes sources, destinations, monitoring modes, pre-FX/post-FX/post-mixer taps, many-to-one submixes, one-to-many taps, multi-output Instruments, and named sidechain destinations. `S3`
- Live warns that Return feedback can raise level dramatically and disables Return-to-Return Sends by default. `S2`

## Automation and modulation

- Live records Arrangement automation, distinguishes temporary manual override from written automation, can re-enable the written state, and edits envelopes as breakpoints with typed values, curves, snapping, independent lanes, and an option to lock envelopes to Song time while Clips move. `S4`
- CLAP Parameters have stable IDs, finite plain-value ranges/defaults, stepped/enum/automatable/modulatable flags, value/text conversion, sample-offset events, and separate Automation and Modulation clearing. The specification advises hosts to persist plain Automation values and plain-value Modulation deltas. `S11`

## Latency compensation

- Live compensates Live and plug-in device delays, including Return Tracks. Reduced-latency monitoring allows monitored Tracks to be out of sync with compensated Tracks. High reported latency can increase sluggishness and CPU load. `S5`
- CLAP's latency extension reports samples. A latency change is allowed during activation; an active plug-in requests restart before applying changed topology. `S10`

## Crash and failure containment

- Live can skip plug-in scanning at launch. A plug-in that crashes during scanning is identified and can be made unavailable after repeated crashes. `S5`
- Bitwig advertises VST/CLAP hosting with delay compensation and crash protection. `S9`
- Live keeps crash-recovery BaseFiles, Undo, and metadata, can trigger restoration on restart, and maintains the ten most recently saved backup Sets after the second save. `S6` `S7`

## Realtime behavior

- PipeWire calls an `RT_PROCESS` stream from a realtime thread. File operations, blocking operations, and PipeWire functions not marked RT-safe are unsafe there. Late-queued buffers slip to the next processing cycle. PipeWire exposes stream state and graph timing. `S14`
- CLAP says audio threads can face sub-millisecond deadlines and should avoid allocation/free, contended locks, I/O, and waiting. Long or blocking main-thread work belongs on background threads. Its thread-check extension supports runtime assertions. `S13`
- PipeWire's RT module obtains realtime scheduling through rlimits, the realtime portal, or RTKit and includes CPU-time limits. `S15`
- Live defines audio CPU as processing time divided by one buffer's playback time; 100% exhausts the deadline. Its documentation recommends testing the most intense simultaneous workload instead of average load. `S16`

## Project portability and recovery

- Live Projects group Sets and media. The file manager lists references, locates missing files, and can collect external files into the Project. Moving or deleting externally referenced files breaks a Set. `S6`
- CLAP state saves and restores Parameter and non-Parameter state through streams and supports a dirty notification. The Parameter specification says hosts should not invent a Parameter-only fallback when state is unsupported. `S12` `S11`

## Rendering and export

- Live exports a defined start and length, Main or individual/selected Tracks, equal-length aligned files, optional Return/Main processing, lossless formats at selectable rate/depth, and explicit dither. Hardware-dependent paths render in realtime; detected dropouts can restart the render. `S6`
- Ardour Stem export includes silence to preserve synchronization and can export Tracks or Buses with processing. Its export analysis reports format, channels, rate, duration, peak, true peak, integrated loudness, and Loudness Range. `S17` `S18`
- ITU-R BS.1770-5 specifies Programme Loudness and true-peak algorithms; it does not prescribe one universal distribution target. `S19`
- CLAP distinguishes realtime and offline render modes; a plug-in can require realtime operation or use more expensive algorithms offline. `S20`

## Accessibility and keyboard workflow

- Live documents keyboard focus targets, focus traversal, neighboring-control traversal, exact numeric entry, view-independent commands, transport, browser navigation, and audio-engine control. `S8`
- Live's command vocabulary reuses selection, arrows, Enter, Escape, typed values, and standard undo/save operations across views. `S8`

## Sources

| ID | Primary source | Relevant claim | Accessed |
| --- | --- | --- | --- |
| S1 | Ableton, [Live 12 Manual: Arrangement View](https://www.ableton.com/en/live-manual/12/arrangement-view/) | Linear arrangement, navigation, grids, clips, fades, time edits, meter markers | 2026-07-26 |
| S2 | Ableton, [Live 12 Manual: Mixing](https://www.ableton.com/en/live-manual/12/mixing/) | Metering, groups, returns, sends, Main, cue, feedback warning | 2026-07-26 |
| S3 | Ableton, [Live 12 Manual: Routing and I/O](https://www.ableton.com/en/live-manual/12/routing-and-i-o/) | Sources/destinations, monitoring, taps, submixes, sidechains | 2026-07-26 |
| S4 | Ableton, [Live 12 Manual: Automation and Editing Envelopes](https://www.ableton.com/en/live-manual/12/automation-and-editing-envelopes/) | Recording, override/re-enable, breakpoint editing, curves and locking | 2026-07-26 |
| S5 | Ableton, [Live 12 Manual: Working with Instruments and Effects](https://www.ableton.com/en/live-manual/12/working-with-instruments-and-effects/) | Plug-in scanning behavior and device delay compensation | 2026-07-26 |
| S6 | Ableton, [Live 12 Manual: Managing Files and Sets](https://www.ableton.com/en/live-manual/12/managing-files-and-sets/) | Projects/assets, collection, lossless export, aligned Tracks, realtime rendering | 2026-07-26 |
| S7 | Ableton, [Backup Sets](https://help.ableton.com/hc/en-us/articles/360000377870-Backup-Sets) and [Recovering a Set manually after a crash](https://help.ableton.com/hc/en-us/articles/115001878844-Recovering-a-Set-manually-after-a-crash) | Bounded backups and crash recovery files | 2026-07-26 |
| S8 | Ableton, [Live 12 Manual: Live Keyboard Shortcuts](https://www.ableton.com/en/live-manual/12/live-keyboard-shortcuts/) | Focus, navigation, editing, value entry, transport, export | 2026-07-26 |
| S9 | Bitwig, [Bitwig Studio Feature List](https://www.bitwig.com/feature-list/) | VST/CLAP crash protection and delay compensation | 2026-07-26 |
| S10 | CLAP, [`latency.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/latency.h) | Sample latency and restart/change contract | 2026-07-26 |
| S11 | CLAP, [`params.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/params.h) | Stable Parameter IDs, plain values, Automation/Modulation, event timing | 2026-07-26 |
| S12 | CLAP, [`state.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/state.h) | Plug-in state save/load and dirty notification | 2026-07-26 |
| S13 | CLAP, [`thread-check.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/thread-check.h) | Main/audio/background thread roles and realtime restrictions | 2026-07-26 |
| S14 | PipeWire, [Streams](https://docs.pipewire.org/page_streams.html) | Process callback, RT safety, buffer timing and stream state | 2026-07-26 |
| S15 | PipeWire, [RT module](https://docs.pipewire.org/page_module_rt.html) | RTKit/portal/rlimit scheduling and RT CPU-time safeguards | 2026-07-26 |
| S16 | Ableton, [Live 12 Manual: Computer Audio Resources and Strategies](https://www.ableton.com/en/live-manual/12/computer-audio-resources-and-strategies/) | Buffer-deadline CPU metric and worst-case workload testing | 2026-07-26 |
| S17 | Ardour, [Export Dialog](https://manual.ardour.org/exporting/export-dialog/) | Formats, time spans, overwrite checks, and written-file analysis | 2026-07-26 |
| S18 | Ardour, [Stem Exports](https://manual.ardour.org/working-with-sessions/interchange-with-other-daws/stem-exports/) | Silence-aligned Stems and optional Track/Bus processing | 2026-07-26 |
| S19 | ITU-R, [Recommendation BS.1770-5](https://www.itu.int/rec/R-REC-BS.1770/en) | Programme Loudness and true-peak measurement algorithms | 2026-07-26 |
| S20 | CLAP, [`render.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/render.h) | Realtime/offline modes and hard realtime requirements | 2026-07-26 |
