# Instrument player

A browser page for playing every [example instrument](../../examples/instruments/README.md) from a computer keyboard, the mouse or a MIDI controller.

```bash
cargo build --release --locked --bin crump
uv run tools/instrument_assets/build.py --setup
uv run tools/instrument_player/build.py --serve
```

Open the printed address, `http://127.0.0.1:8765/tools/instrument_player/` by default. Later sessions can skip the build with `--serve-only`. The server binds to localhost and serves the repository read-only.

## What you hear

Multisample instruments play their exact example media. The page reads zones, sustain loops, round-robin groups, Level, Touch and the Attack/Release envelope from each Instrument Module. Loops use the engine's linear crossfade; a released looped note keeps looping under its release fade rather than playing the sample's own tail. It mirrors the engine's gain law, `Level × ((1 − Touch) + Touch × velocity)`. One-shot banks ignore Note Off, as in Crump.

Native acoustic instruments have no media, so the build renders them through Crump. It plays isolated notes every half octave at two velocities, then slices the result into a preview bank. Between rendered velocities, level scales linearly.

The browser repitches with its own resampler, not the engine's interpolator. The Room control is a player-only convolution hall, not part of any Project. The audition phrase button plays the notes from each example's `project.crump`.

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

`build.py` writes `renders/instrument-player/manifest.json` and the acoustic preview WAVs, replacing earlier player output. It requires the release `crump` binary (`--crump PATH` overrides it) and NumPy/SciPy, which `uv run` installs from the script header.
