<img src="assets/branding/icon.png" alt="Crump icon" width="96" height="96">

# Crump

Crump is a text-first DAW built for coding agents. A song is a folder of plain `.crump` files: an agent (or you, in any editor) writes the music, and Crump plays it, checks it, and keeps every version. When you want to fine-tune, open the window and turn a knob, nudge a note or drag a fader; each change is written straight back to the text.

```text
song {
  tempo 112bpm
  pattern bass_a 1bar {
    note 0beat E2 3/4beat 0.8
    note 2beat B1 3/4beat 0.76
  }
}
```

![Arrangement view of the deep-house reference project](assets/screenshots/gui-timeline.png)

- **Agents first.** Every command has a `--json` mode, errors point to file, line and column, and edits refuse to apply to stale files. A broken edit never replaces the last version that played.
- **Quick hands-on changes.** The window has the synth, FX rack, mixer, piano roll and automation, with normal DAW shortcuts. Mouse edits keep your formatting and comments.
- **History you can see.** Every edit is saved. Scrub through versions to see the knobs, effects and notes as they were, play any version, and restore it.
- **Everything built in.** Analog, wavetable, sample, granular and spectral synths, drums, effects, sends and sidechains. No plug-ins, and the offline render sounds exactly like playback.

Crump runs on Linux with PipeWire. It has no audio recording or third-party plug-ins ([limits](docs/support.md)).

## Try it

```bash
sudo apt install build-essential dpkg-dev pkg-config libpipewire-0.3-dev libclang-dev
git clone https://github.com/NathanaelRea/crump.git && cd crump
cargo run --release -- --project fixtures/deep-house-reference
```

Use `--release`; a debug build is too slow for realtime audio. To install, run `cargo install --path . --locked`, then `crump new my-sketch` and `crump` inside it.

An agent works through the CLI:

```console
$ crump --json status
{"command":"status","data":{"active_entry":"000000000001","candidate_valid":true,"state":"active",...},"ok":true,"schema_version":1}
$ crump validate
instruments/dh-bass.crump:5:3: error[CRV024]: authored Parameter value is outside its legal domain
$ crump publish --summary "Darker bass" && crump render --output render/master
```

## More

- [Examples](examples/instruments/README.md): 184 instruments to copy, plus the [reference projects](fixtures/)
- [Workspace and shortcuts](docs/workspace.md) · [Project language](docs/language.md) · [CLI and agents](docs/cli-and-agents.md)
- [History and rendering](docs/history-and-rendering.md) · [Cookbook](docs/cookbook/README.md) · [All docs](docs/README.md)

Crump is developed in the open but does not accept external pull requests. MIT licensed; see [LICENSE](LICENSE).
