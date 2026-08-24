# Verification

This page identifies repository fixtures and checks that exercise documented Crump behavior, along with the limits of those measurements.

## Reference Project

[`fixtures/reference/project.crump`](../fixtures/reference/project.crump) is the compact end-to-end fixture:

- **Name:** Neon Steps
- **Tempo and meter:** 112 BPM, 4/4
- **Arrangement:** 16 bars, 34.285708 seconds
- **Render tail and range:** 2 seconds and 36.285708 seconds
- **Roles:** polyphonic Neon Lead, polyphonic Pulse Bass, and stock Circuit Drums
- **Structure:** four-bar introduction, eight-bar contrasting middle, and four-bar return
- **Media:** none; every sound is synthesized
- **Processing:** low-pass filtering, saturation, delay, and reverb
- **Automation:** lead cutoff, delay mix, room, Track level, and Track balance

`crump check` verifies a valid Project, the expected duration, a graph Track, stock percussion, section variation, creative processing, and absence of Media Assets.

## Workload fixture

[`fixtures/production/project.crump`](../fixtures/production/project.crump) is a self-contained 96-bar, 120 BPM Project. It combines a declared WAV asset with sampler and native graph Instruments, Automation, Parameter Locks, Pattern Variations, Grooves, sidechains, Groups, a Return, Sends, and Master processing.

`tests/production_scale.rs` checks its exact range, media and native coverage, resource admission, finite Audio, equivalent 64/128/256-frame grouping, and isolated sampler onset behavior. This fixture verifies engine and resource behavior; it does not measure subjective listening quality or hardware playback.

## Automated checks

The Rust and CLI suites cover:

| Area | Checked behavior |
| --- | --- |
| Source and validation | Lossless source edits, source locations, multi-diagnostic recovery, media traversal/symlink rejection, IDs, units, graph typing, routing, resource bounds, and non-finite authored values |
| Schedule and control | Exact-time rounding, half-open schedules, Pattern Variations, Groove transforms, Parameter Locks, Automation/Modulation provenance, deterministic event identity, and callback-grouping equivalence |
| Audio engine | Graph voices, stock drums, sampler traversal and loops, routing, latency compensation, native processor vectors, finite output, tails, bypass, state retention, and publication transitions |
| Persistence | Candidate/Active isolation, append/list/diff/restore, stale source/media rejection, content-addressed media, transaction recovery, and tamper rejection |
| Modules and assets | Module export/install/inspect, WAV import/collect/list, generated-audio provenance, exact media restore, and atomic no-replace behavior |
| CLI and Agents | One-response JSON schema, exit classes, checked edits, required publication rationale, and source/Active/History/render isolation |
| Rendering | Exact range planning, deterministic Master output, aligned Stem taps, finalized-file analysis, hash/frame checks, destination validation, and atomic bundle exchange |
| Offline measurement | Exact Active range and quantum, disclosed final block, timing statistics, missed deadlines, finite output, identity fields, and atomic report publication |
| Workspace | Terminal startup/draw/quit, keyboard navigation, source-backed edits, audition targets and paths, processor state, meters, Parameter provenance, Graph inspection, diagnostics, and History |
| Packaging | Crate contents, Debian archive layout, installed documentation and fixtures, executable mode, and checksum sidecar |

An invalid Candidate cannot replace the Active Revision, and rendering continues to use the exact Active snapshot while invalid source remains on disk.

## Offline workload records

[`docs/evidence/offline-workload-2026-08-21/`](evidence/offline-workload-2026-08-21/README.md) contains reports from an Arch Linux developer host for selected ranges of the workload fixture at 64, 128, and 256 frames. The reports retain measured pass/fail results, including a missed deadline rather than discarding it.

These records measure offline engine timing for the named executable, host, range, and quantum. They do not contain PipeWire xrun counts, realtime scheduling evidence, device behavior, callback allocation/blocking audits, or listening judgments.

## Analysis vectors

Finalized-file analysis tests cover silence, DC and clipping, non-finite data, truncated or corrupt files, inter-sample overload, the official EBU steady-tone loudness tolerance, and EBU true-peak steady-tone cases 15–19. The complete official EBU signal set and transient true-peak cases 20–23 have not been executed, so the documentation names the exact Crump method rather than claiming full EBU Mode or BS.1770-5 conformance. See [Finalized Audio Analysis](research/finalized-audio-analysis.md).

## Common commands

```bash
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
cargo package --locked
bash packaging/build-deb.sh dist
```
