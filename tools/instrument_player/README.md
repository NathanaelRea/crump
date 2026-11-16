# Instrument player

A browser page for playing every [example instrument](../../examples/instruments/README.md) from a computer keyboard, the mouse or a MIDI controller.

```bash
cargo build --release --locked --bin crump
uv run tools/instrument_player/build.py --serve
```

Open the printed address, `http://127.0.0.1:8765/tools/instrument_player/` by default. Later sessions can skip the build with `--serve-only`. The server binds to localhost and serves the repository read-only.

## What you hear

Every instrument is a native graph with no media, so the build renders it through Crump. It plays isolated notes every half octave at two velocities, then slices the result into a preview bank. Unpitched instruments play two takes of each stroke; a split instrument such as the cajón has one stroke per key range. Synths and sustained acoustic instruments (winds, brass, bowed strings, voices and organs) are rendered as held notes: they loop part of the hold and fade with their Release on Note Off, and the mono basses play without glide. Struck and plucked instruments ring out. Between rendered velocities, level scales linearly.

The browser repitches with its own resampler, not the engine's interpolator. The Room control is a player-only convolution hall, not part of any Project. The audition phrase button plays the notes from each example's `project.crump`.

## New instruments and references

The **New** list holds the 92 expansion instruments, each with a line on what it should sound like. If you have fetched and compared the [reference recordings](../instrument_assets/README.md#reference-recordings), instruments with a `ref` tag show them under the title. **▶ Ref** plays a recording, **▶ Model** plays this instrument at the note detected in it, and **A/B** plays one after the other. The chart overlays the two long-term spectra, each relative to its loudest band; below are the 20 dB decay and spectral centroid of both. Recordings carry their room and microphone, so treat the numbers as hints and trust your ears. Rate instruments and add notes as before, then **Copy feedback**.

## Controls

| Input | Action |
| --- | --- |
| `A`–`;`, `W` `E` `T` `Y` `U` `O` `P` | Play one octave and a third from the current C |
| `Z` / `X` | Octave down / up |
| `C` / `V` | Velocity down / up |
| `Space` | Sustain pedal |
| `[` / `]` | Previous / next instrument |
| `Enter` | Audition phrase |
| `Esc` | Stop all sound |

Click lower on a key to play harder. Shift-click instruments in the list to layer them. MIDI needs a browser with Web MIDI and the **Enable MIDI** button; note velocity and CC64 sustain are supported.

👍/👎 and notes are stored in the browser. **Copy feedback** collects them as Markdown.

## Output

`build.py` writes `renders/instrument-player/manifest.json` and the native preview WAVs, replacing earlier player output. It includes `renders/instrument-references/report.json` when present. It requires the release `crump` binary (`--crump PATH` overrides it) and NumPy/SciPy, which `uv run` installs from the script header.
