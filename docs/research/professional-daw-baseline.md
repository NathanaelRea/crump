# Professional DAW Baseline for Crump

Access date for all web sources: **2026-07-26**.
Status: Informative destination research; its former First Public Release implications and proposed 3-5 minute professional-track gate are superseded by the current product contract and release interpretation.

## Purpose and method

This report identifies professional workflow and engineering baselines relevant to Crump's Complete Production Environment destination. It starts with Ableton Live 12 because its linear arrangement, electronic-production workflow, routing, automation, file management, and rendering documentation are close to that intended depth. Bitwig Studio, Ardour, CLAP, PipeWire, and ITU-R sources are used where they add clearer evidence about failure isolation, host contracts, Linux real-time behavior, export verification, or loudness measurement.

Only first-party manuals, specifications, and platform documentation are used. Each row separates **observed competitor or standard behavior** from a **Crump implication**. An observation is evidence, not a requirement for feature parity. Proposed numerical gates are Crump engineering recommendations derived from the documented deadline and recovery constraints; they are not claims about competitor guarantees.

The observations remain useful, but every Crump implication below predates the narrow Musical Sketch release and is non-normative unless separately accepted into the current product contract or an ADR. Third-party plugins, professional routing and Automation, stems, samples, and the former complete-track gate are outside the First Public Release.

## Evidence

### Arrangement and editing

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live's Arrangement View is a linear timeline. It supports an overview, beat and elapsed-time rulers, locators, loop selection, selection-based edits, adjustable fixed/adaptive grids, clip move/resize/split, and whole-arrangement insert/delete/duplicate-time operations. Meter markers may occur off a barline and create fragmentary bars. [S1] | Adopt the workflow primitives, not Live's UI: precise playhead and time-range addressing; named locators; loop/audition range; grid-aware and grid-free move, resize, split, duplicate, insert-time, and delete-time structured edits; explicit tempo/meter inspection; and deterministic preview from any position. Crump's rational Musical Time and fragmentary-bar semantics already support this cleanly. |
| Live keeps clip fades separate from automation and uses short edge fades to avoid clicks; consolidation creates new audio before the track effects/mixer rather than silently pretending to be a reversible clip edit. [S1] | Preserve non-destructive source operations and make any future destructive/bounce operation explicit about its tap point and generated asset. First release need not add waveform fades or consolidation because advanced audio editing is out of scope. |
| Live exposes keyboard commands for focus movement, value entry, transport, grid changes, selection, split, duplicate, time insertion/deletion, track folding, zoom, and export. [S8] | Every first-release authoring and inspection path must be operable without a pointer. Structured TUI edits should map to stable commands, report their target and result, and share undo history with text edits. |

### Mixing and routing

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live provides peak and RMS metering, volume, two stereo pan modes, mute, solo/cue, nested Group Tracks, Return Tracks fed by sends, and one Main track. Sends can be pre- or post-mixer; return and main channels host effects. [S2] | Retain Crump's explicit Track/Group/Return/Master roles and fixed Channel Strip order. Provide inspectable peak plus average-level metering, pre/post send state, gain/panner/mute state, signal activity, clipping/non-finite diagnostics, and routing ancestry. Solo/cue remain transient Audition State and must not alter source or renders. |
| Live's routing UI is a patchbay with explicit sources, destinations, monitoring modes, and pre-FX/post-FX/post-mixer tap points. It supports many-to-one submixes, one-to-many taps, multi-output instruments, and named sidechain destinations. [S3] | Compile source into an inspectable typed graph. Diagnostics should name the complete source-to-destination path, tap point, layout, and failed compatibility rule. Do not copy Live's implicit mono/stereo conversion: Crump's accepted design requires explicit conversion nodes and acyclic inter-channel routing. |
| Live warns that return feedback can raise level dramatically and unexpectedly, and disables return-to-return sends by default. [S2] | Keep arbitrary feedback cycles out of the first-release graph. Intentional feedback belongs only inside an explicit bounded stateful device with finite-output tests and monitoring protection. |

### Automation and modulation

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live records Arrangement automation, distinguishes temporary manual override from written automation, can re-enable the written state, and edits parameter envelopes as breakpoints with exact typed values, curves, snapping, independent lanes, and an option to lock envelopes to song time while clips move. [S4] | Always show a parameter's base, Automation, Modulation contributions, effective value, unit, and whether playback is source-driven or transiently auditioned. Structured moves must state whether Song-scoped Automation moves with selected material. Exact physical-value entry and deterministic interpolation are mandatory; recorded automation is not required for first release. |
| CLAP parameters have stable IDs, finite plain-value ranges/defaults, explicit stepped/enum/automatable/modulatable flags, value-to-text and text-to-value conversion, sample-offset events during processing, and separate automation and modulation clearing. The specification advises hosts to persist plain automation values and plain-value modulation deltas. [S11] | Preserve immutable Parameter keys and authored physical units. A future CLAP adapter must map CLAP stable IDs and capabilities to Crump Parameters, retain sample offsets, reject non-finite metadata, avoid recording duplicate MIDI-CC and parameter automation, and never infer modulation support. This does not make CLAP hosting a release requirement. |

### Latency compensation

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live automatically compensates Live and plug-in device delays, including Return Tracks. Its reduced-latency monitoring mode lowers monitored-track latency by allowing those tracks to be out of sync with compensated tracks. High reported latency can increase sluggishness and CPU load. [S5] | Keep full Event/Audio/Control compensation at every convergence in normal playback and rendering. If a low-latency audition mode is ever added, label its uncompensated paths and exclude it from authoritative render semantics rather than silently changing timing. |
| CLAP's latency extension reports latency in samples. A latency change is allowed during activation; an active plug-in requests restart before the changed topology is applied. [S10] | Treat latency as configuration metadata used to compile a Candidate Revision. Never resize compensation buffers or rebuild topology in the audio callback. Test changing native-device latency and, conditionally, CLAP latency across parallel, Send, Return, and sidechain convergence. |

### Crash and failure containment

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live can skip plug-in scanning at launch. A plug-in that crashes scanning is identified; after a repeated scan crash Live can make it unavailable until reinstall. [S5] | If CLAP is admitted, discovery must run outside the audio engine and preferably outside the main process, with per-binary timeout, crash attribution, quarantine, explicit rescan, and a cache committed only after a successful scan. Native release startup must not depend on third-party code. |
| Bitwig advertises VST/CLAP hosting with delay compensation and crash protection. [S9] | Process isolation is the professional containment reference, not proof that every plugin can be made safe. Conditional CLAP acceptance should require that a plug-in process crash cannot corrupt project source, kill the TUI, leave unsafe output, or prevent reopening in a bypassed/quarantined state. |
| Live keeps crash-recovery BaseFiles, Undo, and metadata, can trigger restoration on restart, and maintains the ten most recently saved backup Sets after the second save. [S6][S7] | Write source and generated metadata atomically, retain bounded recoverable revisions, and keep recovery data outside the authoritative project until the user accepts it. On startup, identify the last durable revision, candidate recovery revision, and active runtime revision. Recovery must never overwrite the only known-good project implicitly. |

### Real-time safety and performance

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| PipeWire calls an `RT_PROCESS` stream from a real-time thread; file operations, blocking operations, and PipeWire functions not explicitly marked RT-safe are unsafe there. Late-queued buffers slip to the next processing cycle. PipeWire exposes stream state and graph timing. [S14] | The callback may only dequeue/queue buffers, consume prebuilt immutable graph state, process preallocated DSP/event storage, and publish lock-free bounded telemetry. No allocation, deallocation, blocking/contended lock, file or console I/O, parsing, graph construction, plugin scanning, or unbounded work may occur there. Treat late completion as an xrun-class fault. |
| CLAP says the audio thread may face deadlines below 1 ms and should avoid allocation/free, contended locks, I/O, and waiting. Long or blocking main-thread work belongs on background threads. Its thread-check extension supports runtime assertions. [S13] | Enforce thread roles in APIs and debug builds. Add instrumentation that fails tests on callback allocation, blocking synchronization, forbidden syscalls, or main-thread-only CLAP calls from audio processing. Keep parsing, asset loading, saves, and render encoding off the callback. |
| PipeWire's RT module obtains real-time scheduling through rlimits, the realtime portal, or RTKit and includes CPU-time limits as protection against an RT thread locking the system. [S15] | Detect and report actual scheduling class, priority, sample rate, quantum, graph/device state, and xruns. Non-RT fallback may be an explicit degraded mode but cannot pass release acceptance, matching the accepted Linux baseline. |
| Live defines its audio CPU meter as processing time divided by one buffer's playback time; 100% means the deadline is exhausted, and it recommends testing the most intense simultaneous workload rather than average load. [S16] | Measure callback wall time against each quantum deadline and test the peak reference-project section, not an idle average. Report current, peak, and high-percentile deadline utilization plus xruns and graph-update latency. |

### Project portability and recovery

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live Projects group Sets and media; its file manager lists references, locates missing files, and can collect external files into the project. Live warns that moving or deleting externally referenced files breaks a Set. [S6] | Keep human-readable source authoritative. Add a deterministic project validation/packaging command that inventories every asset with normalized project-relative path, media properties, and content hash; reports missing/changed/external assets; and can copy approved external assets into the project without rewriting unrelated source. |
| CLAP state saves and restores both parameter and non-parameter state through streams and supports a dirty notification. The parameter specification says hosts should not invent a parameter-only fallback when state is unsupported. [S12][S11] | Conditional CLAP projects must persist opaque plugin state with plugin identity/version and a checksum, validate save/load round trips, and surface unsupported or failed state explicitly. Never claim reproducibility from a parameter snapshot when the plugin does not implement state. Native projects remain useful without CLAP. |

### Rendering and export

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live exports a defined start and length, Main or individual/selected tracks, equal-length aligned individual files, optional Return/Main processing, lossless WAV/AIFF/FLAC at selectable rate/depth, and explicit dither. Hardware-dependent paths render in real time; detected dropouts can restart the render. [S6] | Require an explicit render range and tail policy; lossless master plus selected stems; stable, collision-safe names; identical start sample and duration for stems; declared sample rate, format, bit depth, and dither; and atomic output publication. Hardware-dependent real-time rendering is out of first-release scope. |
| Ardour stem export includes silence to preserve synchronization and can export tracks or buses with processing applied. Its export analysis reports format, channels, rate, duration, peak, true peak, integrated loudness, and loudness range. [S17][S18] | Emit a machine-readable render manifest containing source revision/engine identity, range/tail policy, sample rate, layout, format/depth, stem tap and processing policy, frame count, file hash, sample peak, true peak, integrated loudness, and non-finite count. Analyze the written files, not only the in-memory bus. |
| ITU-R BS.1770-5 specifies algorithms for programme loudness and true-peak level; it does not prescribe one universal distribution target. [S19] | Implement/report BS.1770-5-compatible integrated loudness and true peak when metering is available, but do not fail a creative render against a universal LUFS target. Fail only explicit project/delivery constraints, clipping policy, non-finite samples, encoding errors, or internal inconsistency. |
| CLAP distinguishes real-time and offline render modes; a plugin can require real-time operation or use more expensive algorithms offline. [S20] | A future CLAP render path must query and honor render mode, record whether a path was offline or real-time constrained, and include that in reproducibility evidence. Native playback and offline rendering still share Crump's timing, automation, modulation, and latency semantics. |

### Accessibility and keyboard workflow

| Evidence from primary source | Crump baseline implication |
| --- | --- |
| Live documents direct keyboard focus targets, forward/backward focus traversal, neighboring-control traversal, exact numeric entry, view-independent editing commands, transport, browser navigation, and audio-engine control. [S8] | Keyboard completeness is a release requirement for the TUI, not an optional shortcut layer. Focus must be visible and programmatically represented; every control needs a stable name, current value/unit, editable state, and error text; status cannot rely on color, animation, waveform shape, or meter graphics alone. |
| Live's command vocabulary reuses selection, arrows, Enter, Escape, typed values, and standard undo/save operations across views. [S8] | Keep a small consistent command grammar, searchable command/help surface, user-remappable keys with conflict detection, and a documented reset/default map. Automated acceptance should execute the entire native reference workflow through commands without pointer input or visual-only assertions. |

## Recommendations for Crump

### Product baseline

1. **Make the linear workflow complete.** Support keyboard-first navigation from Song to Track/Clip/Pattern/device/Parameter, exact time and value entry, locators, loop range, grid-aware and free edits, split/duplicate/move/resize, and global insert/delete time. Keep Session launching, comping, and destructive waveform editing out of scope.
2. **Make signal flow explainable.** Provide one command that traces any audible node to Master, including channel role, ports/layouts, inserts, send tap, panner, mute, latency, sidechains, and compensation. Meter peak and average level, expose clipping/non-finite events, and keep Solo/cue transient.
3. **Make control provenance visible.** For a Parameter, show identity, physical unit/domain, base, Automation value and representation, each Modulation route/depth, clamp or held-non-finite state, and final effective value. Never flatten procedural control material silently.
4. **Make bad edits and failures non-destructive.** Candidate Revision errors must leave the identified Active Revision sounding. Save atomically, keep bounded revisions/recovery journals, and offer explicit inspect/accept/discard recovery. A failed render must not replace an existing good artifact.
5. **Keep third-party hosting conditional and isolated.** Native devices must complete the reference project. Admit CLAP only after scanning, stable parameter mapping, state round trips, latency/restart handling, offline mode, GUI policy, timeout/crash containment, quarantine, and reopen-with-bypass pass an explicit gate.
6. **Treat exports as build artifacts.** A render command should be reproducible and scriptable, write a manifest, align stems, define tails and tap/processing policy, analyze final files, and publish outputs atomically.

### Proposed measurable release gates

These thresholds are proposed Crump requirements and should be fixed against named hardware and fixtures before release.

| Area | Proposed acceptance gate |
| --- | --- |
| End-to-end workflow | An independent target musician completes the documented 3-5 minute Reference Project from open/edit/audition through save/reopen and master/stem render without developer intervention. Every required action is possible keyboard-only. |
| Correctness | Golden impulse/event fixtures across serial, parallel, Group, Send, Return, and sidechain paths align at every convergence to within **0 samples** of declared integer-sample compensation, at supported quanta (at least 64, 128, and 256 frames). Playback and offline render produce the same event positions and compensation. |
| Real-time | On the named Ubuntu/PipeWire/hardware baseline at the release rate and quantum, the worst-case Reference Project runs **30 continuous minutes with 0 PipeWire xruns**, 0 missed callback deadlines, 0 callback allocations, 0 blocking/contended locks, and 0 forbidden callback I/O. Record maximum and p99.9 callback utilization; require p99.9 at or below **70%** of the quantum deadline to retain operating margin. |
| Live updates | A scripted stress run applies valid and invalid source edits during playback. **100%** of invalid candidates leave the prior Active Revision audible; valid publication completes within a separately fixed p99 boundary; no partial graph/state publication, stuck notes, non-finite output, or unbounded discontinuity occurs. |
| Save/recovery | Fault injection at every save/publication step yields either the complete old revision or complete new revision after restart, never a mixed/corrupt project. **100%** of test cases preserve the last durable source and referenced in-project assets. Recovery is explicit and idempotent. |
| Render | Repeated unchanged native renders on the fixed engine/build/configuration satisfy the project's chosen deterministic comparison rule; all stem files share start frame and frame count; manifest hashes match files; tails match policy; and non-finite count is **0**. Interrupted or failed renders leave no artifact presented as successful. |
| Portability | Copy the project to a clean reference account with only declared dependencies. Validation reports **0 missing or changed assets**, reopen preserves authored IDs/routing/parameters/automation/seeds, and the master passes the deterministic comparison rule. External absolute paths fail the portable-project gate. |
| Resilience | Parser, UI/TUI, audio-device loss/reconnect, render cancellation, and (if admitted) scanner/plugin-process crash tests produce no source corruption or dangerous output. A quarantined plugin cannot block native startup; plugin failure can be reopened in an identified bypassed state. |
| Accessibility | Automated command-level tests cover **100% of the documented first-release workflow** without pointer input. Manual review confirms visible focus, logical traversal, exact value entry, non-color status/error equivalents, terminal resize behavior, and usable output with the selected screen-reader/terminal baseline. |

## Explicit non-goals and tradeoffs

- **No feature parity claim.** Live, Bitwig, REAPER, Ardour, and Pro Tools solve broader recording, live-performance, interchange, hardware, and post-production markets. Their presence in the evidence does not add those features to Crump.
- **No Session View, take comping, live audio recording, advanced waveform editing/warping, video, surround/immersive mixing, control-room system, or arbitrary external sync** for the first public release.
- **No mandatory plugin ecosystem.** Process isolation improves containment but adds IPC latency, scheduling complexity, state/version problems, and test-matrix cost. CLAP remains conditional; VST3 parity is not implied.
- **No arbitrary routing feedback or implicit channel coercion.** This gives up some DAW flexibility in exchange for static validation, bounded behavior, deterministic compensation, and readable source.
- **No universal mastering target or hidden normalization/limiting.** Report standardized loudness/true peak and honor explicit delivery policy. Monitoring safety processing remains non-authoritative and absent from offline rendering.
- **No promise of cross-machine bit identity for undeclared hardware, third-party plugins, changed engine versions, or nondeterministic external inputs.** Define the deterministic comparison domain precisely and report all render dependencies.
- **No opaque project container or DAW-project interchange requirement.** Human-readable source plus declared assets remains authoritative. Packaging may collect assets but must not replace that model.
- **Performance versus latency is explicit.** Smaller quanta reduce interaction latency but tighten callback deadlines; full compensation preserves alignment but can increase monitoring latency. Publish supported rate/quantum combinations and measured margins rather than claiming generic "low latency."

## Sources

| ID | Primary source | Relevant claim | Accessed |
| --- | --- | --- | --- |
| S1 | Ableton, [Live 12 Manual: Arrangement View](https://www.ableton.com/en/live-manual/12/arrangement-view/) | Linear arrangement, navigation, grids, clips, fades, time edits, meter markers | 2026-07-26 |
| S2 | Ableton, [Live 12 Manual: Mixing](https://www.ableton.com/en/live-manual/12/mixing/) | Metering, groups, returns, sends, Main, cue, feedback warning | 2026-07-26 |
| S3 | Ableton, [Live 12 Manual: Routing and I/O](https://www.ableton.com/en/live-manual/12/routing-and-i-o/) | Sources/destinations, monitoring, taps, submixes, sidechains | 2026-07-26 |
| S4 | Ableton, [Live 12 Manual: Automation and Editing Envelopes](https://www.ableton.com/en/live-manual/12/automation-and-editing-envelopes/) | Recording, override/re-enable, breakpoint editing, curves and locking | 2026-07-26 |
| S5 | Ableton, [Live 12 Manual: Working with Instruments and Effects](https://www.ableton.com/en/live-manual/12/working-with-instruments-and-effects/) | Plugin scanning behavior and device delay compensation | 2026-07-26 |
| S6 | Ableton, [Live 12 Manual: Managing Files and Sets](https://www.ableton.com/en/live-manual/12/managing-files-and-sets/) | Projects/assets, collection, lossless export, aligned tracks, real-time rendering | 2026-07-26 |
| S7 | Ableton, [Backup Sets](https://help.ableton.com/hc/en-us/articles/360000377870-Backup-Sets) and [Recovering a Set manually after a crash](https://help.ableton.com/hc/en-us/articles/115001878844-Recovering-a-Set-manually-after-a-crash) | Bounded backups and crash recovery files | 2026-07-26 |
| S8 | Ableton, [Live 12 Manual: Live Keyboard Shortcuts](https://www.ableton.com/en/live-manual/12/live-keyboard-shortcuts/) | Focus, navigation, editing, value entry, transport, export | 2026-07-26 |
| S9 | Bitwig, [Bitwig Studio Feature List](https://www.bitwig.com/feature-list/) | VST/CLAP crash protection and delay compensation | 2026-07-26 |
| S10 | CLAP, [`latency.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/latency.h) | Sample latency and restart/change contract | 2026-07-26 |
| S11 | CLAP, [`params.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/params.h) | Stable parameter IDs, plain values, automation/modulation, event timing | 2026-07-26 |
| S12 | CLAP, [`state.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/state.h) | Plugin state save/load and dirty notification | 2026-07-26 |
| S13 | CLAP, [`thread-check.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/thread-check.h) | Main/audio/background thread roles and real-time restrictions | 2026-07-26 |
| S14 | PipeWire, [Streams](https://docs.pipewire.org/page_streams.html) | Process callback, RT safety, buffer timing and stream state | 2026-07-26 |
| S15 | PipeWire, [RT module](https://docs.pipewire.org/page_module_rt.html) | RTKit/portal/rlimit scheduling and RT CPU-time safeguards | 2026-07-26 |
| S16 | Ableton, [Live 12 Manual: Computer Audio Resources and Strategies](https://www.ableton.com/en/live-manual/12/computer-audio-resources-and-strategies/) | Buffer-deadline CPU metric and worst-case workload testing | 2026-07-26 |
| S17 | Ardour, [Export Dialog](https://manual.ardour.org/exporting/export-dialog/) | Formats, time spans, overwrite checks, and written-file analysis | 2026-07-26 |
| S18 | Ardour, [Stem Exports](https://manual.ardour.org/working-with-sessions/interchange-with-other-daws/stem-exports/) | Silence-aligned stems and optional track/bus processing | 2026-07-26 |
| S19 | ITU-R, [Recommendation BS.1770-5](https://www.itu.int/rec/R-REC-BS.1770/en) | Programme loudness and true-peak measurement algorithms | 2026-07-26 |
| S20 | CLAP, [`render.h`](https://github.com/free-audio/clap/blob/main/include/clap/ext/render.h) | Real-time/offline modes and hard real-time requirements | 2026-07-26 |
