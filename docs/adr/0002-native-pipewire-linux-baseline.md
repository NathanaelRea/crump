# ADR 0002: Native PipeWire Linux Baseline

Status: Accepted
Date: 2026-07-23

## Context

Crump's first public release needs one documented Linux configuration on which its native workflow, real-time safety, transient MIDI Note Audition, and performance claims can be reproduced. Promising generic Linux support would leave distribution policy, audio-server behavior, device access, real-time privileges, and packaging too variable for a credible release gate.

The backend choice also fixes an architectural seam. A native PipeWire client, a JACK client, and direct ALSA access expose different timing, routing, device, and failure contracts to the engine.

## Decision

- Use fully updated Ubuntu Desktop 26.04 LTS on `amd64`, in a local systemd user session, as the sole first-release reference configuration. The later public-release support contract will define Crump's update and support duration within Canonical's lifecycle.
- Use the distribution PipeWire, WirePlumber, and RTKit stack. Crump is a native PipeWire client for stereo playback and clocked MIDI note-on/off input used only for first-release Note Audition; ALSA is the underlying device and Sequencer substrate.
- Treat JACK as optional interoperability, not a required daemon or minimum-install dependency. Do not provide a separately supported direct ALSA PCM or Sequencer backend in the first-release baseline.
- Require one named wired, in-tree-ALSA-supported stereo output interface and one named ALSA-Sequencer-visible USB MIDI 1.0 controller for release acceptance. Exact hardware is selected with the performance and support contracts.
- Ship an unconfined Ubuntu `amd64` deb package and rely on distribution libraries and services. Sandboxed formats and other distributions or architectures are not part of the first supported configuration.
- Use the stock unprivileged PipeWire and RTKit path; do not require root, a custom kernel, capabilities, `audio`-group membership, or custom PAM limits. Detect actual real-time scheduling and expose failures. Non-real-time playback may continue in an explicit degraded mode but cannot satisfy performance acceptance.
- Keep implementation adapters, crates, internal sample format, resampling, block subdivision, and exact scheduling policy open until prototypes and benchmarks establish them.

## Consequences

- Release evidence can target one reproducible OS, server model, package format, and hardware class instead of an unbounded Linux matrix.
- Audio and MIDI share PipeWire's graph clock and negotiated processing model, while Crump depends on WirePlumber policy and must diagnose server, device, quantum, xrun, and real-time state.
- Users on other distributions, architectures, classic JACK, direct ALSA, sandboxed packages, Bluetooth, aggregate devices, or unsupported hardware receive no first-release compatibility promise.
- A broken PipeWire user session blocks supported real-time playback; direct ALSA is not a hidden fallback.
- The performance decision must validate rates, quanta, latency, xruns, device failure, MIDI timing, and RTKit behavior on named hardware. The support decision must define update policy, installation, diagnostics, and the duration of the promise.

## Alternatives Considered

- Ubuntu 24.04 LTS: rejected as the release gate because it has a shorter remaining standard-maintenance window and an older PipeWire stack, though it may be tested best-effort.
- Debian stable or Fedora Workstation: rejected as additional first-release gates because each adds distribution policy, packaging, and test-matrix work without proving another product path.
- JACK or JACK2 as the primary API: rejected because it adds a compatibility package or separate server lifecycle to a native workflow that PipeWire can provide directly.
- Direct ALSA PCM and Sequencer access: rejected because Crump would own device exclusivity, hotplug, conversion, routing, and cross-clock MIDI alignment already handled by the supported PipeWire stack.
- Flatpak, Snap, AppImage, or static portable bundles: deferred because confinement, device access, real-time behavior, and host integration require separate qualification.

## Evidence

- [Resolving issue](../../.scratch/crump-first-release-plan/issues/04-establish-linux-baseline.md)
- [Primary-source Linux baseline research](../../.scratch/crump-first-release-plan/assets/04-linux-audio-midi-baseline-research.md)
- [Canonical Ubuntu lifecycle](https://ubuntu.com/about/release-cycle)
- [PipeWire stream contract](https://docs.pipewire.org/page_streams.html)
- [PipeWire MIDI design](https://docs.pipewire.org/page_midi.html)
- [PipeWire real-time module](https://docs.pipewire.org/page_module_rt.html)
