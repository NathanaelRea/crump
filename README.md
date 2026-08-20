# Crump

Crump is a Linux-first, terminal-first **Guided Production Instrument** for electronic and synthesized music. Familiar production views lead human work, a small declarative language remains the durable Project source, and external Authoring Agents use the same readable files and a versioned `crump --json` command surface.

The First Public Release is deliberately narrower than a general DAW: it makes, auditions, revises, restores, and renders a 30–60 second no-sample Musical Sketch. Native graph instruments, synthesized percussion, exact musical time, persistent history, deterministic float-WAV rendering, PipeWire playback/MIDI, and the Ratatui Production Workspace are implemented in Rust.

> **Release status:** implementation preview. Automated validation passes, but the public-release hardware endurance run and independent Beginner Musician / Experienced Producer evaluations are still required. See [Release evidence](docs/release-evidence.md). Do not interpret an unverified machine as part of the supported performance baseline.

## What works

- Human-readable, lossless `.crump` Project and Instrument Module source
- Source-located parse, semantic, graph, and resource diagnostics
- Timeline Tracks, reusable Patterns, Clips, pitched Events, and stock drum hits
- Polyphonic composable DSP graphs with physical-unit Parameters and Faceplates
- Keyboard-complete Timeline, Event Grid, Faceplate, Parameters, Graph, Source, Diagnostics, and History views
- Native stereo 48 kHz PipeWire playback and transient MIDI 1.0/UMP Note Audition
- Checked source-preserving edits with stale-revision protection
- Explicit Candidate → Active publication and append-only exact Project History
- Safe Active graph replacement through an aligned 10 ms equal-power transition
- Deterministic stereo IEEE 32-bit-float WAV plus a hashed render manifest
- Stable one-response JSON commands for external Authoring Agents

Samples, plug-ins, recording, automation, advanced routing/mixing, generated Rust DSP, and complete-track scope are intentionally outside this release.

## Install from source

The reference build target is Ubuntu Desktop 26.04 LTS `amd64` with PipeWire 1.6, WirePlumber, and Rust 1.88 or newer. On Ubuntu/Debian, install the native build dependencies:

```bash
sudo apt install build-essential dpkg-dev pkg-config libpipewire-0.3-dev libclang-dev
```

Install Rust with [rustup](https://rustup.rs/), then build or install Crump:

```bash
git clone https://github.com/nathan/crump.git
cd crump
cargo install --path . --locked
crump doctor
```

To build the unconfined Ubuntu `amd64` package candidate, run `packaging/build-deb.sh dist`. It includes the user guides, Reference Project, and provider-neutral Authoring skill as well as the binary. See the [support contract](docs/support.md) for inspection and installation commands. A build without native PipeWire is available for parser, history, and render tooling only:

```bash
cargo build --release --no-default-features
```

## First sketch

```bash
crump new my-sketch --name "My Sketch"
cd my-sketch
crump                 # opens the Production Workspace in a terminal
```

Inside the workspace:

- `Space` starts/stops the exact Active Revision.
- `1`–`9` select views; `Tab` and `Shift-Tab` move between them.
- Arrow keys select and make bounded source-backed adjustments.
- `Enter` accepts an exact value; `P` publishes a valid Candidate.
- `?` shows complete keyboard help; `q` exits.

The starter Project is intentionally silent. For a complete 34-second sketch:

```bash
cp -r /path/to/crump/fixtures/reference neon-steps
crump --project neon-steps publish \
  --origin human --actor "$USER" \
  --summary "Adopt reference sketch" \
  --explanation "Initial inspected publication"
crump --project neon-steps check
crump --project neon-steps play
crump --project neon-steps render --output render/master --plan
crump --project neon-steps render --output render/master
```

The render bundle contains `master.wav` and `manifest.json` and is published atomically.

## Safe source workflow

`project.crump` and imported `.crump` modules are authoritative. Crump never makes an invalid Candidate audible:

1. Files on disk form the **Candidate Revision**.
2. `validate` checks the complete Candidate and emits source locations.
3. `publish` atomically records a valid Candidate as the **Active Revision**.
4. Playback and rendering consume exact Active snapshots, not unchecked disk state.
5. Every structured edit and restore creates an append-only **History Entry**.

This lets a musician keep playing the last valid state while a person, editor, or Agent repairs invalid source.

## Commands

```text
new  validate  inspect  status  publish  check
workspace  play  render  doctor
history list|show|diff|restore
edit parameter|track-level|track-pan|note-pitch|event-time|note-duration|event-velocity|hit-voice
module export|install|list|inspect
```

Every non-interactive command supports `--json` and emits exactly one response with `schema_version`, `ok`, `command`, and either `data` or `error`. The interactive workspace intentionally rejects `--json`.

## Documentation

- [Production Workspace](docs/workspace.md)
- [Project language](docs/language.md)
- [CLI and Authoring Agents](docs/cli-and-agents.md)
- [Provider-neutral Crump Authoring skill](skills/crump-authoring/SKILL.md)
- [Audio, revision transitions, and MIDI](docs/audio-and-midi.md)
- [History and deterministic rendering](docs/history-and-rendering.md)
- [Support contract and troubleshooting](docs/support.md)
- [Release evidence and remaining human gates](docs/release-evidence.md)
- [Product contract](requirements/product-contract.md)
- [First Public Release definition](docs/product/first-public-release.md)
- [Domain glossary](CONTEXT.md)
- [Architecture overview](docs/architecture/overview.md)

## License

Mozilla Public License 2.0. See [LICENSE](LICENSE).
