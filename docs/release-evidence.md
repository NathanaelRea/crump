# First Public Release Evidence

> **Status:** NOT YET RELEASE-CERTIFIED

This file separates completed implementation/automation evidence from gates that require named reference hardware or independent humans. A passing build is not substituted for a musician evaluation, and a short developer-machine playback smoke test is not called a real-time qualification.

## Acceptance fixture

- Source: [`fixtures/reference/project.crump`](../fixtures/reference/project.crump)
- Name: Neon Steps
- Source revision: `582c2119c62e2c7d54a83848ff5d71bec00a8b8c0f646a2696fc4607525240b2`
- Tempo/meter: 112 BPM, 4/4
- Arrangement: 16 bars, 34.285708 seconds
- Render tail/range: 2 seconds / 36.285708 seconds
- Roles: polyphonic Neon Lead, polyphonic Pulse Bass, stock Circuit Drums
- Sections: four-bar introduction, eight-bar contrasting middle, four-bar return
- Assets: no media files; all sound is synthesized
- Creative graph processing: low-pass, saturation, delay, and reverb
- Production Control Foundation 1A arc: lead cutoff, delay mix, room, Track level, and Track balance point Automation

`crump check` passes all seven objective checks: valid Project, 30–60 second arrangement, graph Track, stock-percussion Track, section variation, creative processing, and no media assets.

## Automated evidence completed

The current source has demonstrated:

| Area | Evidence |
| --- | --- |
| Compilation | Stable all-target/all-feature and headless checks succeed; isolated `cargo +1.88.0 check --all-targets --all-features --locked` proves the declared MSRV |
| Unit/integration | 206 library tests + 22 end-to-end CLI tests + 2 expressive-Instrument tests + 2 production-scale tests + 2 Reference Project tests pass |
| Static quality | `cargo fmt --all --check`, `git diff --check`, primary Rust diagnostics, and strict locked Clippy succeed |
| Source safety | Full block ranges, source/media traversal and symlink rejection, source-v1 compatibility, asset-aware exact-byte hashing, multi-diagnostic recovery, unit/domain and graph validation tests; creative-processing checks ignore unused Instruments |
| Schedule/control/engine | Exact-time rounding/snapping, bounded half-open schedules, recursive off-callback Pattern Variation materialization, seeded Groove transforms, Clip-scoped Parameter locks, Automation/lock/Modulation provenance (including step-only Mute), variable-block sample equivalence, per-Track graph-voice admission, deterministic Track/Group/Return/Master routing, convergence compensation, all-channel meters, offline-safe Solo/cue, exact unchanged Automation across publication, compatible processor-state retention, Audition Phrase/range looping, deterministic voices/drums, and topology-affected transitions |
| Persistence | Exact minimal replacement, append/list/diff/restore, stale source/media rejection, legacy sample-free schema compatibility, content-addressed binary blobs, exact media restore, source-and-binary transaction journaling, provisional-state rollback, committed-transaction completion, tamper rejection, and restart recovery |
| Modules | Semantically validated standalone export/install/inspect, bounded input, atomic no-replace publication, conflict coverage, and exact library-snapshot restore |
| Agent contract | One-response JSON usage/command coverage, stable exit classes, checked Event/Automation/mix-target and Timeline/Groove/Variation/lock edits, atomic no-replace WAV import/list/collection, media metadata inspection, effective-value and compiled-route inspection, and required Agent rationale/provenance |
| Rendering | Non-mutating exact-range/stem planning, aligned pre/post Track/Group stems with explicit inclusion policy, 16-Stem raw spool shards, a 192-Stem maximum-topology render, post-spool failure cleanup/destination preservation, exact neutral-mix stem reconstruction, repeated unchanged WAV/manifest comparison, finalized-file peak/loudness/DC/clipping/non-finite analysis vectors, duplicate/unknown selection rejection, stemless↔stemmed exact-set replacement, per-file hash/frame/non-finite/recomputed-analysis failure, overlap/symlink/unrecognized/tampered-destination rejection, no-replace preservation, and atomic multi-artifact bundle exchange |
| Qualification | Exact Active/range/64–256-full-quantum offline workload plans and schema-1 reports, exact-deadline disclosed short final blocks, compile/build/host/resource identities, conservative fixed-histogram p99.9 and missed-deadline evaluation, deterministic fake-timer pass/fail/overflow tests, non-aligned all-supported-quantum range tests, protected atomic report conflict behavior, and explicit `null` PipeWire xrun/external-evidence fields |
| Workspace | Pseudo-terminal startup/draw/quit smoke test; source editor handoff; source-backed Event, Automation, and Mixer controls; Locator/Groove/Clip-transform Timeline context; Pattern Variation and Parameter-lock Event Grid inspection; all-channel peak/average display; transient Solo/cue command seam; effective Parameter provenance; selectable Node/Bus/Parameter/Mapping source navigation; and documented keyboard commands |
| PipeWire | Native audio stream starts on the local developer machine; setup/first-callback waits are bounded; stopped streams are observable; bounded queues and MIDI parser/recovery tests pass |
| Packaging | `cargo package` verifies; the `.deb` builder installs guides/fixture/Authoring skill, emits an external checksum sidecar, supports reproducible fallback construction, and has archive/extracted-binary checks in pinned Ubuntu CI |

The test suite specifically proves that an invalid Candidate cannot replace the Active Revision and that `render` still uses the exact Active snapshot in that state.

## Production roadmap engineering evidence — not acceptance

The codebase also contains unaccepted roadmap work beyond the certified First Public Release subset:

- Slice 4 engineering now implements Project-level portable WAV declarations, normalized path and exact-hash validation, bounded immutable `AssetInventory`, missing/changed/duplicate/unsupported diagnostics, atomic external import and canonical collection, asset-aware Candidate revisions, and content-addressed exact-byte History/Active/restore/recovery. Asset-free Projects preserve source-v1 revision identity and legacy snapshots without an asset manifest still load. Verified WAVs compile off-callback into immutable project-rate sample sources. Sampler Instruments provide mapped notes, durable one-shot/gated occurrences, original-source regions, repitch/gain/pan/envelopes/fades, choke groups, deterministic voice replacement, and checked place/split/move/duplicate/trim/slip/fade operations. Unit/integration tests prove deterministic playback and rendering behavior. Media Tracks, time stretching, musician workflow evidence, clean-account portability evidence, and Slice 4 acceptance remain open.
- Slice 5 engineering adds deterministic 1–8-subvoice stereo unison with width/drift, oscillator glide, velocity-aware amplitude/timbre paths, typed envelope-to-filter modulation, key tracking, bounded pitch modulation, and low-pass/high-pass/band-pass/notch filtering. Graph voice replacement uses a preallocated second state slot and fixed 64-frame crossfade; admission counts that storage and work. Five readable modules and an imported Project under `fixtures/expressive/` exercise pad, pluck, bass, percussion, and transition roles with shared post-voice tails. `tests/expressive_instruments.rs` freezes finite sample-exact 64/128/256-frame output and admitted resource facts; `docs/expressive-instruments.md` records the progressive controls and costs. Filter-mode listening selection, independent producer judgments, mono/stereo listening evidence, and Slice 5 acceptance remain open.
- Slice 6 engineering adds checked durable Clip create/delete/move/resize/duplicate/split and local offset/transpose/gain/fade/Groove operations; named Locators and exact Locator audition ranges; exact grid-aware or explicitly free timing edits; atomic insert/delete/duplicate arrangement-time transforms with half-open follow/locked Automation policy; reusable seeded Groove swing/timing/velocity/probability with boundary-safe onset retention; acyclic source-backed Pattern Variations materialized off-callback without flattening; and Pattern-relative physical-unit Parameter locks with base → Automation → lock → Modulation precedence and Clip-end reset. Timeline and Event Grid inspection distinguish shared, locally treated, and procedurally varied material. Automated CLI and engine tests prove checked publication, canonical probability identity, recursive transform chaining, deterministic schedules, lock provenance, and range planning. A target-musician complete-song Production View run, no-source-edit workflow evidence, and Slice 6 acceptance remain open.
- Slice 7 production-scale engineering freezes `fixtures/production/`, a self-contained 96-bar, 120 BPM (3:12) Project with a repository-retained exact-hash portable PCM one-shot, Sampler, expressive graph Instruments, stock percussion, Automation, Parameter locks, Pattern Variations, Grooves, sidechain, Groups, Return, Sends, and Master processing. `tests/production_scale.rs` verifies its 180–300 second duration, exact frame range, media/native coverage, resource admission, finite Audio, sample-exact 64/128/256 grouping, and an isolated silent-before-onset/nonzero-after-onset Sampler path. No listening claim follows from this fixture.
- Slice 7 qualification engineering adds `qualify`: a non-mutating plan and optional atomic schema-1 report for one exact Active Locator range at 64, 128, or 256 offline frames. It records revision/executable/host/resource identities, exact full/final block sizes, block timing, realtime factor, mean/peak/conservative fixed-histogram p99.9 utilization, missed deadlines, and finite output. Fake-timer tests prove pass/fail and overflow reporting; non-aligned tests prove exact ranges/deadlines at every supported quantum. Report publication rejects Project source, declared media, and History destinations. The [archived unsupported-host probe](evidence/production-scale-offline-2026-08-21/README.md) profiles the fixture's authored peak `drop`→`break` range at every promised quantum and retains one failed 64-frame run rather than hiding its single offline missed deadline. The profiler deliberately records no PipeWire xruns and names the missing target-graph, 30-minute named-hardware, and callback-safety evidence; this does not close production qualification.
- Slice 7 delivery engineering reopens every finalized 48 kHz stereo float Master/Stem WAV and publishes a versioned measurement object in the schema-2 hashed manifest. Analysis schema 2 reports sample peak/dBFS, a named custom 4× reconstructed peak/dBTP estimate, BS.1770-derived K-weighted gated integrated loudness, maximum three-second short-term loudness sampled at 10 Hz, Tech 3342-derived Loudness Range, per-channel DC, clipping counts, non-finite count, and analyzed frames. Synthetic vectors cover silence, DC/clipping/non-finite data, truncated/corrupt files, an inter-sample overload, the official EBU steady-tone loudness tolerance, and EBU true-peak steady-tone cases 15–19; the complete official signal set and transient true-peak cases remain open, so formal EBU/BS.1770 conformance is not claimed. Render publication rejects non-finite or frame-inconsistent artifacts. Repeated `--stem` selections export durable Track/Group pre/post-fader taps with explicit upstream/owner/downstream processing policy, Master-latency padding, identical frame count/tail, independent analysis/hash, deterministic names/order, bounded raw spool shards, and atomic artifact-set replacement. The latency fixture proves neutral-mix stem reconstruction, duplicate/unknown rejection, stemless↔stemmed replacement, and WAV/analysis tamper rejection; a maximum admitted topology renders all 192 selectable Track/Group taps while opening at most 16 Stem WAVs per shard, and an injected post-spool failure preserves the exact prior destination and removes staging. Existing bundles are reopened and reanalyzed before exchange. No normalization or universal LUFS target is applied. Additional format decisions, production-scale/fault qualification, independent producer delivery, and Slice 7 acceptance remain open.
- `CompiledMixGraph` compiles stable Track, Group, Return, and Master roles with ordered processors, durable pre/post Sends, named sidechains, deterministic fan-in, source-to-Master signal paths, and all-channel telemetry.
- `tests/fixtures/production_mix.crump` exercises every Slice 2 routing role plus an authored Master chain.
- Impulse tests prove zero-sample main, parallel Send/Return, and sidechain convergence at 64/128/256-frame quanta; a production-routing fixture proves monitoring and offline authored results agree below the separate monitoring safety stage.
- Compile-time inspection publishes named work/memory budgets and 2× publication headroom, and validation rejects oversized candidates before runtime allocation.
- Stable source-backed edits cover channel fader/panner/Mute, Send, processor, and Master targets. Compatible shared processor state survives publication; Solo/cue remains transient and offline-inert.
- Processor inspection resolves effective defaults, physical domains, authored/continuous-target provenance, latency, nonlinear evaluations, filtered oversampling factor/tap count, and allocation/work facts. Fixed-capacity telemetry uses stable owner/processor indexes across compatible publication and reports compressor reduction independently from makeup gain.
- Admission counts each repeated Instrument post-chain runtime instance and separately preserves graph-voice/Node allocations through compiled resource recalculation. Allocation-accounting tests match exact shared and per-voice Delay/Reverb heaps and reject a 256-voice graph whose two-second per-voice Delay would exceed publication headroom.
- Utility tests prove gain, polarity, mono, mid preservation, side narrowing, and non-decreasing stereo correlation. EQ tests cover finite decaying impulses, measured cut/shelf/parametric responses including Q behavior, every active-band inspection value, and exact output across 64/128/256-frame chunking.
- Compressor vectors verify linked peak/RMS detection, one-pole attack/release, threshold/ratio/soft-knee behavior, exact wet/dry endpoints, wet-path makeup, external detector replacement, and gain-reduction telemetry. `tests/fixtures/compressor_sidechain.crump` is a source-backed kick/sub-bass project; its compiled route test isolates the trigger in the audible sum and proves attack, ducking, release recovery, and telemetry movement.
- Delay tests verify straight/dotted/triplet tempo resolution, fractional interpolation, filtered feedback, independent-stereo versus cross-fed ping-pong echoes, preallocated read-head crossfades, and finite maximum-feedback operation during repeated time changes. Validation and compilation reject conflicting free/synchronized settings; inspection exposes effective synchronized time and exact state admission.
- Reverb uses fixed preallocated stereo comb/gain-neutral-allpass networks. Vectors verify exact static pre-delay offsets through the one-second boundary, click-safe non-integral pre-delay changes and half-frame interpolation, diffusion frequency/DC gain, decay and damping movement, deterministic fractional-delay modulation, side-only stereo input, mono-compatible wet width, finite impulse and sustained-input stability, one shared Return state across multiple Sends, exact allocated DSP-state bytes, and a stable declared abstract-work estimate.
- Saturation and Limiter share a fixed filtered 4×, 129-tap linear-phase FIR kernel. Vectors verify round-trip/interpolator latency, dry-path alignment, mode-specific THD, strong alias suppression, inter-sample ceiling control, authored release recovery, exact state accounting, finite boundary operation, and exact output across 64/128/256-frame grouping.
- Runtime admission counts Arc-shared mix, control, schedule, and Instrument programs; scheduled Event identities; complete Song/Track/control/mix container backing (including machine-word-rounded packed Boolean vectors); stock-drum voice storage; graph voices; native DSP state; and exactly two resident publication slots. An RAII reservation is acquired before replacement compilation/allocation, and an atomic one-replacement gate prevents the command and reclaim rings from retaining additional runtimes. A busy gate retains the newest Active revision for retry. Controller-side publication resets large DSP storage in place; callback transition preparation only realigns cursors, and a storage-fingerprint test verifies that repeated transport realignment preserves every tracked heap address/capacity. Allocator-private bookkeeping is not claimed as byte-exact.
- `tests/fixtures/production_mix.crump` now exercises every native processor kind in the complete Track/Group/Return/Master topology. Integrated tests prove a decaying post-arrangement Return tail, identical authored monitoring/offline output below the safety stage at 64/128/256-frame quanta, and structural admission with two-publication residency. Limiter lookahead is structural and non-automatable; inspection discloses its reconstructed-peak detector latency and fixed 0.1 dB safety margin.

These are engineering facts only. Named-hardware workload, independent producer listening, Slice acceptance, and release certification remain open.

## Archived pre-Foundation deterministic render record

The record below belongs to Reference Project revision `877271…`, before Production Control Foundation 1A and its Automation arc changed the fixture. It remains useful historical evidence for the predecessor runtime but **does not certify or provide current render hashes for revision `582c211…`**. Current deterministic behavior is covered by automated repeated-render and variable-block tests; a frozen release artifact record is still required.

Captured 2026-08-19 on an **unsupported/non-reference** Omarchy/Arch developer host:

```text
OS release:          omarchy/4.0.0
Kernel:              7.1.8-arch1-3
Architecture:        x86_64
Rust/Cargo:           1.96.1
PipeWire:            1.6.8
ALSA library:         1.2.16.1
Reference flag:      false
```

Two independent release-mode render invocations produced byte-identical WAV and manifest files:

```text
Project revision:    877271f61eb4c9852a4f4097c967fb81cf1ffb41b3c80f7c25155d9d7cc99c8b
Frames:              1,741,714
Duration:            36.28570833333333 s
Peak:                0.24394135
WAV SHA-256:         a0854ec27e423ace9c9acfab90253493b73072c87f959db6d6846251b98562e3
Manifest SHA-256:    e815de7acf4997b361602a8024f1c1bcc8e6f5d90c318e0aaee64973d7e9e944
Executable SHA-256:  51778e913fa00e3661f1f1efabed88e4a98bf5393c2d2887be505f62fbb9a514
Observed wall time:  3.19 s and 3.28 s
```

This confirms a local deterministic comparison only. The executable hash is a transient untagged working-tree build and must be replaced by the final signed release artifact record.

## Local package construction record

On the same unsupported developer host, `cargo package --locked --allow-dirty` packaged and verified the Rust crate. The Debian builder then used its deterministic GNU `ar`/GNU `tar` fallback because `dpkg-deb` was unavailable locally. Two independent package invocations were byte-identical, and each generated checksum sidecar verified:

```text
Package:             crump_0.1.0_amd64.deb
Checksum sidecar:    crump_0.1.0_amd64.deb.sha256
Archive members:     debian-binary, control.tar.xz, data.tar.xz
Installed binary:    usr/bin/crump (mode 0755)
Installed guides:    language, CLI/Agents, workspace, audio/MIDI, history/render, support, evidence
Installed extras:    Reference Project and crump-authoring/SKILL.md
Smoke result:        crump 0.1.0
```

The archive was extracted and its control metadata, file modes, documentation, fixture, skill, and executable were inspected. The archive hash intentionally lives in the external `.sha256` sidecar rather than this packaged evidence file, which cannot contain its own final archive hash without changing that hash. This remains an untagged working-tree candidate and does not substitute for `dpkg` install/upgrade/uninstall testing on Ubuntu 26.04 or signature verification.

## Local PipeWire smoke record

The same unsupported developer host played the Reference Project for five seconds through native PipeWire:

```text
Callbacks:                    20
Late callbacks:               0
Current utilization:          0.1391
Peak utilization:             0.1643
Approximate p99.9 utilization: 0.17
Processing errors:            0
MIDI dropped:                 0
Frames processed:             245,760
```

The server-selected buffers were much larger than the intended 256-frame reference quantum. This record proves connection and audible processing only; it does not satisfy latency, xrun, endurance, or hardware support gates.

Utilization is process time divided by represented buffer duration. p99.9 is maintained callback-safely in a fixed 1%-wide histogram and is therefore a conservative bucket approximation. PipeWire xrun evidence must still come from the server (`pw-top`) alongside Crump counters.

## External Authoring Agent workflow — OPEN

The machine contract and provider-neutral skill are implemented, but release evidence still needs one documented harness/model configuration available at evaluation time. Starting from `crump new`, record that the external Agent can read all explicit imports, create the briefed valid graph/percussion sketch, consume diagnostics, repair invalid work, run `check`, publish with rationale while the TUI is closed, respond to one ordinary-language listening revision, export/reuse the Instrument Module, and render without scraping the TUI or using hidden state. Archive the harness/model/version, skill version, commands, Candidate/Active identities, History entries, and any human intervention. Automated CLI tests do not substitute for this integration run.

## Required reference-hardware run — OPEN

Before a public tag, freeze and record:

- Ubuntu Desktop 26.04 LTS image/update date and kernel;
- CPU, memory, motherboard/USB topology;
- named wired stereo USB interface, USB IDs, profile, firmware, ALSA driver;
- named class-compliant USB MIDI 1.0 controller and USB IDs;
- PipeWire/WirePlumber/RTKit versions and configuration;
- actual 48 kHz / 256-frame graph operation and granted scheduling; and
- final release binary/package hashes.

Then run the worst Reference Project section continuously for at least 30 minutes. Acceptance target:

- 0 PipeWire xruns;
- 0 Crump late callbacks;
- 0 processing errors or non-finite samples;
- 0 callback allocations, blocking/contended locks, parsing, graph construction, filesystem/console I/O;
- p99.9 callback utilization ≤ 70% and recorded maximum;
- no audible dropouts while navigating all views;
- bounded valid-publication transition latency with no unsafe discontinuity;
- 100% invalid Candidate attempts leave prior Active audio unchanged; and
- device disconnect and MIDI queue stress leave no stuck note or unsafe output.

The callback implementation is designed for these constraints, but code structure is not a substitute for measurement.

## Physical MIDI run — OPEN

On the named controller, record:

- Note On/Off and velocity on all used channels;
- selection change while holding a note (Note Off must reach the original Track);
- repeated notes and zero-velocity Note On;
- CC 120/123 recovery;
- cable unplug/replug while notes are held;
- queue-overflow panic recovery;
- graph-Instrument playability and stock drum mapping; and
- proof that audition creates no source or History change.

Automated SPA sequence, MIDI decode, original-Track Note Off, and overflow panic tests are complete. No claim of USB hardware evidence is made yet.

## Independent musician evaluation — OPEN

Neither evaluator may be a Crump implementer or prior author of the fixture.

### Beginner Musician

Give only the Creative Brief and user documentation. Record whether they can, without developer intervention:

1. open and understand Candidate vs Active;
2. start playback and follow the playhead/activity;
3. adjust a Faceplate control and enter an exact value;
4. alter a note in Event Grid;
5. inspect an Agent Change and diagnostics;
6. restore an earlier state;
7. save, exit, reopen, and hear the same Active state; and
8. render and locate the master.

Record completion, errors, requests for help, terminology confusion, confidence, and whether the result feels like their authorship.

### Experienced Producer

Record whether they can inspect and modify the same fixture and judge:

- terminology and transferable workflow;
- exact Parameter units/domains and source relation;
- Faceplate usefulness versus graph/source depth;
- Event/Clip/Track semantics;
- graph topology and modulation limits;
- playback transition behavior;
- synthesized audio and simple balance as musically credible for the narrow sketch; and
- missing capability that most blocks a next Validated Slice.

Accessibility and professional credibility remain separate outcomes; one evaluator cannot substitute for the other.

## Accessibility review — OPEN

Keyboard reachability is implemented, but a text interface is not automatically accessible. Manual review must cover:

- visible focus and non-color state labels;
- logical Tab/view/selection traversal;
- exact input and modal cancellation;
- terminal resizing and small-screen behavior;
- screen-reader output on the selected terminal baseline;
- diagnostics and History diffs without visual-only cues; and
- complete workflow with no pointer.

## Packaging and recovery qualification — OPEN

Crate verification, repeatable local archive construction, and best-effort Ubuntu 24.04 CI build/inspection are automated. Before release:

- build and install the final unconfined `.deb` on a clean reference account;
- verify declared runtime dependencies and uninstall/upgrade behavior;
- copy the fixture to a second clean account and rerun validation/render comparison;
- fault-inject every source publication, History commit, restore, module install, and render-publication step;
- verify each interruption exposes the complete old or new state and restart recovery is idempotent; and
- archive commands, logs, hashes, manifests, `pw-top`, and evaluator records with the tag.

## Release decision

A release decision is **blocked** while any OPEN section above lacks evidence. The implementation can be tested and developed now; it must not be described as having passed the full First Public Release Acceptance Gate until this file is updated with named, reproducible records and independent evaluator signoff.
