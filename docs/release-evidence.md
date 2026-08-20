# First Public Release Evidence

**Status: NOT YET RELEASE-CERTIFIED**

This file separates completed implementation/automation evidence from gates that require named reference hardware or independent humans. A passing build is not substituted for a musician evaluation, and a short developer-machine playback smoke test is not called a real-time qualification.

## Acceptance fixture

- Source: [`fixtures/reference/project.crump`](../fixtures/reference/project.crump)
- Name: Neon Steps
- Source revision: `877271f61eb4c9852a4f4097c967fb81cf1ffb41b3c80f7c25155d9d7cc99c8b`
- Tempo/meter: 112 BPM, 4/4
- Arrangement: 16 bars, 34.285708 seconds
- Render tail/range: 2 seconds / 36.285708 seconds
- Roles: polyphonic Neon Lead, polyphonic Pulse Bass, stock Circuit Drums
- Sections: four-bar introduction, eight-bar contrasting middle, four-bar return
- Assets: no media files; all sound is synthesized
- Creative graph processing: low-pass, saturation, delay, and reverb

`crump check` passes all seven objective checks: valid Project, 30–60 second arrangement, graph Track, stock-percussion Track, section variation, creative processing, and no media assets.

## Automated evidence completed

The current source has demonstrated:

| Area | Evidence |
| --- | --- |
| Compilation | Stable all-target/all-feature and headless checks succeed; isolated `cargo +1.88.0 check --all-targets --all-features --locked` proves the declared MSRV |
| Unit/integration | 60 library tests + 11 end-to-end CLI tests + 2 Reference Project tests pass |
| Static quality | `cargo fmt --all --check`, `git diff --check`, primary Rust diagnostics, and strict locked Clippy succeed |
| Source safety | Full block ranges, import traversal rejection, source hashing, multi-diagnostic recovery, unit/domain and graph validation tests; creative-processing checks ignore unused Instruments |
| Schedule/engine | Exact-time rounding, bounded half-open schedules, Audition Phrase looping, deterministic voices/drums, and affected-Track transition tests |
| Persistence | Exact minimal replacement, append/list/diff/restore, stale rejection, schema/identity checks, source transaction journaling, provisional-state rollback, committed-transaction completion, and restart recovery |
| Modules | Semantically validated standalone export/install/inspect, bounded input, atomic no-replace publication, conflict coverage, and exact library-snapshot restore |
| Agent contract | One-response JSON usage/command coverage, stable exit classes, checked Event edits, and required Agent rationale/provenance |
| Rendering | Non-mutating exact-range planning, repeated unchanged WAV/manifest comparison, non-finite failure, overlap/symlink/unrecognized/tampered-destination rejection, no-replace preservation, and atomic bundle exchange |
| Workspace | Pseudo-terminal startup/draw/quit smoke test; source editor handoff; source-backed Event controls; selectable Node/Bus/Parameter/Mapping source navigation; and documented keyboard commands |
| PipeWire | Native audio stream starts on the local developer machine; setup/first-callback waits are bounded; stopped streams are observable; bounded queues and MIDI parser/recovery tests pass |
| Packaging | `cargo package` verifies; the `.deb` builder installs guides/fixture/Authoring skill, emits an external checksum sidecar, supports reproducible fallback construction, and has archive/extracted-binary checks in pinned Ubuntu CI |

The test suite specifically proves that an invalid Candidate cannot replace the Active Revision and that `render` still uses the exact Active snapshot in that state.

## Local deterministic render record

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
