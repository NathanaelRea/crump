# First-Release Support Contract

Crump is intentionally honest and narrow about support. “Runs on Linux” is not a performance certification. The First Public Release reference contract is one updated desktop stack; other environments may work but are not part of its low-latency, deterministic-comparison, or response-time promise.

## Reference platform

| Component | First-release reference |
| --- | --- |
| OS | Fully updated Ubuntu Desktop 26.04 LTS |
| Architecture | `amd64` / Rust `x86_64` |
| Session | Local systemd user session |
| Audio service | Distribution PipeWire 1.6 + WirePlumber + RTKit |
| Kernel/device layer | In-tree ALSA drivers |
| Project rate | 48 kHz |
| Initial acceptance quantum | 256 frames (5.33 ms graph period) |
| Output | One wired ALSA-supported stereo device through PipeWire |
| MIDI | One class-compliant USB MIDI 1.0 keyboard visible to PipeWire |
| Packaging | Unconfined native binary / `.deb` candidate |

Bluetooth, HDMI-only, network, aggregate, and multichannel devices are outside the gate. JACK compatibility is incidental; Crump uses PipeWire's native API. Containers, SSH-only sessions, WSL, macOS, Windows, ARM, and headless servers are not first-release playback environments.

The exact wired audio interface, USB MIDI controller, host CPU, memory, kernel, firmware, and device IDs still need to be frozen in the public evidence record. Until that happens and the endurance run passes, this repository is an implementation preview rather than a certified First Public Release.

## Source build dependencies

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install build-essential dpkg-dev pkg-config libpipewire-0.3-dev libclang-dev
```

Install Rust 1.88 or newer with rustup. A release build is:

```bash
cargo build --release --locked
```

The default `pipewire` Cargo feature is required for supported playback and MIDI. `--no-default-features` builds the Project, history, validation, workspace inspection, and offline render surfaces with playback stubs; it is not a supported music-production build.

To construct the unconfined first-release package candidate:

```bash
packaging/build-deb.sh dist
dpkg-deb --info dist/crump_0.1.0_amd64.deb
sudo apt install ./dist/crump_0.1.0_amd64.deb
crump --version
```

The builder uses `dpkg-deb` on Ubuntu. A deterministic GNU `ar`/GNU `tar` fallback exists for archive construction on developer hosts. The package preserves the source documentation layout under `/usr/share/doc/crump/` (user guides are in `docs/`), installs the Reference Project under both `fixtures/reference/` and `examples/reference/`, and installs the provider-neutral Authoring skill under `/usr/share/crump/skills/crump-authoring/`. Only package installation, upgrade, and removal on the named Ubuntu reference environment can qualify a release artifact.

## Readiness check

```bash
crump --project ./song doctor
crump --project ./song validate
crump --project ./song check
wpctl status
pw-top
```

`doctor` reports the OS release/architecture classification, whether this binary was compiled with PipeWire support, `pw-cli` availability/version, and Project root/revision/diagnostics when a Project can be loaded (otherwise `project_error`). `ready` is true only for the exact Ubuntu 26.04 `x86_64` reference identity with compiled native support, an available CLI, and no invalid loaded Project. It does **not** prove a live library connection, RTKit scheduling, quantum, sink quality, MIDI wiring, xrun-free endurance, independent usability, or musical credibility.

## PipeWire configuration

Crump negotiates stereo float audio at 48 kHz and accepts the server-selected quantum. For the reference run, configure the PipeWire graph to 48 kHz / 256 frames through supported distribution mechanisms and confirm the actual values in `pw-top`; do not infer them from Project source.

WirePlumber normally routes the `crump` Music playback stream to the default sink. It may auto-connect MIDI to `crump-midi-input`; otherwise use the desktop patchbay or `pw-link`. Crump does not store machine-specific PipeWire object IDs in the portable Project.

RTKit should grant the normal distribution-managed real-time scheduling policy. Crump does not ask users to run as root, apply broad `rtprio` overrides, or disable desktop security. A run without working real-time scheduling is degraded and cannot qualify performance evidence.

## Supported Project envelope

The first release accepts only the validated Musical Sketch slice:

- one finite linear Song at 48 kHz;
- 30–60 seconds for acceptance (the parser can represent other finite spans);
- native graph Instruments and stock synthesized drums;
- finite Patterns, Events, Clips, topology-stable point Automation, routed LFO Modulation, and the certified Reference Project's Track-level stereo balance;
- at most 64 authored Nodes per graph voice;
- at most 256 graph voices across actual runtime Track instances;
- at most 128 Automation lanes, 1,024 points per lane, 8,192 points total, 256 Modulation routes, 2,048 route operators, and 512 callback-time Parameter targets; and
- at most 30 seconds of explicit Render Tail.

Unaccepted roadmap engineering can additionally declare/import/collect up to 256 Project-owned WAV assets (64 MiB each, 256 MiB total), preserve their exact bytes through History, decode/resample them off-callback, play them through bounded sampler Instruments, bounce exact routed ranges, and prepare exact-frame shift-only duration/pitch assets under ADR 0012. Later engineering also includes the compiled mix graph/processor floor, expressive native devices, and checked linear arrangement, Groove, Pattern Variation, and Parameter-lock workflows. None of this expands the certified First Public Release envelope without its separate evidence gates.

The limits protect bounded work; they are not a recommendation to use maximum voices/Nodes on reference hardware. The frozen Reference Project is [`fixtures/reference/project.crump`](../fixtures/reference/project.crump).

## Known exclusions

The First Public Release does not promise:

- certified sampler, imported-audio, or prepared-transform quality, or Media Tracks (these exist only as unaccepted roadmap engineering);
- recording, comping, proprietary warp modes, formant preservation, or realtime routed pitch;
- VST, CLAP, LV2, or other plug-in hosting;
- user Rust/native extensions or arbitrary scripting;
- recorded or procedural Automation, non-LFO routed Modulation sources, or allocation-changing Automation/Modulation targets;
- production-qualified advanced mixing, processor quality, stem export, or complete-track delivery (roadmap mix-graph engineering exists but is not release-certified);
- complete-track duration, mixing, mastering, or delivery formats;
- MIDI clock/transport, pitch bend, pressure, CC mapping, MPE, SysEx, or full MIDI 2.0;
- accessibility certification solely because the client is text-based;
- byte-identical rendering across builds or architectures; or
- low-latency guarantees on unrecorded hardware.

## Diagnostic bundle

When reporting a playback or support problem, capture without secrets:

```bash
crump --version
crump --project ./song doctor --json
crump --project ./song status --json
crump --project ./song validate --json
crump --project ./song qualify --quantum 128 --plan --json
# When an offline workload report is relevant:
crump --project ./song qualify --quantum 128 \
  --output evidence/qualification.json --json
uname -a
rustc --version
pw-cli --version
wpctl status
pw-top -b -n 10
systemctl --user status pipewire wireplumber
```

Also record:

- exact OS image and update date;
- CPU, memory, kernel, audio/MIDI make/model and USB IDs;
- selected sink/profile/rate/quantum;
- whether RTKit granted scheduling;
- reproduction steps and whether offline render is correct;
- Crump executable SHA-256 (also present in render manifests and qualification reports);
- the explicit Locator range and quantum used for any offline qualification report; and
- callback, late-callback, processing-error, MIDI-dropped, and independently observed PipeWire xrun counts.

A `qualify` report times only the offline engine and intentionally leaves `pipewire_xruns` as `null`. Archive it as workload evidence, but capture target-graph xruns and the 30-minute realtime run separately.

Do not publish usernames, home paths, unrelated PipeWire node names, environment variables, Project source, or Agent prompts unless needed and approved.

## Failure triage

### Build cannot find PipeWire

Confirm `pkg-config --modversion libpipewire-0.3` succeeds and install `libpipewire-0.3-dev`. Bindgen also needs Clang development libraries.

### Workspace will not start

It requires stdin and stdout attached to an interactive terminal. `workspace --json`, redirected output, and non-terminal automation are rejected intentionally. Use normal CLI commands for scripts.

### Playback fails but render works

The engine and Project are likely valid; inspect user services, sink routing, rate/profile, RTKit, and PipeWire logs. Native setup and first-callback waits are bounded to 10 seconds, and a stopped/disconnected stream is reported rather than allowed to hang. Do not rewrite source to fix a host-routing fault.

### Candidate is invalid but old music still plays

That is the safety contract. Open Diagnostics, run `validate`, repair source, or restore History. The old Active Revision remains audible until valid source is explicitly published.

### MIDI is not visible

Verify the controller appears in `wpctl status` or a PipeWire graph tool and link it to `crump-midi-input`. ALSA presence alone does not guarantee that session policy has exposed/routed it as the required PipeWire control stream.

### Render comparison differs

First compare manifest Project revision, Crump version, executable hash, architecture, sample rate, frame count, and settings. For stems, also compare the exact selected owner/tap set, processing policy, and alignment delay before comparing per-file hashes and analysis. A difference outside the documented comparison domain is not a determinism regression.

## Security updates and lifecycle

No public support lifetime is claimed until the First Public Release is tagged and its Ubuntu lifecycle policy is recorded. Users should apply normal distribution security updates; the acceptance environment records exact package/build identities so updates can trigger a repeat qualification rather than encouraging an insecure frozen desktop.
