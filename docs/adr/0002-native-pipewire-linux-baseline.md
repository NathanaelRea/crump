# ADR 0002: Native PipeWire Linux Baseline

Status: Accepted
Date: 2026-07-23

## Context

PipeWire, JACK, and direct ALSA expose different timing, routing, device, and failure contracts. Crump uses one explicit Linux playback boundary rather than hiding several backends behind unequal behavior.

## Decision

- The documented playback environment is fully updated Ubuntu Desktop 26.04 LTS on `amd64` in a local systemd user session.
- Crump uses the distribution PipeWire, WirePlumber, and RTKit stack for stereo float playback and transient MIDI Note Audition. ALSA is the underlying device and Sequencer substrate.
- JACK interoperability is incidental. Crump has no separately supported JACK daemon, direct ALSA PCM backend, or direct ALSA Sequencer backend.
- Playback uses a wired, in-tree-ALSA-supported stereo output device. MIDI uses a class-compliant USB MIDI 1.0 controller visible to PipeWire.
- Packaging is an unconfined Ubuntu `amd64` native binary or Debian archive using distribution libraries and services.
- Crump uses normal unprivileged PipeWire and RTKit policy. It does not require root, a custom kernel, Linux capabilities, `audio`-group membership, or custom PAM limits.
- The engine reports scheduling and stream failures instead of silently switching to another backend.

## Consequences

Audio and MIDI share PipeWire's graph clock and negotiated processing model. Crump depends on WirePlumber policy and exposes server, device, quantum, xrun, and scheduling diagnostics. A broken PipeWire user session blocks playback; direct ALSA is not a hidden fallback.

Other distributions and architectures, classic JACK, direct ALSA, sandboxed packages, Bluetooth, aggregate devices, multichannel output, and unsupported hardware are outside the documented playback environment.

## Rejected alternatives

- Ubuntu 24.04 LTS, Debian stable, and Fedora Workstation are not documented playback environments.
- JACK/JACK2 as the primary API would add another server lifecycle or compatibility package.
- Direct ALSA would make Crump own device exclusivity, hotplug, conversion, routing, and cross-clock MIDI alignment.
- Flatpak, Snap, AppImage, and static portable bundles are unsupported because their device and host-integration behavior differs from the native package.

## Evidence

- [Canonical Ubuntu lifecycle](https://ubuntu.com/about/release-cycle)
- [PipeWire stream contract](https://docs.pipewire.org/page_streams.html)
- [PipeWire MIDI design](https://docs.pipewire.org/page_midi.html)
- [PipeWire real-time module](https://docs.pipewire.org/page_module_rt.html)
