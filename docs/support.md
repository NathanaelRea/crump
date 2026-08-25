# Supported Environment

Crump playback uses native PipeWire on Linux. Parser, validation, History, workspace inspection, and offline rendering can be built without PipeWire, but that build has playback stubs and is not a music-playback configuration.

## Playback environment

| Component | Supported configuration |
| --- | --- |
| OS | Fully updated Ubuntu Desktop 26.04 LTS |
| Architecture | `amd64` / Rust `x86_64` |
| Session | Local systemd user session |
| Audio service | Distribution PipeWire 1.6 + WirePlumber + RTKit |
| Kernel/device layer | In-tree ALSA drivers |
| Project rate | 48 kHz |
| Output | One wired ALSA-supported stereo device through PipeWire |
| MIDI | Class-compliant USB MIDI 1.0 keyboard visible to PipeWire |
| Packaging | Unconfined native binary or `.deb` |

Crump negotiates stereo float audio at 48 kHz and accepts the server-selected quantum. Bluetooth, HDMI-only, network, aggregate, and multichannel devices are unsupported. JACK compatibility is incidental; Crump uses PipeWire's native API. Containers, SSH-only sessions, WSL, macOS, Windows, ARM, and headless servers are unsupported playback environments.

## Source build dependencies

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install build-essential dpkg-dev pkg-config libpipewire-0.3-dev libclang-dev
```

Install Rust 1.88 or newer with rustup, then build:

```bash
cargo build --release --locked
```

The default `pipewire` Cargo feature enables playback and MIDI. `--no-default-features` retains Project loading, validation, History, workspace inspection, and offline rendering with playback stubs.

To construct and install the Ubuntu `amd64` package:

```bash
packaging/build-deb.sh dist
dpkg-deb --info dist/crump_0.1.0_amd64.deb
sudo apt install ./dist/crump_0.1.0_amd64.deb
crump --version
```

The builder uses `dpkg-deb` on Ubuntu and a deterministic GNU `ar`/GNU `tar` fallback on other developer hosts. The package installs the binary, user guides, Reference Project, starter Instrument library, deep-house integration Project, and native-synthesis qualification fixtures. Generated renders and `.crump` working History are intentionally excluded.

## Diagnostics

```bash
crump --project ./song doctor
crump --project ./song validate
crump --project ./song check
wpctl status
pw-top
```

`doctor` reports OS and architecture classification, compiled PipeWire support, `pw-cli` availability/version, and Project revision/diagnostics when the Project loads. Its machine-readable `ready` field is true only for the exact Ubuntu 26.04 `x86_64` identity with compiled native support, an available PipeWire CLI, and no invalid loaded Project.

`doctor.ready` does not report a live library connection, RTKit scheduling, negotiated quantum, sink quality, MIDI wiring, PipeWire xruns, device endurance, accessibility, or listening quality.

## PipeWire configuration

WirePlumber routes the `crump` Music playback stream to the default sink. Use desktop sound settings, `wpctl`, `pw-link`, or a PipeWire graph tool to change the sink or link MIDI to `crump-midi-input`. Crump does not store machine-specific PipeWire object IDs in the portable Project.

Use distribution-managed RTKit policy. Crump does not require root, broad `rtprio` overrides, or disabled desktop security. `pw-top` reports the actual graph rate, quantum, and xruns; Project source does not.

## Project bounds

Validation and compilation enforce these bounds before runtime publication:

- one finite linear Song at 48 kHz;
- at most 64 authored Nodes per graph voice and 256 graph voices across Track instances;
- at most 128 Automation lanes, 1,024 points per lane, and 8,192 points total;
- at most 256 Modulation routes, 2,048 route operators, and 512 callback-time Parameter targets;
- at most 256 declared WAV assets, 64 MiB per asset, and 256 MiB of declared WAV bytes per Project; and
- at most 30 seconds of explicit Render Tail.

Additional schedule, routing, processor, voice, work, memory, latency, and generated-audio limits are validated and exposed through diagnostics or inspection. The limits bound work; they are not recommended project sizes for every machine.

## Unsupported behavior

Crump does not provide:

- Media Tracks, recording, comping, live input, or destructive waveform editing;
- proprietary warp modes, formant preservation, or realtime routed pitch;
- VST, CLAP, LV2, or other plug-in hosting;
- user Rust/native extensions or arbitrary scripting;
- recorded Automation, MPE, MIDI clock/transport, controller mapping, SysEx authoring, or full MIDI interchange;
- byte-identical rendering across builds or architectures;
- a universal loudness target or hidden normalization; or
- low-latency guarantees for undocumented devices and hosts.

## Diagnostic bundle

When reporting a playback or support problem, capture only relevant, non-secret facts:

```bash
crump --version
crump --project ./song doctor --json
crump --project ./song status --json
crump --project ./song validate --json
crump --project ./song qualify --quantum 128 --plan --json
uname -a
rustc --version
pw-cli --version
wpctl status
pw-top -b -n 10
systemctl --user status pipewire wireplumber
```

Also record the OS/update date, CPU and memory, audio/MIDI make/model and USB IDs, sink/profile/rate/quantum, RTKit scheduling, reproduction steps, executable SHA-256, Locator range and quantum used by `qualify`, and relevant Crump/PipeWire counters.

A `qualify` report times only the offline engine and leaves `pipewire_xruns` as `null`. PipeWire xruns and device behavior must be observed through the live graph. Use `--require-pass` when automation must fail on an offline threshold miss; the report is still written before the nonzero exit.

For native-synthesis qualification and escalation, follow the exact [Milestone 8 runbook](qualification/native-synthesis.md) and attach an [automated run record](evidence/native-synthesis-m8/automated-run-template.json). Producer credibility requires an independently completed [producer record](evidence/native-synthesis-m8/producer-template.md). Realtime support claims require a completed [named hardware/PipeWire endurance record](evidence/native-synthesis-m8/hardware-endurance-template.md); offline reports and blank templates do not substitute for either.

Do not publish usernames, home paths, unrelated PipeWire node names, environment variables, Project source, or Agent prompts unless needed and approved.

## Troubleshooting

### Build cannot find PipeWire

Confirm `pkg-config --modversion libpipewire-0.3` succeeds and install `libpipewire-0.3-dev`. Bindgen also needs Clang development libraries.

### Workspace will not start

The workspace requires stdin and stdout attached to an interactive terminal. `workspace --json`, redirected output, and non-terminal automation are rejected. Use normal CLI commands for scripts.

### Playback fails but render works

Inspect user services, sink routing, rate/profile, RTKit, and PipeWire logs. Native setup and first-callback waits are bounded to 10 seconds, and a stopped or disconnected stream is reported instead of hanging. Do not rewrite Project source to fix host routing.

### Candidate is invalid but old music still plays

This is expected. Open Diagnostics, run `validate`, repair source, or restore History. The prior Active Revision remains audible until valid source is explicitly published.

### MIDI is not visible

Verify the controller appears in `wpctl status` or a PipeWire graph tool and link it to `crump-midi-input`. ALSA presence alone does not guarantee that session policy exposed it as a PipeWire control stream.

### Render comparison differs

Compare manifest Project revision, Crump version, executable hash, architecture, sample rate, frame count, settings, and selected Stem owner/tap set before comparing file hashes and analysis.
