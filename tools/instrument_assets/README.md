# Instrument library tools

Every instrument in the [example library](../../examples/instruments/README.md) is a native graph written in its `instrument.crump`, so there is no media to generate. These tools check the library and compare it against recordings.

## Reference recordings

[`references.json`](references.json) lists freely licensed recordings to compare the instruments against: isolated notes from the [University of Iowa MIS](https://theremin.music.uiowa.edu/MIS.html), the [Philharmonia](https://philharmonia.co.uk/resources/sound-samples/) sample library and [VSCO-2 CE](https://github.com/sgossner/VSCO-2-CE) where they exist, otherwise [Wikimedia Commons](https://commons.wikimedia.org/) or YouTube recordings with their own licences. They are listening and analysis aids only, downloaded locally and never committed or distributed. Fetching needs ffmpeg, and yt-dlp for YouTube clips. Comparing needs the release binary:

```bash
cargo build --release --locked --bin crump
uv run tools/instrument_assets/references.py --fetch
uv run tools/instrument_assets/references.py --compare
```

`--fetch` downloads each source once, slices the clip in `references.json` and writes it to `renders/instrument-references` as 48 kHz mono. `--compare` detects each clip's pitch, renders the module at that note through Crump and records sixth-octave long-term spectra, 20 dB decay and spectral centroid for both in `report.json`, which the [instrument player](../instrument_player/README.md) shows beside each instrument. A clip's `hz` overrides the detected pitch; a `stroke` picks which stroke of a split instrument it compares against, using the keys in its catalog entry. `--profile ID` prints the harmonic spectral envelope measured from an instrument's pitched clips, as `[[hz, dB], ...]` points to fit a body to.

## Verification

Build the CLI, then validate and render the distributed audition Projects:

```bash
cargo build --release --locked --bin crump
python3 tools/instrument_assets/verify.py --render --workers 2
python3 -m unittest discover -s tools/instrument_assets -p 'test_*.py'
```

The verifier publishes each example through the normal History seam, checks the exact Active Revision, plans its render and rejects silent, clipped or non-finite output. Reports and renders go to `renders/instrument-verification`. Use a fresh `--output` directory for another render pass; existing renders are not replaced. Omit `--render` for validation and publication only.

`crump check` also evaluates a separate 30–60-second Reference Musical Sketch contract. These shorter instrument auditions intentionally do not satisfy that contract; verification records the result while requiring full Project validation. The tests additionally check catalog completeness, global ID uniqueness, that no module depends on media, and importing several modules into a fresh Project with the copy helper.
