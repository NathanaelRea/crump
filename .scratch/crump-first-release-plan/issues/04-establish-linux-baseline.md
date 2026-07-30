# Establish the Supported Linux Audio and MIDI Baseline

Type: research
Mode: AFK
Status: resolved
Blocked by: none

## Question

Which Linux distributions, audio APIs or servers, MIDI APIs, device assumptions, packaging constraints, and real-time privileges provide a supportable first-release baseline, and what tradeoffs follow from that choice?

## Answer

Adopt one narrow, reproducible first-release reference configuration:

- Fully updated Ubuntu Desktop 26.04 LTS on `amd64`, running a local systemd user session. Canonical maintains the LTS through May 2031; Crump's own update and support duration remains for [Define the Public-Release Support Contract](21-define-support-contract.md).
- Native PipeWire stereo playback and PipeWire MIDI/control ports backed by WirePlumber's ALSA Sequencer bridge. ALSA is the device substrate; JACK is optional interoperability, not a required daemon or supported backend contract.
- One named wired, in-tree-ALSA-supported USB stereo interface and one named ALSA-Sequencer-visible USB MIDI 1.0 controller for acceptance. Exact devices, rates, quanta, latency, and reliability limits remain for [Establish First-Release Performance and Reliability Budgets](20-establish-performance-budgets.md) and the support contract.
- An unconfined Ubuntu `amd64` deb using distribution libraries and services. Other distributions, architectures, classic JACK, direct ALSA, sandboxed packages, Bluetooth, aggregate devices, and multichannel output are outside the first supported configuration rather than hidden fallback paths.
- Stock PipeWire and RTKit real-time setup without root, a custom kernel, capabilities, `audio`-group membership, or custom PAM limits. Crump must report actual real-time status; non-RT playback may continue as explicitly degraded but cannot pass performance acceptance.

This accepts narrower reach and dependence on distribution session policy in exchange for a support matrix that can be installed, instrumented, and reproduced. Backend crates, internal sample format, resampling, processing blocks, and precise scheduler policy remain implementation choices subject to prototype evidence.

The [primary-source research report](../assets/04-linux-audio-midi-baseline-research.md) records the comparison, source citations, and required validation. [ADR 0002](../../../docs/adr/0002-native-pipewire-linux-baseline.md) fixes the consequential platform and backend seam.
