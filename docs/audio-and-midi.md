# Native Audio, Revision Transitions, and MIDI

Crump's supported first-release audio path is native PipeWire on Linux. Playback and offline rendering share the same compiled schedule, Event ordering, seeded voices, graph compiler, Node implementations, and Track balance. Ratatui is not involved in DSP.

## Audio format and routing

- Graph and Project rate: 48,000 frames/second
- Stream format: interleaved stereo `F32LE`
- PipeWire role/category: Music / Playback
- Node name: `crump`
- Project routing: Instrument → Track level/pan → stereo Master
- Monitoring-only safety: finite-value sanitizer and soft emergency ceiling above 0.9

The safety stage is not authored sound and is never used for offline output. Offline processing instead fails immediately with the exact frame when a non-finite sample appears.

PipeWire/WirePlumber selects and routes the default stereo sink. Use the normal desktop sound settings, `wpctl`, `pw-link`, or a graph tool such as qpwgraph to change the sink. Bluetooth, network audio, multichannel output, capture, aggregate devices, and JACK-only operation are not part of the first-release support contract.

## Starting playback

```bash
crump --project ./song play
crump --project ./song play --seconds 10
```

`play` compiles the exact Active Revision. In a terminal, `Ctrl-C`, `q`, or Escape stops early. Native setup and first-callback waits are each bounded to 10 seconds; a backend error, premature stop/disconnect, or callback timeout exits as an operational failure instead of waiting indefinitely. The JSON result reports Active/Candidate identity, elapsed time, peak, callback count, late-callback count, processing errors, transition state, voice count, MIDI state, and dropped MIDI count.

The Production Workspace uses the same adapter and can publish new Active runtimes while playing.

## Real-time callback contract

Before connecting a stream, Crump parses, validates, compiles schedules/topology, allocates all graph voices and delay/reverb buffers, and constructs the complete runtime. The PipeWire process callback performs only:

- bounded pops from fixed-capacity single-producer/single-consumer rings;
- fixed Track/voice/Node DSP work;
- writes into PipeWire's mapped output buffer;
- bounded old/new transition mixing;
- atomic status reads/writes; and
- bounded telemetry pushes.

It does not parse source, perform filesystem or console I/O, construct graphs, acquire a mutex, wait on another thread, or intentionally allocate. Retired runtime destruction is handed back through a fixed queue and occurs on the controller thread. If that queue is temporarily full, one retired runtime remains owned and retried by the callback rather than being destroyed there.

A callback is counted late when measured wall time exceeds the duration represented by its current buffer. This is useful live evidence but is not equivalent to PipeWire's server xrun counter; a release endurance run records both Crump telemetry and `pw-top`.

## Active Revision transitions

Candidate bytes never enter playback directly. On a successful publication while the workspace is playing:

1. the controller loads the exact committed Active snapshot;
2. validates and compiles it away from the process callback;
3. initializes it at the controller's latest transport frame;
4. sends one boxed, fully allocated runtime through a bounded queue; and
5. realigns it to the callback's exact current frame and crossfades old and new runtimes for 10 ms with equal-power gains.

The transition begins at a PipeWire block boundary. Transport does not restart. Semantically unchanged Tracks are processed once and retain their running voice/effect state across completion; changed schedules, Instruments, and effects restart at the aligned frame behind the protective transition, as accepted by [ADR 0011](adr/0011-first-release-graph-transition.md).

If another publication arrives during a transition, it waits in bounded control state. If publication or reclamation cannot proceed, the old audible runtime continues. Invalid Candidates never create a runtime.

## MIDI Note Audition

Crump creates a second PipeWire input stream named `crump-midi-input` with SPA `application/control` format. It accepts:

- normal MIDI 1.0 channel-voice Note On and Note Off;
- zero-velocity Note On as Note Off;
- All Sound Off (CC 120) and All Notes Off (CC 123); and
- MIDI 1.0 channel voice carried in 32-bit UMP packets.

Velocity is normalized from `0…127`. Channel and pitch form transient note identity. Note On is routed to the Track associated with the currently selected Instrument. Note Off remembers the Track chosen at Note On, so changing selection while holding a key cannot strand the old voice.

For graph Instruments, MIDI pitch drives the per-note oscillator pitch and Note Off releases envelopes. Stock drums use the standard first-release mapping:

| MIDI note | Voice |
| ---: | --- |
| 35, 36 | kick |
| 37, 38, 40 | snare |
| 39 | clap |
| 42, 44, 46 | hat |

Other channel messages, SysEx, clock, transport, pitch bend, pressure, CC Parameter mapping, MPE, and native MIDI 2.0 semantics are ignored in this release.

## Connection and stuck-note recovery

WirePlumber may auto-connect an available MIDI source. If it does not, connect the controller output to `crump-midi-input` with the desktop graph tool or `pw-link`. The workspace header shows `MIDI ●` only while the stream is actively receiving through the PipeWire graph.

Crump releases every graph voice when:

- the MIDI stream transitions from connected to disconnected;
- CC 120 or 123 is received;
- a MIDI queue overflow occurs;
- the user requests all-notes-off; or
- playback shuts down.

The MIDI producer queue is fixed-capacity. If any event is dropped, an atomic panic flag causes the audio callback to perform all-notes-off at the next block even when the queue was too full to enqueue the release itself. Dropped-event and consumed-event counters remain visible.

Note Audition is transient. It never modifies Pattern Events, Parameter source, Candidate/Active identity, or Project History.

## Computer keyboard audition

The workspace offers the same transient seam without hardware. Graph Instruments use `A W S E D F T G Y H U J K` for C4–C5. Stock drums use `A/S/D/F` or `Z/X/C/V` for kick/snare/hat/clap. Key-release events send Note Off, and a short timeout handles terminals that do not report release.

## Troubleshooting

Check the host first:

```bash
crump doctor
wpctl status
pw-top
systemctl --user status pipewire wireplumber
```

Common failures:

- **`audio_unavailable` / connection failure:** PipeWire user services are absent or inaccessible.
- **Stream stays paused:** no usable sink is routed, or session policy has not linked it. Non-interactive `play` reports a no-callback timeout after 10 seconds; the workspace remains available so routing can be repaired and playback restarted.
- **MIDI indicator is empty:** controller is not represented/routed as a PipeWire control source.
- **Late callbacks increase:** increase the PipeWire quantum, close competing real-time workloads, verify RTKit, and capture `pw-top` rather than assuming a Crump DSP fault.
- **Stream stopped/disconnected:** `play` exits with an operational failure; the workspace displays a restart prompt. Repair routing/device state before pressing `Space` again.
- **Audio is silent:** verify an Active Revision exists, the arrangement contains audible Events, Track level is not near `-96 dB`, and the selected sink is not muted.

See [Support](support.md) for the narrow reference environment and evidence limits.
