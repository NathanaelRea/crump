<img src="assets/branding/icon.png" alt="Crump icon" width="96" height="96">

# Crump

Crump is a Linux-first, terminal Guided Production Instrument for electronic and synthesized music. Familiar production views lead human work, a small declarative language remains the durable Project source, and external Authoring Agents use the same readable files and versioned `crump --json` command surface.

## Capabilities

- Human-readable, lossless `.crump` Project and Instrument Module source
- Source-located parse, semantic, graph, route, media, and resource diagnostics
- DAW-style fit/zoom Timeline Tracks with Clip ranges, canonical Event activity, Locator/playhead overlays, and verified sample-occurrence source waveforms
- Reusable Patterns, Pattern Variations, Clips, Locators, and Groove Templates
- Polyphonic Composable DSP Graphs, stock drums, expressive virtual-analog, wavetable, sample, multisample, granular, and deterministic spectral devices, and portable WAV-backed sampler Instruments
- Source-backed point Automation, Pattern Parameter Locks, routed LFO Modulation, and an Envelope Follower
- Sample-accurate control evaluation shared by playback and rendering, with value provenance
- Track, Group, Return, Send, sidechain, and Master routing with latency compensation
- Native EQ, dynamics, Delay, Reverb, Saturation, Chorus/Flanger, Crusher, Stutter, and Limiter processing
- Keyboard operation across Timeline, Event Grid, Faceplate, Parameters, Automation, Processing, Graph, Source, Diagnostics, and History views
- Native stereo 48 kHz PipeWire playback and transient MIDI Note Audition
- Checked source-preserving edits with stale-revision protection
- Explicit Candidate → Active publication and append-only exact Project History
- Portable WAV and exact mono wavetable import/collection, off-callback sample decode/resampling, deterministic mip and spectral-peak compilation, graph sample/multisample/granular/spectral and sampler playback, bounce, and exact-frame sample preparation
- Safe runtime publication through compatible state handoff or an aligned topology fade
- Deterministic stereo float Master/Stem bundles with hashes and finalized-file audio analysis
- Versioned exact-range offline workload reports
- Stable one-response JSON commands for external Authoring Agents

Crump does not support recording, Media Tracks, third-party plug-ins, arbitrary native extensions, formant preservation, realtime routed pitch, MPE, or non-Linux playback. See [Supported Environment](docs/support.md) for the complete platform and capability limits.

## Install from source

Playback documentation targets Ubuntu Desktop 26.04 LTS `amd64` with PipeWire 1.6, WirePlumber, and Rust 1.88 or newer. On Ubuntu/Debian, install the native build dependencies:

```bash
sudo apt install build-essential dpkg-dev pkg-config libpipewire-0.3-dev libclang-dev
```

Install Rust with [rustup](https://rustup.rs/), then build or install Crump:

```bash
git clone https://github.com/NathanaelRea/crump.git
cd crump
cargo install --path . --locked
crump doctor
```

To build the unconfined Ubuntu `amd64` package, run `packaging/build-deb.sh dist`. It includes the user guides, Reference Project, starter Instrument library, deep-house integration Project, and native-synthesis qualification fixtures as well as the binary. A build without native PipeWire is available for parser, history, workspace inspection, and render tooling only:

```bash
cargo build --release --no-default-features
```

## Start a Project

```bash
crump new my-sketch --name "My Sketch"
cd my-sketch
crump
```

Inside the workspace:

- `Space` starts, pauses, or resumes the exact Active Revision; `S` stops.
- `1`–`9` and `0` select Timeline through Source; Tab reaches Diagnostics, History, and Help.
- `Tab` / `Shift-Tab` or `gt` / `gT` move between views.
- `j` / `k` or arrow keys navigate; Timeline `h` / `l` seeks on an exact musical grid.
- Timeline `V` selects a half-open range, `L` loops it, `/` searches, and `:` jumps to bars, beats, Locators, or relative musical durations.
- Press `i` in an Instrument-related view for explicit Note Audition mode; `Esc` returns to Normal mode.
- `Enter` accepts an exact value; `P` publishes a valid Candidate.
- `?` shows keyboard help. `Ctrl-C` is the only workspace and interactive-playback exit shortcut; `q` and `Esc` never exit.

The starter Project is silent. From the repository root, copy [`fixtures/reference/`](fixtures/reference/) to exercise a complete synthesized arrangement:

```bash
cp -R fixtures/reference neon-steps
crump --project neon-steps publish \
  --origin human --actor "$USER" \
  --summary "Adopt reference sketch" \
  --explanation "Initial inspected publication"
crump --project neon-steps check
crump --project neon-steps play
crump --project neon-steps render --output render/master --plan
crump --project neon-steps render --output render/master
crump --project neon-steps qualify --quantum 128 --plan
crump --project neon-steps qualify --quantum 128 \
  --output evidence/qualification.json
crump --project neon-steps render --output render/stems \
  --stem track/lead:post_fader
```

The render bundle contains analyzed `master.wav` and `manifest.json`; each requested Track or Group tap adds an aligned, independently analyzed and hashed Stem WAV. The complete artifact set is published atomically.

[`fixtures/production/`](fixtures/production/) is a self-contained 96-bar Project used by resource, routing, sampler, and deterministic grouping tests.

Both Projects keep each Instrument in `instruments/<id>.crump`, imported by `project.crump`. Tweak its sound, Parameters, and Faceplate in that file; the main Project assigns it to Tracks and owns Patterns, Clip placement, Automation, and mixing. Copy the whole Project directory so the imports travel with it.

[`fixtures/starter/`](fixtures/starter/) contains nine source-readable native-synthesis Instrument Modules, exact lawful assets, and authored Audition Phrases. [`fixtures/deep-house-reference/`](fixtures/deep-house-reference/) is an original portable 112-bar (3:40) integration fixture using every runnable source family with Patterns, Automation, Modulation, routing, effects, stems, and deterministic delivery evidence. These fixtures are engineering/content preparation; independent listening and named-hardware acceptance remain pending under the [native-synthesis qualification runbook](docs/qualification/native-synthesis.md).

[`examples/instruments/`](examples/instruments/README.md) contains 92 copyable instruments with audition Projects: 60 house instruments and a 32-piece orchestra, built from native acoustic graphs and procedural multisample banks. The [instrument player](tools/instrument_player/README.md) plays all of them in a browser. The [acoustic model reference](docs/native-acoustics.md) explains excitation and resonant-body profiles. The [audio comparison tool](tools/audio_compare/README.md) builds standalone listening and time/frequency reports from WAV files.

## Safe source workflow

`project.crump`, imported `.crump` modules, and declared Project-owned media are authoritative. Crump never makes an invalid Candidate audible:

1. Source and declared media files on disk form the **Candidate Revision**.
2. `validate` checks the complete Candidate and emits source locations.
3. `publish` atomically records a valid Candidate as the **Active Revision**.
4. Playback and rendering consume exact Active snapshots, not unchecked disk state.
5. Every structured edit and restore creates an append-only **History Entry**.

This lets playback continue from the last valid state while a person, editor, or Agent repairs invalid source.

## Commands

```text
new  validate  inspect  status  publish  check
workspace  play  render  qualify  doctor
history list|show|diff|restore
edit parameter|track-level|track-pan|note-pitch|event-time|note-duration|event-velocity|hit-voice
edit automation-*|clip-*|locator-*|time-*|groove-*|pattern-variation-*|parameter-lock-*
edit sample-place|sample-move|sample-duplicate|sample-split|sample-trim|sample-slip|sample-fade|sample-gain|sample-pan
asset import|import-wavetable|collect|bounce|prepare|list
module export|install|list|inspect|preview|copy|collect
```

Every non-interactive command supports `--json` and emits exactly one response with `schema_version`, `ok`, `command`, and either `data` or `error`. The interactive workspace intentionally rejects `--json`.

## Documentation

- [Documentation index](docs/README.md)
- [Production Workspace](docs/workspace.md)
- [Project Language](docs/language.md)
- [CLI and Authoring Agents](docs/cli-and-agents.md)
- [Production Cookbook](docs/cookbook/README.md)
- [Project Agent Skills](.agents/skills/README.md)
- [Audio, revision transitions, and MIDI](docs/audio-and-midi.md)
- [History and deterministic rendering](docs/history-and-rendering.md)
- [Supported environment and troubleshooting](docs/support.md)
- [Verification](docs/verification.md)
- [Product contract](requirements/product-contract.md)
- [Domain glossary](CONTEXT.md)
- [Architecture overview](docs/architecture/overview.md)

## License

MIT. See [LICENSE](LICENSE).
