# Native acoustic models

Crump's `exciter` and `modal_bank` Nodes generate impacts, collision gestures and resonant bodies directly. Instruments compose these with the existing filters, mixers, envelopes and modulation system. Numerical body data lives in Acoustic Profiles; playing controls and routing live in Instrument Modules.

The [instrument examples](../examples/instruments/README.md) include percussion, cymbals, pianos and wooden bodies built this way. The [minimal acoustic fixture](../fixtures/acoustic/project.crump) demonstrates a separately imported profile.

## Profiles and routing

A separate profile file contains one named body:

```crump
crump-profile 1
profile wooden-body {
  root 261.625565hz
  mode body frequency 1329hz decay 18ms gain 0.8 after_decay 40ms after_gain 0.08
}
```

Import the file before the root Project declaration, using a normalized Project-relative `.crump` path. Source-v2 Instrument Modules can also contain profiles after their Instrument declaration. `crump module export` includes every required profile, so the exported acoustic module is self-contained.

Profiles share one Project-wide namespace. When multiple imported sources (such as a standalone `crump-profile 1` file and one or more self-contained Instrument Modules) declare the same profile ID with semantically identical parsed bodies—comparing root frequency and ordered modes rather than comments, formatting, or mode labels—Crump deduplicates them deterministically into one profile, keeping the first declaration in import order. Declaring the same profile ID with differing bodies across sources (or repeating the same profile ID within one source file) fails validation with `CRV210`, naming both locations so one can be renamed.

Connect `note.gate` and `note.velocity` to an `exciter`. Connect the resulting mono Audio, `note.pitch` and `note.gate` to a `modal_bank`. Convert its mono output through `mono_to_stereo` before the graph Output.

| Node | Required inputs | Settings |
| --- | --- | --- |
| `exciter` | `gate`, `velocity` | `kind impact\|collisions\|texture`, `contact`, `hardness`, `rate`, `duration`, `attack`, `level` |
| `modal_bank` | mono `audio`, `pitch`, `gate` | `profile`, `max_modes`, `key_track`, `decay_scale`, `brightness`, `rise`, `level` |

Physical controls latch on a rising gate. Level remains continuous. A changed latched control affects the next strike. Mono note retargeting starts a new contact and body response.

## Excitation

`impact` produces a finite, normalized contact pulse. Hardness blends squared and fourth-power sine shapes; velocity changes force and contact duration.

`collisions` distributes contacts through a seeded statistical gesture with sixteen fixed contact slots. Its optional attack creates a gradual onset and suppresses the forced initial contact. Zero attack starts immediately.

`texture` produces seeded, centered uniform random force with the envelope `(1 − exp(−t/attack))² × exp(−t/duration)`, normalized to its analytical peak. Zero attack gives immediate exponential decay. Velocity scales force linearly. The gesture lasts at most eight times `attack + duration`, including its initially silent sample. Contact, hardness and collision rate do not affect this kind.

| Setting | Domain | Default |
| --- | --- | --- |
| Contact | 20 μs–20 ms | 0.1 ms |
| Hardness | 0–1 | 0.5 |
| Collision rate | 1–10,000 Hz | 400 Hz |
| Gesture decay (`duration`) | 5 ms–2 s | 0.1 s |
| Attack | 0–1 s | 0 |
| Level | 0–4 | 1 |

## Resonant bodies

Each mode is a stable second-order resonator with a prompt decay and optional aftersound. Decay values are amplitude e-folding times, rather than T60. Frequencies follow the profile root and key tracking. Modes outside 18 Hz to 0.43 of the sample rate are omitted. Brightness tilts modal amplitudes; decay scale changes their damping.

Optional `rise` multiplies the body output by `(1 − exp(−t/rise))²` after a trigger. This shapes how the body emerges without delaying excitation or changing resonance frequencies. It is not peak-normalized, so increasing rise can reduce level. Zero rise leaves the modal response unchanged. This is an authored envelope, not a structural propagation simulation.

Profiles contain 1–256 modes. Root frequency spans 8–20,000 Hz; mode frequencies span 18–22,000 Hz. Decays span 0.1 ms–12 s, and component gains span 0–4 with a positive total no greater than 64. Every mode declares both decay components; use `after_gain 0` when no aftersound is needed.

| Setting | Domain | Default |
| --- | --- | --- |
| Maximum modes | 1–256; must cover the profile | 64 |
| Key tracking | 0–1 | 1 |
| Decay scale | 0.1–4 | 1 |
| Brightness exponent | −2–2 | 0 |
| Rise | 0–1 s | 0 |
| Level | 0–4 | 1 |

Positive rise values below 1 μs are treated as 1 μs. Profiles contain numerical frequencies, gains and decay constants; they do not contain recorded waveforms, phases or spectral frames.

## Lifecycle and resource limits

Profiles participate in source snapshots, revision identity and History. Invalid bodies, missing dependencies and insufficient mode ceilings fail validation. Changing a profile restarts the affected Instrument state. Admission accounts for immutable profiles, prepared voices, trigger work and publication transitions; a valid mode count does not guarantee admission at every polyphony.

Note Off allows the body to ring. Acoustic graphs and graphs containing one-shot sample sources have a 30-second maximum release, with a terminal 50 ms fade and earlier retirement once quiet. Voice stealing, arrangement duration and render tails can still end a sound. Gated graph release limits are unchanged.

`src/acoustics.rs` owns the shared DSP; graph adapters handle controls and lifecycle. `src/dsp.rs` contains the shared state-variable filter. Callback processing allocates no memory. These models approximate linear resonances and statistical contacts; they do not simulate nonlinear cymbal coupling or a coupled hammer/string/soundboard system.

## Offline analysis

`cargo run --release --example acoustic_probe` accepts a JSON Profile and excitation request on stdin and emits 48 kHz mono little-endian float32 samples on stdout. The adapter uses the same acoustic DSP and float32 boundaries as the graph. Optional high-pass and low-pass stages use the shared filter. See the [probe source](../examples/acoustic_probe.rs) for the request fields and limits. [`filter_impact_probe`](../examples/filter_impact_probe.rs) exposes contact-driven filter responses in the same format.

The [audio comparison tool](../tools/audio_compare/README.md) compares arbitrary reference and candidate WAVs using onset envelopes, spectra, spectrograms and band decay. These measurements help locate differences; listening determines whether a difference matters musically.
