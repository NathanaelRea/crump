# Crump Product Contract

This document defines Crump's current product behavior. `CONTEXT.md` defines canonical domain terms, accepted ADRs define architectural boundaries, and the user guides describe operation.

## Product identity

Crump is a Linux-first terminal Guided Production Instrument for electronic and synthesized music. A Ratatui workspace leads human authoring, a small declarative language is the durable Project source, and external Authoring Agents use the same readable files and versioned JSON CLI as human-operated tools.

Crump has no embedded model, inference runtime, hosted backend, opaque project database, or alternate Agent-only editing path. Authoring, playback, saving, restoration, and rendering work without an Authoring Agent or paid service.

## Project authority and revisions

A Project contains readable `.crump` source plus explicitly declared Project-owned WAV assets. Source and declared media on disk form the Candidate Revision. The Active Revision is the latest valid snapshot published through Crump and is the only state used for playback and rendering.

A publication:

- validates the complete Project;
- rejects invalid syntax, semantics, graphs, routes, units, resource bounds, media identity, and non-finite authored values;
- records origin, actor, summary, and explanation;
- atomically stores source and media in Project History; and
- schedules a bounded runtime transition without making invalid Candidate state audible.

Structured edits are revision-checked, preserve unrelated source text where practical, validate the complete Project, publish atomically, and append a History Entry. Stale edits are rejected rather than redirected.

Project History is append-only and persists across close and reopen. Restore publishes the selected exact source-and-media snapshot as a new History Entry; it does not erase later entries or move a history pointer. Git is optional and is never invoked implicitly.

## Song and language model

A Project owns one finite Song on a linear Timeline. Song time starts at zero and uses exact rational Musical Time. Tempo and meter maps resolve authored positions deterministically. A Song contains:

- Tracks with Clips that reference reusable finite Patterns;
- concrete note, drum, and control Events;
- acyclic Pattern Variations;
- named Locators;
- deterministic Groove Templates;
- Groups, Returns, Sends, sidechains, and one Master; and
- point Automation, Pattern Parameter Locks, and routed Modulation.

Clips use half-open ranges, explicit `once` or `loop` traversal, optional source offsets, transpose, gain, fades, and Groove assignment. Arrangement-time operations state whether material follows or remains locked. Checked edits preserve durable IDs and do not silently flatten procedural material.

The DSL is declarative rather than general-purpose. It has no arbitrary code, mutable variables, opaque plug-in state, or hidden project database. Durable authored objects carry explicit identities; missing or duplicate identities invalidate the Candidate.

## Media and generated audio

A Media Asset is a Project-relative `.wav` file identified by an exact lowercase SHA-256 digest. Import copies bytes into the Project; wavetable import additionally authors an exact mono frame count/length, normalization, and morph interpretation; collect moves valid declarations to canonical Project media paths. Absolute paths, traversal, symlinks, unsupported formats or table shapes, changed bytes, duplicate IDs, and resource-limit violations are rejected.

Sampler Instruments and source-v2 graph `sample_source`/`multisample`/`granular`/`spectral` Nodes share one off-callback decode/resample cache. Samplers support bounded mappings and sample occurrences. Graph sources add exact slices/zones, forward/reverse and bounded loop laws, gated/one-shot playback, repitch-only rate, velocity/key zones, layers, deterministic round robin, gain/pan, and fixed Track-owned voice state; unsupported articulations and transparent time stretching fail closed. Granular voices add 1–64 preallocated semantic-identity grains, bounded density scheduling, finite windows/edge laws, scan/freeze, spray/direction, stereo spread, and a one-second maximum source tail. Spectral voices consume an immutable referenced-only `crump-spectral-peaks-v1` derivative using 2,048/256 periodic-Hann analysis, at most 128 deterministic peaks, independent scan/rate and root-relative pitch, bounded regions, transient policies, and preallocated phase state. Source-v2 `wavetable` graph Nodes instead compile declared consecutive periodic frames into immutable, versioned, Nyquist-bounded mip tables off-callback. Exact Media Asset bytes remain authoritative for every derivative family.

Bounce captures one exact Active Revision from Master or a selected Track, Group, or Return tap. It publishes stereo float WAV bytes and readable provenance atomically without replacing source material. Sample preparation creates a new exact-frame WAV from one source region with independent pitch displacement, the versioned `crump-wsola-effect-v1` algorithm, and `shift` formant policy. Original assets remain unchanged.

Media Tracks, recording, comping, proprietary warp modes, formant preservation, and realtime routed pitch processing are unsupported.

## Instruments, routing, and processing

Native Instruments include stock synthesized drums, sampler Instruments, and polyphonic Composable DSP Graphs. Graphs use typed Event, mono/stereo Audio, and uni/bipolar Control ports, explicit conversion and bounded summing, separate per-voice and one optional Track-owned shared post-voice domain, bounded resources, deterministic processing, and declared latency. Stable source version 2 provides durable Bus IDs, Instrument-owned Modulation, expressive voicing/virtual-analog Nodes, exact-asset wavetable/sample/multisample/granular/spectral Nodes, and sound-neutral generated-section metadata while retaining the complete version-1 floor.

Routing compiles Tracks, nested Groups, Returns, Sends, named sidechains, and Master into one acyclic graph. Channel processing order, pre/post-fader taps, deterministic fan-in, and convergence latency compensation are explicit. Solo, cue, and transient processor bypass are monitoring state and do not affect source or offline rendering.

Native processors are Utility, EQ, Compressor, Gate/Expander, Delay, Reverb, Saturation, Chorus/Flanger, Crusher, Stutter, Limiter, and the testing/compensation latency processor. Processor settings, durable bypass, ordering, latency, state policy, work, and memory are validated and inspectable. Unknown settings and no-op declarations are rejected.

Third-party plug-in hosting, user-supplied native extensions, Agent-generated Rust, arbitrary DSP scripting, and live microphone input are unsupported.

## Parameters and control

Every Parameter has a stable owner/key identity, kind, physical unit, legal domain, default, and declared Automation/Modulation capabilities. Current kinds are continuous real, bounded integer, boolean, and enum. Only continuous values accept non-step interpolation or Modulation.

Effective values apply base value, Song Automation, optional Pattern Parameter Lock, and additive Modulation in that order. Evaluation is sample-accurate and independent of callback grouping. Values outside authored domains invalidate the Candidate; finite runtime modulation overshoot is constrained to the target domain, and non-finite control results retain the last finite value with a diagnostic.

Routed LFOs and the channel-tap Envelope Follower are available Modulation sources. Recorded Automation, MPE, OSC, MIDI clock/transport, controller mapping, SysEx authoring, and broad MIDI interchange are unsupported.

## Workspace and CLI

The terminal workspace provides keyboard operation for Timeline, Event Grid, Faceplate, Parameters, Automation, Processing, Graph, Source, Diagnostics, and History views. Timeline provides a fit/zoom whole-Song arrangement grid with authored Clip ranges, canonically materialized note/hit activity, Locators, playhead, and exact-asset source-waveform envelopes for sampler occurrences; it does not imply Media Track or post-processor waveform state. Exact value entry and source navigation are available without pointer input. Graph topology editing remains source-based; the Graph view inspects nodes, connections, Parameters, mappings, and source locations.

The CLI provides validation, inspection, status, publication, checked edits, playback, rendering, offline workload measurement, asset operations, module operations, and History operations. Non-interactive commands accept `--json` and emit one versioned response containing `schema_version`, `ok`, `command`, and either `data` or `error`. The interactive workspace rejects `--json`.

External Authoring Agents edit normal Project files and call the same machine-readable CLI. Every intended valid save requires explicit publication with Agent origin and explanation. Agents do not scrape the TUI or access hidden state.

## Playback and transitions

Playback uses native PipeWire stereo float audio at the fixed 48 kHz Project rate. The documented environment is Ubuntu Desktop 26.04 LTS `amd64` with PipeWire, WirePlumber, RTKit, in-tree ALSA drivers, a wired stereo output device, and optional class-compliant USB MIDI.

Transient MIDI Note Audition accepts Note On/Off and velocity for the selected Native Instrument without creating Events or History. Selection changes retain the original note destination so Note Off reaches the voice that received Note On.

Runtime construction, parsing, media decode, allocation, and large-state preparation occur away from the audio callback. Compatible publication retains compatible voices and processor state and ramps only changed continuous targets. Topology changes use an aligned equal-power transition, restart affected voices/effects, and preserve transport and semantically unchanged Tracks.

Bluetooth, network audio, multichannel output, capture, aggregate devices, JACK-only operation, containers, SSH-only sessions, WSL, macOS, Windows, ARM, and headless playback are unsupported.

## Rendering and measurement

Offline rendering consumes one exact Active Revision and the same semantic schedule and DSP implementations as playback below the monitoring safety stage. The render range covers Song time zero through arrangement end plus the authored non-negative Render Tail.

A render publishes an atomic bundle containing a stereo 48 kHz IEEE 32-bit-float Master WAV, a versioned manifest, and any selected aligned Track/Group Stem WAVs. The manifest records revision, build and executable identity, range, format, frame count, per-file hashes, and finalized-file analysis. Repeating a render with the same Project, settings, executable, sample rate, and documented `x86_64` comparison domain produces identical bytes.

Finalized-file analysis reports sample peak, a named fixed-oversampling reconstructed-peak estimate, BS.1770/EBU-derived loudness measurements, per-channel DC, clipping, non-finite samples, and analyzed frame count. Crump does not normalize audio or enforce a universal loudness target.

`qualify` measures one exact offline Active range at a selected 64, 128, or 256-frame quantum and can atomically publish a versioned report. `--require-pass` preserves the report and returns a failed gate when automated thresholds miss. It measures offline engine timing and finite output; it does not measure PipeWire xruns or device scheduling. Callback allocation/deallocation is separately attested on warmed normal, command, publication-transition, and reclamation paths; lock/blocking/I/O absence remains reviewed call-path evidence.

## Resource and support limits

Validation and compilation enforce finite Project, schedule, graph, control, media, processor, voice, work, and memory bounds before runtime publication. Numeric limits are documented in the language and support guides and are available through inspection.

Crump does not promise byte identity across builds or architectures, low-latency behavior on undocumented hardware, standards conformance beyond the named analysis method and tested vectors, accessibility solely because the interface is text, or sonic equivalence with named products.

Stable source version 2 and the named sample, wavetable, spectral, schedule, and qualification algorithm identities are support contracts. Filter/default/category sound compatibility and final Serum-class acceptance remain experimental until independent-producer and named Linux/PipeWire/device endurance evidence passes; automated templates or offline reports do not satisfy those gates.
