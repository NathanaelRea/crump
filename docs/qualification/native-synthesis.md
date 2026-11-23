# Native synthesis qualification runbook

This runbook collects Milestone 8 engineering evidence without treating templates or offline timing as producer, PipeWire, or hardware results.

## Automated repository checks

From a clean checkout, using the locked dependency graph:

```bash
python3 fixtures/synth-alpha/generate_max_fixture.py --check
cargo test --locked --test native_synthesis_qualification --all-features
cargo test --locked 'realtime::native::tests::maximum_topology_' --all-features -- --test-threads=1
cargo test --locked realtime::native::tests::granular_spectral_independent_maxima_callback_is_allocation_free --all-features -- --exact --test-threads=1
sudo tests/qualification/clean-account-portability.sh target/debug/crump
cargo test --all-targets --all-features --locked
cargo test --all-targets --no-default-features --features tui --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
```

The maximum topology fixture is an admitted combined profile, not the combination of every independent ceiling. It sustains an eight-note simultaneous chord for one 4/4 bar (96,000 frames/two seconds at the fixture's 48 kHz and 120 BPM) against seven resident graph voices, exercising deterministic stealing and hundreds to thousands of full callbacks per supported quantum before the disclosed render tail. Raising it to eight voices is a checked rejected boundary because two-runtime transition work exceeds the callback-work budget. The separate readable `fixtures/synth-alpha/maximum-granular-spectral.crump` fixture reaches both independent per-voice ceilings (two 64-slot granular sources and two 128-peak spectral sources) at the largest admitted polyphony of five; the contract test rejects polyphony six and binds its actual steady-state, publication-transition, and memory estimates.

`qualify` measures the offline engine only. Use `--require-pass` for a nonzero exit when its report fails:

```bash
crump --project PROJECT qualify --quantum 128 \
  --maximum-p999-utilization 0.80 --require-pass \
  --output evidence/offline-128.json
```

Do not use shared CI wall-clock timing as named-hardware evidence.

## Portability and current recovery matrix

`tests/qualification/clean-account-portability.sh` publishes the exact-asset maximum Project, copies its source, media, Active state, and complete History to a distinct empty account, then validates, reopens, inspects, prepares playback, exports/inspects/renders an offline Module audition, edits, renders, restores, and byte-compares the original Master and manifest. CI runs it as a separate OS user; changing only `HOME` is not the clean-account gate.

The locked Rust suites currently cover these recovery boundaries:

| Boundary | Current automated evidence | Scope boundary |
| --- | --- | --- |
| media and wavetable import/morph publication | `edit::tests::wavetable_import_writer_failure_rolls_back_source_asset_and_history` removes write permission from the source root after initializing writable History/media directories, forcing the real import transaction to fail after asset staging; it then reopens the unchanged Project and History | modeled source-writer failure, not arbitrary filesystem or kernel faults |
| source/media transaction interruption | `history::tests::complete_source_transaction_rolls_back_a_failed_writer`, `history::tests::startup_rolls_back_uncommitted_source_and_asset_bytes`, `history::tests::startup_completes_committed_source_and_asset_bytes` | modeled source/media journal states |
| disposable derivative rebuild interruption | `maximum_profile_is_admitted_deterministic_grouping_independent_and_rebuildable` compares rebuilt sample, wavetable, and spectral identities/audio; `interrupted_disposable_derivative_rebuild_leaves_published_runtime_usable` discards a complete candidate before publication, proves the existing runtime is unchanged, and rebuilds byte-identical audio | derivatives have no durable cache-publication boundary: they remain candidate-owned memory until whole-runtime publication |
| History and restore interruption | `history::tests::startup_can_roll_back_to_an_invalid_candidate_source`, `history::tests::startup_rollback_preserves_invalid_media_bytes_touched_by_restore`, `history::tests::startup_rolls_back_an_uncommitted_source_transaction`, `history::tests::startup_completes_a_committed_source_transaction` | modeled restore journal states, including partial source/media writes |
| render bundle interruption | `engine::render::tests::injected_post_spool_failure_preserves_destination_and_cleans_staging` | modeled post-spool publication boundary |

Every modeled durable interruption above reopens as the complete old or complete new state; disposable derivatives rebuild from exact source/assets after their candidate is discarded. Nothing here claims arbitrary filesystem or kernel fault coverage.

## Crump-owned callback call-path review

The reviewed Crump-owned PipeWire process path is `run_pipewire`'s process listener → `CallbackState::process_buffer` → bounded ring/atomic command handling → `SongRuntime::process_frame`/`process_transition_frame` → precompiled control, Track, graph, shared-graph, processor, compensation, meter, and telemetry state. Runtime publication arrives as an already compiled boxed runtime; retirement uses the fixed-capacity reclamation ring or one callback-owned pending slot. MIDI, command, telemetry, voice, Node, route, processor, and graph loops all terminate at compile-time or admitted fixed capacities.

Within that source path, review finds no parser, graph compiler, filesystem API, console/logging output, `Mutex`/`RwLock`, condition variable, channel wait, sleep, or dynamically growing callback container. The separate allocation tests exercise steady processing/control, transition/deferred reclamation, and the independent granular/spectral ceiling. This is a review of Crump-owned code, not proof about libpipewire, libc/libm, the kernel, drivers, or device scheduling; those remain within the named-hardware gate.

## Independent producer protocol

Before work begins, record the evaluator's name or stable pseudonym, relevant production experience, Crump build hash, lawful reference material, and evaluation date. The evaluator must independently:

1. create or substantially revise patches in virtual-analog, wavetable, sample/multisample, granular, and spectral modes;
2. use generated views for ordinary sound design and source for a topology edit;
3. intentionally use envelopes, LFOs, note velocity/key, macros, audio-rate synthesis, and routed Modulation;
4. use those sounds in a complete track;
5. perform documented level-matched comparisons of core categories;
6. save, close, reopen, transfer, audition, automate, render, and deliver the Project; and
7. provide explicit pass/fail judgments and reasons for integrated credibility, starter content, and the sound-compatibility boundary.

Record observations in `docs/evidence/native-synthesis-m8/producer-template.md`. Repository authors must not fill this on a producer's behalf.

## Named Linux/PipeWire/device 30-minute protocol

Freeze these identities before the run: distribution and update date, kernel, CPU, memory, audio interface and USB/PCI IDs, output device, PipeWire/WirePlumber versions, sink profile/rate/quantum, RTKit status, Crump version and executable SHA-256, Project revision, and exact Locator range.

1. Reboot or document host uptime and unrelated workload.
2. Confirm the intended PipeWire sink/profile and fixed 48 kHz graph.
3. Open the worst accepted complete-track section. Exercise maximum admitted voices, unison, Modulation, table scanning, granular/spectral work, shared effects, and live publication.
4. Reset or record initial Crump and PipeWire counters.
5. Run continuously for at least 30 minutes on the named device. Do not substitute offline rendering.
6. Record start/end monotonic and wall times, PipeWire xruns, late callbacks, processing failures, non-finite samples, p99.9 and peak callback utilization, publications attempted/completed, MIDI drop/panic counters, and interruptions.
7. Preserve raw `pw-top`/support output and the completed hardware template. Any xrun, late callback, processing failure, non-finite sample, shorter duration, unidentified device, or missed published utilization margin is a failed result, not omitted data.

The callback allocation tests separately prove zero allocations/deallocations for (1) warmed maximum-topology steady-state/control handling, (2) publication transition plus deferred reclamation, and (3) the independent granular/spectral ceiling fixture in the tested build. Absence of direct lock, blocking, filesystem, and console calls within the reviewed Crump-owned functions remains call-path evidence. No syscall-absence claim is made for `Instant::now()`, libpipewire, libc/libm, the kernel, drivers, or devices; the hardware run supplies actual PipeWire/device behavior.
