# Crump

Crump is a Linux-first, terminal-first **Guided Production Instrument** for electronic and synthesized music. Familiar production views lead human work, a small declarative language remains the durable Project source, and external Authoring Agents use the same readable files and a versioned `crump --json` command surface.

The First Public Release is deliberately narrower than a general DAW: it makes, auditions, revises, restores, and renders a 30–60 second no-sample Musical Sketch. Native graph instruments, synthesized percussion, exact musical time, persistent history, deterministic float-WAV rendering, PipeWire playback/MIDI, and the Ratatui Production Workspace are implemented in Rust.

> **Release status:** implementation preview. Automated validation passes, but the public-release hardware endurance run and independent Beginner Musician / Experienced Producer evaluations are still required. See [Release evidence](docs/release-evidence.md). Do not interpret an unverified machine as part of the supported performance baseline.

## What works

- Human-readable, lossless `.crump` Project and Instrument Module source
- Source-located parse, semantic, graph, and resource diagnostics
- Timeline Tracks, reusable Patterns, procedural Pattern Variations, Clips, named Locators, and deterministic Groove Templates
- Polyphonic composable DSP graphs, expressive native devices, stock drums, and portable WAV-backed sampler Instruments
- Source-backed point Automation and Pattern Parameter locks for topology-stable physical-unit targets
- Sample-accurate effective-value evaluation shared by playback and rendering, with base/Automation/lock/Modulation provenance
- Keyboard-complete Timeline, Event Grid, Faceplate, Parameters, Automation, Graph, Source, Diagnostics, and History views
- Native stereo 48 kHz PipeWire playback and transient MIDI 1.0/UMP Note Audition
- Checked source-preserving edits with stale-revision protection
- Explicit Candidate → Active publication and append-only exact Project History
- Portable WAV declaration/import/collection, exact content-addressed History ownership, deterministic off-callback decode/resampling, and sampler playback
- Native Track/Group/Return/Master routing with Sends, sidechains, latency compensation, and a bounded processor floor
- Checked Clip, Locator, arrangement-time, Groove, Variation, and Parameter-lock edits without procedural flattening
- Safe Active publication through aligned 10 ms transitions: changed-target scalar handoff or equal-power topology fade
- Deterministic stereo IEEE 32-bit-float Master/Stem bundles with hashes and finalized-file peak, loudness, DC, clipping, and non-finite analysis
- Versioned exact-range offline workload reports that keep PipeWire, callback-safety, and named-hardware evidence explicitly external
- Stable one-response JSON commands for external Authoring Agents

The implemented engineering baseline remains **Production Control Foundation 1A**; this is not completion of the production roadmap or a newly certified release. Later roadmap engineering now includes routed LFO Modulation, the compiled mix graph and native processor floor, Slice 4 sampler playback, Slice 5 expressive-device contracts, Slice 6 linear arrangement/groove workflows, and Slice 7 finalized delivery plus offline workload characterization. Plug-ins, recording, Media Tracks and time stretching, generated Rust DSP, realtime complete-track qualification, named-hardware evidence, listening gates, and every Slice acceptance gate remain open.

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
- `1`–`9` select production views, `0` opens Help; `Tab` and `Shift-Tab` move between all views.
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
# Optional offline range/quantum workload evidence (not a PipeWire test):
crump --project neon-steps qualify --quantum 128 --plan
crump --project neon-steps qualify --quantum 128 \
  --output evidence/qualification.json
# Optional aligned channel delivery:
crump --project neon-steps render --output render/stems \
  --stem track/lead:post_fader \
  --stem group/music:post_fader
```

The render bundle always contains analyzed `master.wav` and `manifest.json`; each requested Track/Group tap adds an aligned, independently analyzed and hashed Stem WAV. The complete artifact set is published atomically.

A separate long-form engineering fixture lives in `fixtures/production/`. It is a self-contained 96-bar (3:12) portable-sampler/native-instrument Project used for admission and deterministic grouping tests; it is not independent listening or named-hardware qualification evidence.

## Safe source workflow

`project.crump`, imported `.crump` modules, and explicitly declared Project-owned media are authoritative. Crump never makes an invalid Candidate audible:

1. Source and declared media files on disk form the **Candidate Revision**.
2. `validate` checks the complete Candidate and emits source locations.
3. `publish` atomically records a valid Candidate as the **Active Revision**.
4. Playback and rendering consume exact Active snapshots, not unchecked disk state.
5. Every structured edit and restore creates an append-only **History Entry**.

This lets a musician keep playing the last valid state while a person, editor, or Agent repairs invalid source.

## Commands

```text
new  validate  inspect  status  publish  check
workspace  play  render  qualify  doctor
history list|show|diff|restore
edit parameter|track-level|track-pan|note-pitch|event-time|note-duration|event-velocity|hit-voice
edit automation-create|automation-delete|automation-point-create|automation-point-time|automation-point-value|automation-point-curve|automation-point-delete
edit clip-*|locator-*|time-*|groove-*|pattern-variation-*|parameter-lock-*
edit sample-place|sample-move|sample-duplicate|sample-split|sample-trim|sample-slip|sample-fade|sample-gain|sample-pan
asset import|collect|list
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
