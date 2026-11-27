# Anjunadeep references: measured arrangement and sound

This note is informative and does not define Crump behavior. It complements [anjunadeep-style.md](anjunadeep-style.md) with measurements taken from five reference tracks. Every claim here is **[M] measured**: it comes from running the scripts below on the audio, not from interviews.

**Measured:** 2026-11-26
**References** (local copies only, never committed):

- Ben Böhmer, "Beyond Beliefs"
- Ezequiel Arias & Durante, "Dream Controller"
- Marsh, "Lailonie" (Extended Mix)
- Nox Vahn & Marsh, "Come Together"
- Romain Garcia, "Everybody Dreams"

## Method

1. Demucs (`htdemucs`) splits each track into drums, bass, vocals and other stems.
2. `composition.py` sets a bar grid from the drum stem's beat track at about 123 BPM, or 117.5 for "Come Together". Per bar, it takes each stem's 75th-percentile 16th-note RMS relative to the track's loud level.
3. A gap is a run of at least two 16ths where the full mix sits more than 18 dB below the loud level.
4. A dropout is a run where the drums stem is more than 30 dB down while the rest keeps playing.

The scripts live outside the repository in a local analysis lab, as the reference audio does.

## Arrangement findings

### Activity maps

In the maps below:

- `#` means within 10 dB of loud.
- `o` means 10–20 dB down.
- `.` means 20–30 dB down.
- A blank means more than 30 dB down, or silent.
- One character is one bar.

Böhmer, "Beyond Beliefs" (160 bars):

```
drums  |        #############o#o##############################o.                        .      .   ###o#######################.#######o#######o##############..#######o
bass   |        ################################################               ################    ###################################################################
other  |oooooo##ooo#ooo#ooo#ooo#ooooooo#ooo#ooo#ooooooooooo#ooo#oooooooooooooo##oo##o###########  #ooooooo#ooo#ooo#ooo#ooo#ooo#ooo#ooo#ooo#ooo##o##ooo#oo##oo##ooo#oooo
```

Durante, "Dream Controller" (216 bars). The first 50 bars:

```
drums  |##############oo##############oo################ooooooooooooooo###
bass   |##############  ##############  ################               ###
```

### What the maps show

- **The intro is one sound.**
  - Böhmer runs 8 bars of pluck and pad alone before drums and bass enter together.
  - "Come Together" and "Everybody Dreams" open with long, gappy ambient intros of 16 to 75 bars.
- **Phrases end with a dropout.** Dream Controller removes drums and bass for the last 2 bars of every 16-bar phrase in its first act, at bars 15–16 and 31–32. Lailonie removes the drums alone for 1–2 bars at bars 33–34 and 47–50. The music layer keeps playing through these dropouts, so the gap reads as breath, not as a stop.
- **Breakdowns are long and the bass leaves completely.**

  | Track | Breakdown | Bass |
  | --- | --- | --- |
  | Böhmer | 32 bars, 57–88 | Absent for 16 bars |
  | Durante | 36 bars, 114–150 | Absent |
  | Marsh | 28 bars, 115–143 | Absent |
  | Nox Vahn & Marsh | 46 bars, 124–170 | Mostly absent |

  In every case drums are absent or reduced to a filtered loop.
- **The drop is preceded by near-silence.**
  - Böhmer: 40 sixteenths (2.5 bars) of near-silence at bar 89, before the drop at bar 92.
  - Durante: 2-bar silences at bars 134 and 138 inside the break.
  - Lailonie: a 2-beat gap at bar 194.
- **Sections are 16 bars, and every entry or exit lands on one.**
  - Böhmer's bass enters at bar 9 and leaves at 57.
  - Durante's "other" stem steps up at 33, 49 and 64.
- **Vocals are sparse.** Across the five tracks, the vocal stem is active in a minority of bars, typically 8-bar phrases with long rests between them. "Everybody Dreams" is the exception: there the vocal is the long-form topline. No track runs a vocal hook, a lead melody and a dense arp at the same time.
- **The music layer is one sustained bed plus one moving part.**
  - The "other" stem's per-bar level stays within about 10 dB for long stretches. It is pad and pluck, not stab chords.
  - Böhmer's `ooo#` rhythm in that stem is a 4-bar figure whose accent lands once per phrase.

### Rules of thumb

1. Introduce at most one foreground part at a time: the arp, the vocal hook or a lead. The pad and texture layers are background, not foreground.
2. End every 16-bar phrase in the groove sections with a 1–2 bar dropout of drums and bass, keeping the pad and the moving part.
3. Make the breakdown 24–36 bars, with no bass and no kick.
4. Put 1 bar to 2.5 bars of near-silence before the main drop, with only reverb tails.
5. Start the track with the moving part and the pad alone for 8–16 bars.
6. Rest the vocal for at least as many bars as it sings.

## Sound-design findings (patch matching)

Patch matching is filled in from `patchlab.py` runs. Each run:

1. cuts an isolated stem segment;
2. transcribes it;
3. renders the same notes through a templated Crump synth with a chorus, saturation and reverb chain;
4. lets CMA-ES minimise a combined distance:
   - time-aligned log-mel spectrogram;
   - long-term spectrum;
   - loudness envelope;
   - 3-band stereo width.

## What the first matching pass got wrong

The first pass scored only the spectrum, the loudness envelope and stereo width. The search learned to fake a reference's thick, reverb-heavy spectrum with noise, filter drive, tape saturation and resonance. The resulting pad scored well and sounded like "an ear pressed against a speaker". Measured against the reference, it was:

- five times flatter (noisier);
- 6–8 dB lower in spectral contrast, because noise and distortion fill the valleys between harmonics;
- 5 dB less tonal.

A recovery test then checked whether the search could find patches that Crump itself had rendered. It could not:

- Final distances were 3.2, 4.3 and 6.0 instead of 0.
- Mean parameter error was 0.22–0.29 of each range; a random guess scores about 0.33.

A numeric match is therefore not evidence of a good sound by itself.

## The loop that works

1. **Blind listening rounds narrow the region.** Six loudness-matched candidates play over the fixture's own chords. The listener picks one or blends several. The previous favourite returns blind as a control.
2. **A perceptual score refines inside that region.** It adds four terms to the spectrum, envelope and width:
   - spectral contrast (clarity);
   - flatness (noisiness);
   - harmonic-to-percussive ratio (tonality);
   - crest factor.

   It also adds two terms of its own:
   - loudness and brightness movement in the 0.3–8 Hz bands (flow);
   - energy in the 2–5 kHz and 5–11 kHz bands (brightness).
3. **Hard caps on the parameter ranges** stop the score being gamed. Noise, drive, resonance and saturation have ceilings.
4. **The listener has the final say.** Different searches reached very different pads with the same score.

| Part | Reference | Outcome |
| --- | --- | --- |
| Pad | Dream Controller "other" stem, breakdown | Listening rounds led to a blend. A refinement then cut the distance from 8.8 to 5.1 (lower is closer). The result is six saws with `spread natural`, a slow 1.9 s swell and an open filter. |
| Horn | The same segment's bright top | The pads still lacked the reference's top. It measured 6 dB brighter at 2–5 kHz and 14 dB brighter at 5–11 kHz. A separate clean, swelling saw layer an octave up matched both bands at a noisiness of 0.29. |
| Pluck | Beyond Beliefs, intro "other" stem | Searches reached about 10.5. Hand-built "classic" plucks scored 25–37, because the reference intro pluck is much darker. |
| Bass | Dream Controller bass stem | The search plateaued at 16.6. Most of the remaining distance is the reference's sidechain pumping, which an isolated patch cannot reproduce. |

## Sound-design rules of thumb (measured, then confirmed by ear)

1. **Unison.** Evenly spaced detune beats at one regular rate. A held chord audibly pulses, with an envelope periodicity of 0.77–0.93.
   - Uneven spacing plus random drift (`spread natural`, `drift_shape wander`) cuts that to 0.25–0.28 with 5–7 voices.
   - Use at least 5 voices; 3 voices are too few to smear the beating.
2. **No amplitude LFO on held pads.** Listeners hear any level swell on a sustained chord as pumping.
3. **Same-pitch layers only add beating.** Put the second oscillator at an octave, a fifth, or an octave plus a fifth.
4. **Brightness comes from a separate clean layer, not from opening a detuned stack.** The open stack measured noisier, and listeners heard it as noisy; the clean swelling saw did not.
5. **Overlapping notes should crossfade, not glide.** Mono legato glide on the horn sounded like "a trombone slide". A 0.6 s fade-in under a 3.5 s tail sounded smooth.
6. **Slow chorus.** Searches repeatedly picked 2–3 Hz chorus to mimic movement. That reads as warble; 0.4–1.2 Hz scores almost the same.
7. **Keep the bass out of the music's midrange.** After saturation, the bass carried 41% of the 150 Hz–2 kHz energy, all of it mono. A 380–500 Hz low-pass on the bass did more for stereo width than widening any other part.

## Writing the track: loop first, then sections

A mix tuned to the references still sounded "super boring", because the material under it never changed: one chord loop and one figure for four minutes. Matching the mix numbers cannot fix that. What worked was to write the music in blind rounds, as the pad was designed. Every round was a set of short, loudness-matched renders with the recipe hidden behind a letter:

1. **Loop sketches.** Six 16-bar loops. Each had its own progression, bass rhythm, top part and swing. The listener picked two that shared a minor Am–F–G harmony, a busy sixteenth bass and light swing.
2. **Top-line synth.** Six patches played the same line over a fixed groove: supersaw, FM bell, acid, pulse, mallet and octave saw.
3. **Top line.** Six lines on the chosen synth. Twice, the listener blindly picked the control: the plain seven-step ostinato.
4. **Sections.** Several candidates each for the intro, the breakdown and the second drop, each heard with enough of the loop around it to judge the transition.

Listener feedback that shaped the result:

- **Carry the hook through the song.** Do not fade from one instrument into another. Break the hook up with variations of itself instead, such as its first bar on repeat.
- **Simplify.** A different variation every eight bars sounded busy.
- **Loop on repeat.** Skip the long outro, and end in the intro's state.

## Breaks, measured from Dream Controller

Per-bar stem levels show two kinds of break:

- **Phrase-end breaks** (bars 15–16 and 31–32): the bass vanishes, more than 50 dB down. The drums drop only about 10 dB, so the hats keep going. The music stem continues.
- **The breakdown** (bars 112–135): drums and bass fade over about ten bars. They do not cut. The music stem gets louder as they leave.
  - Two bars of near-silence follow.
  - Then the bass teases back in two-bar on/off blocks while the drums creep up from −38 dB to −19 dB.

## Kick, measured

Three reference kicks, aligned and median-averaged from their drum stems (`kick_profile.py`):

- **Pitch:** each settles at 45–50 Hz within about 50 ms.
- **Decay:** each falls 20 dB in 60–180 ms.
- **Click:** at most a soft one; the 2–8 kHz band sits about 18 dB under the body.

The first fixture kick was nothing like them. It fell slowly from 174 Hz and was still at 77 Hz after 50 ms. It took 330 ms to fall 20 dB, and it had no click. Listeners heard it as boomy and tom-like.

A synthesized kick shaped to the measurements won the blind kick round. In the full mix it then sounded "overpowering, too sharp", and the kick was not the main cause:

- Rendered on their own, the hats and claps were peaking above the kick, with a 23.5 dB crest factor.
- With the open hats, they carried more energy above 2 kHz than the whole music bus.
- Lowering the kick from −6 dB to −14 dB barely changed the mix's crest or low-end swing.

What fixed it:

- Cut the hats and claps by 5 dB and darken them.
- Remove the treble boost and most of the saturation that had given the old click-less kick its bite.
- Soften the new kick's click to 26 dB under the body.

## Caveats

- Demucs leaks between stems. The "other" stem mixes every non-drum, non-bass part, so a matched pad absorbs some pluck, horn and reverb character.
- Transcription uses CQT peaks and pYIN, and is approximate.
- These measurements describe five tracks and one listener's judgments.
