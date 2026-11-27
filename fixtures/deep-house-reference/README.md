# Deep house reference

An original, self-contained deep house track at 122 BPM in 4/4 and A minor. Its 116-bar half-open arrangement lasts 228.197 seconds (3:48.197) before the explicit four-second render tail. Its last bars return to the intro's state, so it loops seamlessly on repeat. It is meant to sound like a usable melodic deep house track, not a feature checklist.

## How it was written

The style target is Anjunadeep-style melodic house. Every part was chosen in blind, loudness-matched listening rounds of short Crump renders, in this order:

1. the chords and bass riff, from six 16-bar loop sketches with different progressions, bass rhythms and grooves;
2. the top-line synth, from six patches playing the same line;
3. the top line itself, from six lines on that synth;
4. each section, from candidates for the intro, breakdown and second drop;
5. the kick, from five synthesized kicks shaped by measurements of the references' kicks.

The arrangement follows the measured shape of the references (see `docs/research/anjunadeep-measured.md`): one foreground part at a time, phrase-end breaks where the kick and bass leave and the music continues, and a breakdown that fades the groove out instead of cutting it.

## Music

The harmony is one chord per bar: Am(add9), Am(add9), Fmaj7, G6. The bass plays a climbing sixteenth riff on the roots, rising through the octave, minor third, fifth and fourth. The track's spine is the `ostinato`: a seven-step sixteenth figure on the octave saw lead. Its seven steps run against the four-beat bar, so the accent walks across it. It plays through every section. The intro and the last bars use only its first bar, on repeat (`ostinato-cell`), and its filter cutoff is the song's long arc. A swelling horn (`horn`) carries the long top line, E5 then D5 resolving to C5, in the breakdown and the second drop. Its notes overlap and crossfade, with no glide. The soft pluck (`sparkle`) keeps the ostinato going quietly in the breakdown. The drums are a four-on-the-floor kick, off-beat open hats, sixteenth hats with claps on two and four, and tribal toms in the theme and second drop. The toms play the kick sample higher. The kick ducks the bass through `bass-duck` and pumps the Music Group through `music-pump`.

Repetition drifts instead of copying: the `deep-swing` and `loose-hats` Grooves re-roll timing, velocity and inclusion on every loop pass.

| Locator | Bar | What happens |
| --- | ---: | --- |
| `intro` | 0 | The ostinato's first bar alone, on repeat over the pad, filtered dark; kick and hats from bar 8 with the full ostinato |
| `groove-in` | 16 | Bass riff, claps and open hats. Bars 30–31 are a break: the kick and bass leave, the hats continue 8 dB down, and the ostinato dips into more reverb |
| `theme` | 32 | The full loop with toms; the same break at bars 46–47 |
| `break` | 48 | The groove fades out over eight bars while the pad, the horn line and the soft pluck swell in; ten bars of that alone; then two bars of near-silence at bars 66–67 |
| `build` | 68 | The ostinato returns filtered and opens; the bass teases in two-bar blocks; quiet hats creep in under a riser; half a bar of air before the drop |
| `drop` | 76 | The full groove; a break at bars 90–91 under a short riser |
| `drop-b` | 92 | The groove with toms and the horn line on top |
| `turnaround` | 108 | Bass out after two bars, kick and hats after four; the last four bars are the intro's state |
| `end` | 116 | Render boundary |

## Source coverage

The arranged Tracks use these source families:

| Source family | Tracks |
| --- | --- |
| Virtual analog | `bass`, `pad`, `ostinato` (`dh-octave-lead`), `sparkle` (`dh-pluck`), `horn` (`dh-brass`), `transition` |
| Noise graph | `open-hat` (`dh-open-hat`) |
| Sampler | `kick-drums`, `toms` |
| Stock synthesis | `percussion` |

The Project also imports the starter library's wavetable (`dh-lead`, `dh-evolving-texture`), `sample_source` plus multisample (`dh-keys`), granular and spectral Instruments. They compile with the Project, but no Track plays them: they did not survive the listening rounds. `fixtures/starter/auditions.crump` still plays every one of them.

The section table above is authored metadata for verification; all musical events remain reusable Patterns and Clips.

The production includes:

- two Grooves;
- absolute Automation of Instrument parameters and a Send level;
- per-voice, Instrument and global modulation;
- two kick sidechains and two envelope followers;
- Music and Drums Groups;
- shared Room and Echo Returns.

## Sound design and effects

The pad, pluck, horn and bass come from a measured search loop, and the kick from measurements of the references' kicks (see `docs/research/anjunadeep-measured.md`).

- **Ostinato lead.** Four unevenly detuned saws, two more an octave up, and a quiet sine an octave and a fifth up. A decaying low-pass opens each note.
- **Pad.** Six barely detuned saws with `spread natural`, so held chords shimmer instead of pulsing, plus a soft triangle a fifth up. Each chord swells in over 1.9 s, with a short glide between chord voices, through an open, resonance-free filter that drifts with the bar.
- **Horn.** One clean saw whose filter swells open over 0.7 s and rests half open. It fades in over 0.6 s and has a 3.5 s tail, so overlapping notes crossfade.
- **Soft pluck.** Six unevenly spread triangles with a touch of tanh and a quiet sine a fifth above, through a gently key-tracked, lightly driven low-pass.
- **Bass.** A short, round triangle pluck. Its weight comes from Track saturation, and a 380 Hz low-pass keeps its harmonics out of the music's midrange.
- **Kick.** The body falls from 110 Hz to 49 Hz with a 10 ms time constant and is 20 dB down by about 80 ms. A soft 2–8 kHz click sits 26 dB under the body.
- **Open hat.** A high-passed noise burst.

Effects:

- tape saturation on the kick, bass and pad, and on the Drums and Music Groups, with a parallel compressor gluing the Drums Group
- a light crusher and chorus on the hats and claps, which are darkened by a low tone setting and an 8 kHz low-pass so they sit under the kick
- chorus on the ostinato, horn, pad and soft pluck
- tape dirt on the Echo Return, and a light tape stage plus a 20 Hz high-pass on the Master before the glue compressor and limiter

The dynamics are kept light so the mix breathes. The kick pump on the Music Group has a 12 ms attack, so pad and lead attacks pass before it ducks them. The Master glue runs at 1.4:1 with 3.5 dB makeup.

Song modulation moves the effects themselves:

- LFOs drift the echo feedback, breathe the pad saturation and slowly swell the reverb decay.
- Automation opens the ostinato and pad filters section by section and opens the ostinato's Room Send in every break.
- Envelope followers let the kick duck the Room Return and dip the bass cutoff.

## Media

`generate_assets.py` regenerates `media/kick.wav` deterministically with the Python standard library, synthesized to the measurements above. The other four assets are copied from `fixtures/synth-alpha/media`. Every asset is committed because a Project declares each one by exact SHA-256 and will not load without the matching bytes.

## Verification workflow

```bash
crump --project fixtures/deep-house-reference --json status
crump --project fixtures/deep-house-reference --json validate
crump --project fixtures/deep-house-reference --json inspect --at 56bar
crump --project fixtures/deep-house-reference --json publish \
  --origin agent --actor "$AGENT_NAME" \
  --summary "Add deep house reference" \
  --explanation "Integrated source-family reference track"
crump --project fixtures/deep-house-reference --json history list
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/ostinato:post_fader --plan
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/ostinato:post_fader
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final-repeat \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/ostinato:post_fader
```

`check` is intentionally not a completion gate for this fixture: that command enforces the separate 30–60 second Musical Sketch contract and rejects both this required 3–5 minute duration and declared media. `validate` and render compilation are the applicable objective gates.

Requested stems are aligned delivery taps but are not claimed to reconstruct the processed Master. Render manifests disclose independent hashes and analysis. Repeated render hashes prove determinism only when the executable/build, Active Revision, request, and deterministic domain are identical.

## Human evaluation still required

No machine check establishes groove quality or mix translation. `render-evidence.json` records loudness, peak, and determinism only. Listening qualification remains pending, and this fixture makes no commercial-equivalence claim.
