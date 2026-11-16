# Workspace latency investigation — 2026-08-25

## Scope

This investigation measured the release-mode Production Workspace on the repository's `productions/obsidian-veil` Project. It separates Cargo compilation, exact Project reconstruction, filesystem invalidation probes, Ratatui draw CPU cost, PTY output submission, and physical display presentation. The measurements do not include compositor or panel scanout latency.

Host:

- AMD Ryzen 5 7640U, 6 cores / 12 threads, boost ceiling 4.972 GHz
- 32 KiB L1 data and 1 MiB L2 per core; 16 MiB shared L3
- 30 GiB RAM
- Rust 1.96.1, release profile with thin LTO and one codegen unit
- PTY size 140 × 32, `TERM=xterm-256color`

The Project owns 20,801,924 logical bytes across 12 declared media files. Its complete directory is much larger because authoring candidates and prior renders are not Candidate inputs.

## Root cause

The severe tail latency was not keyboard dispatch or Ratatui itself. The UI thread performed a complete Candidate and History reconstruction every 400 ms, even when no relevant file changed. On this Project a refresh occupied the thread for 736–824 ms, so input arriving during that work waited behind it.

The same loop also requested a draw every 40 ms while idle. Additional avoidable work included exact History asset verification during list/state inspection, full WAV decoding for generated Instrument previews, recompiling control state in renderers, rebuilding derived views, formatting off-screen table rows, and destroying replaced runtime state on the terminal thread.

## Final architecture

- A persistent capacity-one builder serializes filesystem probes, exact Candidate/History reconstruction, stability verification, durable Project mutations, and Active runtime preparation. Latest-intent flags coalesce bursts instead of creating threads or an unbounded work queue.
- Startup first renders a loading workspace. Exact Candidate and History state are prepared in the background and swapped only after complete reconstruction and a second background filesystem-generation verification.
- `WorkspaceWatch` tracks file identity, length, modification time, and change time for authoritative source/media and History paths. It includes unresolved declared imports and both symlink objects and followed targets. Metadata is only an invalidation signal; exact bytes, parsing, validation, hashing, and History verification remain authoritative.
- Periodic and explicit reload requests perform no filesystem I/O on the terminal thread. A detected change triggers exact background reconstruction, and changes observed during loading or before final verification cause a coalesced retry.
- Candidate and History invalidation remain independent. Generated Instrument sections, Timeline projections, Mixer metadata, processing inspection, and reusable targeted control inspectors are prepared by revision before adoption.
- Active runtime compilation, priming, boxing, and publication preparation occur on the builder. Candidate, committed Active, queued publication, and audible state are separate: Active inspection becomes live only when callback telemetry reports the matching publication generation.
- Replaced Projects, derived caches, players, and reclaimed `SongRuntime` boxes move through a bounded nonblocking reclaimer. The audio callback transfers only preallocated pointers through its bounded ring; it performs no allocation, locks, filesystem/console I/O, parsing, instrumentation, UI work, or destruction.
- Background panics are contained. Operation-kind tracking clears or retries the correct latch, while a mutation panic forces Candidate/History reconciliation. Interactive shutdown waits for any durable mutation to finish.
- The event loop draws only after a state change, playback/diagnostics animation, or accepted background result. Idle terminal output is zero.
- `F12` and `CRUMP_WORKSPACE_DIAGNOSTICS=1` expose bounded p50/p95/p99 phase counters. An ignored release test reports fixed-size per-view timing and allocations.

## Release results

The final profile used the same Project, host, release profile, 140 × 32 `TestBackend`, and 50 draws per view. `Workspace::load` is the synchronous test helper; interactive startup uses the background path described below.

| Phase | Original | Final | Result |
| --- | ---: | ---: | ---: |
| Synchronous test-helper load | 904.608 ms | 293.677 ms | 3.08× faster |
| Unchanged periodic path | 756.748 ms UI stall | 4.296 µs mean UI queue; 61.546 µs mean background completion | no UI filesystem stall |
| Forced exact reload | 756.748 ms UI stall | 11.632 µs UI queue; 66.196 ms background completion | no UI reconstruction stall |
| Timeline draw | 177.508 µs | 189.994 µs | comparable/noise |
| Mixer draw | 3,205.309 µs | 163.673 µs | 19.58× faster |
| Instrument draw | 502.396 µs | 104.504 µs | 4.81× faster |
| Automation draw | 540.667 µs | 149.887 µs | 3.61× faster |

The five unchanged probes had UI queue costs of 10.269, 3.226, 2.776, 2.555, and 2.655 µs. Their queue-to-background-completion costs were 129.814, 47.028, 43.903, 44.233, and 42.750 µs. These include scheduler and test polling effects; none performs Project reconstruction.

Viewport bounding reduced Mixer work from 6,908 allocations / 534,871 bytes per draw to 1,158 allocations / 91,529 bytes per draw. The final targeted Instrument inspector draws in 104.504 µs with 193 allocations / 33,285 bytes per draw; it reuses evaluator storage and pre-indexes the selected Instrument's routes instead of materializing every Project control target.

## PTY interaction results

The final release PTY probe reported:

- loading-shell first output: **2.492 ms**
- exact Candidate/History loaded and final startup output complete: **359.230 ms**
- idle output over 3 seconds: **0 bytes**
- view-switch output complete: **0.223–0.870 ms**
- 40 Timeline selection changes: mean **0.620 ms**, p50 **0.607 ms**, p95 **0.913 ms**, p99/max **1.085 ms**
- selection sent 1 ms after explicit `r`: **0.496 ms** output completion

The explicit-reload regression budget was 20 ms; the measured queued selection used 2.5% of that budget. Exact reconstruction still took tens of milliseconds in the background, but it no longer delayed the following input.

The original baseline had a 690.322 ms selection sample and a 468.659 ms Mixer switch when input overlapped periodic reconstruction. It emitted 24,435 idle bytes over 30 seconds. Before the background control plane, a selection queued behind `r` measured 111.853 ms, and a later reproduction reached 309 ms. These sample sets are small, so maxima are reproduction evidence rather than population percentiles.

The PTY timer ends at the final observed terminal write. It includes UI-thread queueing but not terminal-emulator parsing, compositor scheduling, panel scanout, or a claim that pixels were visible.

## Hardware and physical lower bounds

“Speed of light” is not a useful optimization target for this path. Electromagnetic propagation across one metre is only a few nanoseconds, while a keyboard event requires kernel input handling, event decoding, layout/formatting, Ratatui buffer diffing, PTY writes, terminal-emulator parsing, compositor scheduling, and display scanout.

Useful order-of-magnitude comparisons are:

- At the CPU's 4.972 GHz boost ceiling, one cycle is about 0.201 ns. The 0.620 ms selection mean is roughly 3.08 million ceiling-frequency cycles. Modern cache/DRAM access costs range from roughly nanoseconds to tens of nanoseconds; no single cache miss explains the complete software path.
- A conservative 20–50 GB/s streaming-memory bound moves the Candidate's 20.8 MB in about 0.42–1.04 ms. The 66.196 ms exact reload is 64–158× that raw-copy floor because it also performs filesystem operations, SHA-256 identity checks, WAV inspection, parsing, semantic validation, History work, and derived-state preparation. Crucially, its UI submission cost is 11.632 µs.
- Moving the Mixer's 91,529 allocated bytes has a 1.8–4.6 µs raw-copy floor at the same bandwidth. Its 163.673 µs draw remains dominated by formatting, allocation bookkeeping, layout, and buffer diffing rather than byte movement.
- A selection emitted about 1,771 PTY bytes. Raw memory movement for that payload is below 0.1 µs, while application output completion averaged 0.620 ms; terminal protocol and software scheduling set the relevant transport floor.
- A 60 Hz display scans a frame every 16.7 ms and a 120 Hz display every 8.3 ms. With application submission around or below 1.1 ms in this sample, terminal/compositor scheduling and scanout are likely to dominate perceived latency.

These are conservative scale comparisons, not cycle-accurate cache measurements. CPU frequency, page-cache warmth, allocator state, terminal emulator, compositor, and display refresh all affect end-to-end results.

## Reproduction

```bash
CRUMP_WORKSPACE_PROFILE_PROJECT=productions/obsidian-veil \
  cargo test --release workspace::tests::profile_workspace_hot_paths \
  -- --ignored --nocapture
```

For an interactive run, press `F12` or start with:

```bash
CRUMP_WORKSPACE_DIAGNOSTICS=1 \
  cargo run --release -- --project productions/obsidian-veil workspace
```

The exit report contains lifetime counts and bounded recent p50/p95/p99 samples for probes, loads, preparation, mutation, adoption, drawing, and event-read-to-draw. Event-read-to-draw starts when Crossterm reports readiness, immediately before event decoding, and ends when Crump submits the terminal draw. It excludes earlier terminal-queue delay and physical presentation.

## Residual boundary

The 400 ms invalidation cadence remains a background metadata probe rather than an OS event source such as inotify. This deliberately keeps the correctness model portable and simple; it affects external-change discovery latency, not input latency. Exact startup and reload still require real I/O and validation, so the final state appears hundreds or tens of milliseconds after the loading shell or request. Renderers still allocate formatted Ratatui content, but every measured production-scale view is below 0.2 ms of draw CPU in this profile. Future work should be driven by a larger admitted Project or terminal profile rather than replacing exact identity checks or moving work into the realtime callback.
