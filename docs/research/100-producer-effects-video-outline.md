# 100 producer effects: transcript-derived video outline

**Status:** Draft, informative research; not normative Crump documentation.

## Source and method

- **Video:** [“f\*\*k it, steal these 100 producer effects”](https://www.youtube.com/watch?v=D5dc7mQ30hU)
- **Channel:** morsbeats_
- **Duration:** 2:03:32
- **Accessed:** 2026-08-21
- **Primary evidence:** the video's English-original automatic captions (`en-orig`) and its 100 creator-authored chapter boundaries.

The captions were retrieved with `yt-dlp` in JSON3 form and grouped by the chapter intervals in the video's metadata. Music-only markers were removed. The complete machine transcript was used as temporary research input and is not checked into the repository.

The recipes below are concise paraphrases, not a verbatim transcript. Effect names follow the creator's chapter titles exactly, including the two chapters named “EZ Chops.” DAW/plugin names and numeric settings are retained where the captions provide them. “Unclear” flags recognition or terminology that cannot safely be repaired from the captions alone; no missing setting is invented. Result/use descriptions paraphrase the presenter or summarize the production purpose implied by the demonstrated recipe.

## What “effect” means in this video

The list is broader than 100 individual DSP processors. It mixes:

- **effect stacks:** EQ, delay, reverb, saturation, modulation, pitch shifting, and spectral/granular processing in series or parallel;
- **clip operations:** duplication, reversal, chopping, stretching, pitching, fading, and warp-mode changes;
- **automation:** filter, pitch, wet/dry, feedback, LFO, tempo, and device on/off movement;
- **routing:** return tracks, buses, sidechains, gates, vocoders, and parallel layers;
- **resampling:** freeze/flatten, bounce, resample, re-edit, and load the result into a sampler; and
- **arrangement devices:** risers, drop reveals, build-ups, transitions, ear candy, and call-and-response layers.

Most entries are therefore reproducible production recipes built from several ordinary operations, not a single named algorithm or plugin preset.

## Chapter-by-chapter outline

### 1. **Halftime** — [00:00](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=0s)

- **Recipe:** Duplicate the melody → stretch the copy to 2× length → pitch it −12 semitones → high-pass/cut lows around 100–150 Hz → blend with the original to taste.
- **Result/use:** A slower, octave-down layer that adds weight without excessive sub.

### 2. **Haunting Melodies** — [00:59](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=59s)

- **Recipe:** Duplicate melody → reverse → reverb about 8 s, 100% wet → freeze/flatten or bounce → stretch → reverse again and align → pitch −12 semitones → band-pass about 200 Hz–3 kHz.
- **Result/use:** A dark, distant melodic layer.

### 3. **Ambitextures** — [02:12](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=132s)

- **Recipe:** Play a simple chord → very large reverb, 60 s decay; captions explicitly say “dry” 100% (likely interface wording, but unclear) → freeze/flatten → trim the chord attack and quiet regions → EQ Eight band-pass roughly 400–500 Hz to ~1.5 kHz.
- **Result/use:** Background ambience and texture.

### 4. **Bass Booster** — [03:19](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=199s)

- **Recipe:** Create a return/bus → EQ at about 100 Hz → Decapitator (or another saturator), drive and wet mix fully up → send/blend the bass into it. **Unclear:** captions call the EQ a “low-pass” while saying it clears the very low end; those statements conflict, so filter direction is not inferred.
- **Result/use:** Parallel saturation that makes bass feel larger.

### 5. **Layers** — [04:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=270s)

- **Recipe:** Duplicate a simple melody → reverse each individual note (quarter-note hits in the example) → add fades → consolidate → pitch +12 semitones → in Ableton use Complex Pro and adjust formants to taste → stretch to 2×.
- **Result/use:** A high, floating reverse-note layer.

### 6. **RevCrush** — [05:56](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=356s)

- **Recipe:** Duplicate melody → reverse selected fragments → add Ableton Redux at ~50% dry/wet and ~11 kHz rate → automate Redux on only for reversed fragments → add tiny fades to remove clicks.
- **Result/use:** Intermittent reversed, bit-crushed glitches.

### 7. **Simples** — [07:19](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=439s)

- **Recipe:** Duplicate melody → pitch copy +12 semitones → Ableton Complex Pro, formants around 50 in the example → offset copy backward by one measure.
- **Result/use:** A simple octave layer with a bouncier call-and-response feel.

### 8. **Talking Synth** — [08:16](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=496s)

- **Recipe:** Put Auto Filter on a synth → enable sidechain and feed it a vocal; raise sidechain gain as needed → change filter to the setting auto-captioned “vow” (**unclear; do not infer a control name**) → pitch −36 semitones, Morph 100%, Envelope 100%.
- **Result/use:** Makes the synth articulate with the vocal, like a talking synth.

### 9. **Big Bass** — [09:11](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=551s)

- **Recipe:** In Ableton clip envelopes (or a pitch shifter), draw a strongly curved transposition fall to −12 semitones for each bass note → add aggressive saturation with the presenter’s “rumble” plugin: slight low boost, no mid boost, slight high boost.
- **Result/use:** Each bass note dives an octave and hits harder.

### 10. **Distortion Fun** — [10:11](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=611s)

- **Recipe:** Add triplet delay (“3” in the shown device), 0% feedback, ~35% wet → drive Waves OneKnob Driver (name inferred only from the presenter's spoken “one knob driver”) very hard/all the way → compensate output level.
- **Result/use:** Aggressive, rhythmically smeared distortion.

### 11. **Chops** — [11:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=690s)

- **Recipe:** Resample/bounce an eight-bar song loop → Slice to New MIDI Track at half notes → pitch slices −12 semitones → set attack ~120 ms → play a new keyboard melody from the slices.
- **Result/use:** Rebuilds the song into a floating chopped motif.

### 12. **LFO Synth** — [12:52](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=772s)

- **Recipe:** Low-pass the synth → map LFO to cutoff with 0–100 range → Sync Rate quarter note, Smoothing 100% → automate LFO Depth 0→100 and Offset −50→100%.
- **Result/use:** A filter pulse that grows and shifts across the phrase.

### 13. **1234** — [14:36](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=876s)

- **Recipe:** Start with repeated eighth/quarter-note plucks → duplicate and apply reverb at maximum decay/100% wet → bounce → resample dry and wet layers together → reverse each note-sized region → copy/paste a chosen measure into random positions.
- **Result/use:** Trippy, rearrangeable pluck patterns.

### 14. **Vibe** — [16:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=990s)

- **Recipe:** Duplicate melody twice and reverse both MIDI clips → first copy: quarter-note delay, 50% feedback, 100% wet; second: same but “third note”/triplet setting → bounce both → reverse audio back and align with original → band-pass each about 100 Hz–3 kHz.
- **Result/use:** Two filtered pre-echo layers around the original melody.

### 15. **Melody Trip** — [17:53](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1073s)

- **Recipe:** High-pass melody around 1.5 kHz → chorus 100% → reverb 14 s, 100% wet → bounce → stretch 2× and trim → low-pass around 2 kHz.
- **Result/use:** Soft, ambient, heavily blurred melody.

### 16. **EZ Songstarter** — [19:11](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1151s)

- **Recipe:** From one melody, make (a) counterline: duplicate, +12 semitones, reverse, offset ~eighth note; and (b) bass: duplicate MIDI, switch instrument to bass, legato notes, remove selected notes, pitch about −12 semitones as appropriate.
- **Result/use:** Quickly derives a counter-melody and bassline from one idea.

### 17. **Bass Drop** — [20:47](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1247s)

- **Recipe:** Before the drop, band-pass bass about 100–300 Hz → automate the EQ off exactly at the drop.
- **Result/use:** Withheld bandwidth opens suddenly, making the drop feel bigger.

### 18. **Reverse Stutters** — [21:45](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1305s)

- **Recipe:** Duplicate vocal chord → reverse → reverb 1.2 s, 100% wet → bounce → reverse back and align → stutter with sine shape, ~62% amount, 1/48-note speed → sidechain-compress to kick/snare → corrective vocal EQ as needed.
- **Result/use:** A pumping reverse-vocal stutter for a drop.

### 19. **Glitched Vocals** — [23:27](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1407s)

- **Recipe:** Stretch vocal 2× → consolidate → return it to original duration by dividing by two twice as stated (the arithmetic/operation is unclear in captions) → choose Ableton Beats warp mode → reverse.
- **Result/use:** A panting/trippy reverse-vocal glitch.

### 20. **Delayed Vox** — [24:12](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1452s)

- **Recipe:** Copy selected vocal words to a new track → quarter-note delay, 50% feedback, 100% wet (example 90 BPM) → reverb ~6.5 s, 32% wet → band-pass 350 Hz–3 kHz → pitch individual words in a 0, +7, +2-semitone pattern (example) using Complex mode.
- **Result/use:** Pitched delay throws behind the lead vocal.

### 21. **Elegant** — [25:45](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1545s)

- **Recipe:** Repeat the first vocal word/syllable manually up to the entry → consolidate and fade in → Little AlterBoy/other formant shifter, automate formant 0→−12 → reverb 5–6 s, automate wet 0→~65% → level-match.
- **Result/use:** A rising, increasingly diffuse vocal transition.

### 22. **Reverse Reverb** — [26:55](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1615s)

- **Recipe:** Copy first syllable → reverse → Ableton Long Tail reverb or equivalent, ~7 s, 100% wet → bounce → reverse again → align before the original and fade its end.
- **Result/use:** Classic reverse-reverb lead-in.

### 23. **Bouncy Stutters** — [27:48](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1668s)

- **Recipe:** Duplicate vocal → track one stutter at eighth-note triplet, 100% wet → track two stutter at 1/16 note, preceded by reverb 5 s/100% wet → blend both with source. Presenter uses a plugin auto-captioned “dark flutters” (**name unclear**).
- **Result/use:** Layered stutters with contrasting rhythms and ambience.

### 24. **Whispers** — [28:58](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1738s)

- **Recipe:** Duplicate and reverse vocal → Vocoder: 40 bands, range captioned 18–80 Hz, level +a few dB, release minimum, attack maximum, depth slightly raised → freeze/flatten → reverse and realign.
- **Result/use:** Creepy whisper layer under the source.

### 25. **Haunting Pad** — [30:25](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1825s)

- **Recipe:** Duplicate vocal → stretch repeatedly → reverse → choose a useful fragment and loop/extend it → reverb ~2.5–3 s, 100% wet → band-pass ~225 Hz–2.5 kHz → level to taste.
- **Result/use:** Turns a vocal fragment into a dark pad.

### 26. **Demon Time** — [31:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1890s)

- **Recipe:** Duplicate lead vocal → pitch −5 semitones in Complex mode → Decapitator drive ~7, middle tone, ~60% wet → lightly sidechain-compress from the original vocal.
- **Result/use:** Gritty low vocal tucked beneath the lead; suggested for drill.

### 27. **Grainy Vocals** — [32:38](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=1958s)

- **Recipe:** Duplicate/warp vocal → shorten by ÷2 three times → consolidate → stretch ×2 three times → Ableton Texture warp, Grain Size maximum, Flux minimum → reverb 1.2 s/~70% wet → automate grain size in recurring triangle shapes.
- **Result/use:** Moving granular vocal texture.

### 28. **Pretty Vocal** — [34:06](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2046s)

- **Recipe:** Copy chosen words → Little AlterBoy pitch −4 semitones → reverb ~6 s/~75% wet → chorus ~50% wet/full stereo → half-note delay, 33% feedback/33% wet → broad band-pass ~300 Hz–9 kHz.
- **Result/use:** Wide, polished vocal ear candy.

### 29. **Vocal Rewind** — [35:32](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2132s)

- **Recipe:** Slice vocal to New MIDI Track at half notes → perform a new vocal chop → duplicate and freeze/flatten → use MIDI notes as visual guides while reversing selected audio notes.
- **Result/use:** Custom chopped phrase with random rewind accents.

### 30. **Vocal Pad** — [36:51](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2211s)

- **Recipe:** Extract vocal fragment → huge reverb; captions say “60% decay” (unit/control unclear), 100% wet → freeze/flatten → trim intelligible attack → duplicate a section and reverse one copy → band-pass ~200 Hz–1.10 kHz → blend.
- **Result/use:** Flowing background pad derived from voice.

### 31. **Delay Pitch** — [38:15](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2295s)

- **Recipe:** Copy final vocal word, add fade-in → reverb ~7 s/100% wet → quarter-note delay, 95% feedback/50% wet → Little AlterBoy (or pitch shifter) 100% mix; halfway through automate 0→+2 semitones (match destination key) → band-pass about 170 Hz–3 kHz.
- **Result/use:** Bridges sections while the delay tail changes key.

### 32. **Pad Kick** — [39:59](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2399s)

- **Recipe:** Sequence four-on-floor kick and pitch groups of four (example 0, −4, +5, −8 semitones) → Convolution Reverb with a pad sample as impulse/carrier, attack ~500 ms, decay 20 s, 100% wet → high-pass ~800 Hz–1 kHz.
- **Result/use:** Converts kick rhythm into a pad-like texture.

### 33. **Snare Slaps** — [41:11](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2471s)

- **Recipe:** Cut a tiny piece from the snare → place it immediately before the hit → remove default fade and draw a very fast fade-in → repeat for each snare.
- **Result/use:** Adds a short pre-hit that makes snares slap harder.

### 34. **Bouncy Vocoder** — [41:48](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2508s)

- **Recipe:** Slice percussion loop to MIDI at quarter notes → generate/randomize a dense pattern (Ableton Seed or manual) → Vocoder carrier External, fed by a muted synth chord → adjust release and formant (~−4) → OTT ~30–35%.
- **Result/use:** Turns percussion into a bouncy pitched synth rhythm.

### 35. **Hat Roll** — [43:25](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2605s)

- **Recipe:** Repeat/chop hat into 1/16, 1/32 or 1/64 spacing to suit BPM → consolidate and fade in → low-pass ~8 kHz → reverb 2.76 s/~25% wet → 1/16 delay, 20% feedback/20% wet.
- **Result/use:** Atmospheric hat roll usable as a transition.

### 36. **Kick 2 808** — [44:45](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2685s)

- **Recipe:** Put Ableton Drum Buss on kick, Boom fully up and reduce output → Decapitator/other saturation, mix fully wet and tune settings → optionally freeze/flatten and load into MIDI sampler.
- **Result/use:** Synthesizes an 808-like sub from a kick.

### 37. **Alien Drums** — [45:35](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2735s)

- **Recipe:** Reverse random pieces of percussion loop → Ableton Phaser-Flanger in Flanger mode (or equivalent): ~13.2 ms time, Amount 0, Feedback ~80%, Wet ~25%; adjust by ear.
- **Result/use:** Creepy, alien-sounding drum loop.

### 38. **Reverse Delays** — [46:55](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2815s)

- **Recipe:** Reverse vocal → delay 100% wet/~40% feedback at the shown half-note setting (presenter notes Ableton naming ambiguity) → bounce → reverse back and align.
- **Result/use:** Delay repeats precede the words rather than trail them.

### 39. **Slappy Chorus** — [47:58](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2878s)

- **Recipe:** On return: eighth-note delay (presenter uncertain about Ableton display), 50% feedback/100% wet → Decapitator, fairly high drive/darker tone/100% mix → chorus full stereo/~50% wet → band-pass 150 Hz–3.5 kHz → send source to taste.
- **Result/use:** Gritty, wide slap/chorus parallel effect.

### 40. **Grainy** — [49:28](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=2968s)

- **Recipe:** On return: Grain Delay 100% wet, 1 Hz, pitch +12 semitones → reverb ~3.5 s/100% wet → low-pass ~1.3 kHz → send vocal or melody.
- **Result/use:** Skittering, grainy reverb-delay bounce in the background.

### 41. **Aesthetic Slaps** — [50:57](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3057s)

- **Recipe:** On return: 20 ms delay, 0% feedback, 100% wet → heavy distortion → reverb ~1 s/100% wet → band-pass ~400 Hz–2.5 kHz → send source.
- **Result/use:** Short, distorted, roomy slap tucked behind vocals/melodies.

### 42. **Reverb Swells** — [53:12](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3192s)

- **Recipe:** Add reverb with ~16 s decay → automate dry/wet in repeated smooth rise-and-fall shapes; vary curve/peak patterns and avoid abrupt click-producing edges.
- **Result/use:** Evolving swells within a single melodic track.

### 43. **Slow Stutters** — [54:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3270s)

- **Recipe:** Ableton Auto Filter, 24 dB low-pass, LFO Amount ~10%, Dry/Wet 100% → automate cutoff ~205 Hz→20 kHz → automate synced LFO rate from 1/32 to quarter note (BPM-dependent).
- **Result/use:** Stutter slows while the filter opens.

### 44. **Double Delays** — [55:56](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3356s)

- **Recipe:** Create two returns: quarter-note delay and third-note/triplet delay, each 30% feedback/100% wet → blend sends to taste; optionally change divisions.
- **Result/use:** Interlocking polyrhythmic echoes.

### 45. **Delay Swells** — [57:01](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3421s)

- **Recipe:** Add fast/slap delay at device setting “1,” feedback ~95% → automate wet mix with smooth rise/fall curves where echoes should bloom.
- **Result/use:** Delay blooms analogous to reverb swells.

### 46. **Ghostly** — [57:47](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3467s)

- **Recipe:** On bus: pitch shift −12 semitones/100% wet → reverb ~5.5 s/100% wet → Auto Pan sine, ~60% Amount, captioned 22 Hz (adjust for song) → send source.
- **Result/use:** Dark, moving octave-down ambience.

### 47. **Blur** — [59:03](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3543s)

- **Recipe:** On bus: tightly isolate around ~1.5 kHz with EQ (exact band shape unclear) → reverb ~4–4.5 s/100% wet → pitch +12 semitones at 50% wet with 350 ms delay window → send source.
- **Result/use:** High-mid, gliding ambient blur.

### 48. **Split Delays** — [1:00:21](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3621s)

- **Recipe:** Return A: low-pass ~1 kHz → quarter-note delay, 32% feedback/100% wet. Return B: complementary high-pass from ~1 kHz → shown half/eighth-note delay, 32% feedback/100% wet. Blend both sends and tune crossover/divisions.
- **Result/use:** Different delay rhythms occupy low and high frequency bands.

### 49. **Video Game** — [1:01:43](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3703s)

- **Recipe:** Bit crusher rate ~4–5 kHz, ~40% wet → pitch shifter −12 semitones at only ~12% wet.
- **Result/use:** Subtle octave layer plus crushed retro/8-bit character.

### 50. **Flanger Delays** — [1:02:40](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3760s)

- **Recipe:** On bus: high-pass ~350 Hz → shown half/eighth-note delay, 35% feedback/100% wet → MetaFlanger preset, Mix 50% (other displayed values not spoken) → reverb ~2.5 s/40% wet → send to taste.
- **Result/use:** Pretty, wide flanged echoes.

### 51. **Open Stutter** — [1:04:08](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3848s)

- **Recipe:** Low-pass and automate cutoff ~600 Hz→fully open → sine stutter; automate Amount 100→~70% and Rate 1/64→quarter note (BPM-dependent).
- **Result/use:** A filtered rapid stutter opens up, slows, and becomes less intense.

### 52. **Nice Delay** — [1:05:16](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3916s)

- **Recipe:** On return: eighth-note delay, 100% wet, ~25% feedback (presenter translates the device’s 0–100 display) → Decapitator ~75% wet/mid drive → band-pass ~500 Hz–3.3 kHz → send source.
- **Result/use:** Slappy, saturated telephone-like delay.

### 53. **Disenchanted** — [1:06:32](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=3992s)

- **Recipe:** Duplicate melody → half-note delay, 95% feedback/100% wet, filtered around 1.02 kHz (built-in delay filter or separate EQ) → reverb ~4 s/~70% wet → blend.
- **Result/use:** Ethereal sustained melodic layer.

### 54. **Tails** — [1:07:42](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4062s)

- **Recipe:** Extract vocal fragment → reverb ~14 s/100% wet → bounce → load tail into Ableton Simpler/other sampler → move sample start past recognizable vocal and add attack to avoid clicks → reverb again ~4 s/50% wet → play MIDI melody.
- **Result/use:** A playable instrument made from a reverb tail.

### 55. **Telephone** — [1:09:50](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4190s)

- **Recipe:** Apply Ableton EQ Eight “Telephone Vocal” preset (caption says book shows exact bands) → Redux/bit crusher at 40 kHz rate, 100% wet; lower bit depth by ear.
- **Result/use:** Telephone-band sound plus line/noise texture, usable on vocal or melody.

### 56. **Glitches** — [1:11:00](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4260s)

- **Recipe:** Duplicate source → reverb ~2 s/50% wet → quarter-note delay, 50% feedback/50% wet → Auto Pan Random at 1/16, Shape 100%, Amount 70% → Overdrive centered ~1.10 kHz, Drive/Tone ~50%, Dry/Wet ~24% → map another sine LFO (Depth 60%, one-bar rate, added jitter) to high- and low-pass cutoffs → blend.
- **Result/use:** Randomly gated, filtered, driven glitch layer.

### 57. **Radio** — [1:13:14](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4394s)

- **Recipe:** Apply EQ Eight “Telephone Vocal” preset (or reproduce the book’s displayed curve) → add Vinyl Distortion/crackle around 40.
- **Result/use:** Old-radio bandwidth and crackle.

### 58. **Broken Game** — [1:14:14](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4454s)

- **Recipe:** Warp Complex/Complex Pro → automate clip transposition 0→−12 semitones over two bars → Redux at 16 bits/100% wet, automate rate ~11 kHz→117 Hz → automate low-pass open→~250 Hz.
- **Result/use:** A pitch-falling, progressively destroyed game-like transition.

### 59. **Movements** — [1:15:20](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4520s)

- **Recipe:** Little AlterBoy pitch −5 semitones; automate Mix 100→0→100% → reverb ~4.5 s, automate wet ~100→20→100% → low-pass automate oppositely, ~1 kHz→open→1 kHz.
- **Result/use:** Background vocal moves between dark/washed and clear/dry states.

### 60. **Slowed & Reverb** — [1:17:16](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4636s)

- **Recipe:** Stretch full track/section to 2× length → add pitch shifter −12 semitones at ~40% wet → reverb ~3 s/~60% wet.
- **Result/use:** Familiar slowed-and-reverbed treatment with added octave character.

### 61. **Tape Stop** — [1:18:34](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4714s)

- **Recipe:** Put 100%-wet pitch shifter on target word/region → automate pitch rapidly from 0 to −24 semitones; adjust start and curve by ear.
- **Result/use:** Two-octave dive mimics tape stopping.

### 62. **LFO Vibes** — [1:19:44](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4784s)

- **Recipe:** Low-pass melody → reverb 1.2 s → map one LFO to filter cutoff and reverb dry/wet, both 0–100% → downward waveform, Smoothing ~10%, eighth-note rate → automate Depth in a triangle.
- **Result/use:** Linked rhythmic filtering and reverb that intensify then relax.

### 63. **Specter** — [1:21:09](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4869s)

- **Recipe:** Copy vocal to new track → pitch +12 semitones at ~50% mix → Ableton Spectral Time: 100% wet, fade-in 350 ms, fade-out 60 ms, Sensor 100%, Retrigger and Freeze on, Ultra resolution → low-pass ~6 kHz.
- **Result/use:** Frozen, spectral octave-up vocal apparition.

### 64. **Crush Transition** — [1:22:24](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=4944s)

- **Recipe:** Remove desired song section (presenter leaves drums/transition material) → group drums/risers/crashes → Redux ~6 kHz, 16 bits, 100% wet → high-pass ~150 Hz → automate both devices on only for transition.
- **Result/use:** Brief crushed, thin drum transition.

### 65. **Intro Suspense** — [1:23:37](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5017s)

- **Recipe:** Over intro, automate project BPM from one quarter to full tempo (example 40→160) → low-pass synth 350 Hz→22 kHz with curved ramp → automate pitch upward with similar curve; presenter estimates starting pitch about a fifth down but is explicitly unsure.
- **Result/use:** Slow, dark intro accelerates and opens into the song.

### 66. **Build Up** — [1:25:17](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5117s)

- **Recipe:** Chop a vocal fragment and repeat at roughly 1/16 notes (presenter is uncertain) → automate pitch 0→+12 semitones → reverb: automate decay 0.25→~11 s and wet ~20→100% (caption momentarily says “190%,” clearly unresolved) → leave tail ringing.
- **Result/use:** Any short sound becomes a rising build-up.

### 67. **Shifty Transition** — [1:26:29](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5189s)

- **Recipe:** On master, automate high- and low-pass filters in opposite directions to open a band-pass window (spoken endpoints include 114 Hz and 2.5 kHz, but mapping is unclear) → 100%-wet pitch shifter pattern +12→−24→+12 semitones→off.
- **Result/use:** Triple, skewed pitch movement paired with an opening filter.

### 68. **DJ** — [1:27:41](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5261s)

- **Recipe:** On master, low-pass 22 kHz→~400 Hz then reopen at drop → saw-shaped/inverted-phase stutter; automate Amount 0→80%; at 174 BPM use 1/16 for first 3/4 then 1/8 for final quarter.
- **Result/use:** DJ-style filtered slowdown/stutter into a drop.

### 69. **Rev Pitch** — [1:29:04](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5344s)

- **Recipe:** Resample transition section → reverse → pitch +12 semitones → automate high-pass from ~4 kHz toward fully open/off (**caption direction wording is unclear**).
- **Result/use:** Bright, reversed, pitched transition sweep.

### 70. **Autobots** — [1:30:14](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5414s)

- **Recipe:** On master, 100%-wet pitch shifter automate −24→0 semitones → high-pass automate ~2 kHz→10 Hz/off at transition.
- **Result/use:** Rising two-octave pitch transition with low end restored.

### 71. **Sweepy** — [1:31:07](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5467s)

- **Recipe:** On master, reverb 14 s; automate wet over final four bars to ~60%, then slowly down → high-pass 10 Hz→1 kHz, hold briefly, then release at drop.
- **Result/use:** Creates a spacious sweep/fake-out between sections.

### 72. **EZ Risers** — [1:32:46](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5566s)

- **Recipe:** Stretch any sound (example dog bark), fade in → Ableton Tones warp, Grain Size maximum → low-pass automate ~600 Hz→open → reverb 2.5 s, automate wet 0→~50%.
- **Result/use:** Turns an arbitrary one-shot into a riser.

### 73. **Subby Transition** — [1:33:51](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5631s)

- **Recipe:** Remove bass from transition → group drums → automate low-pass ~1.87 kHz→open and high-pass ~10 Hz→750 Hz to create opposing filter movement → place a sub drop, duplicate/reverse one copy, crossfade/align → add risers as desired.
- **Result/use:** Filtered drums plus forward/reverse subs make a low-end transition.

### 74. **Cut Transition** — [1:35:10](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5710s)

- **Recipe:** Duplicate transition vocal twice → pitch layers +12 and −12 semitones and blend → group drums/other music; repeat kick-containing segment at quarter/eighth-note spacing → automate group/device on-off for hard cuts and to suppress tails.
- **Result/use:** Conventional chopped stop-start transition with octave vocals.

### 75. **Rev Transition** — [1:36:19](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5779s)

- **Recipe:** Clear roughly one bar before drop → copy first bar of incoming melodies/basses backward into gap → reverse all and fade in → add cinematic boom; duplicate and reverse boom for second half, retaining some tail.
- **Result/use:** Incoming arrangement rises backward into the drop.

### 76. **Cut & Move** — [1:37:15](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5835s)

- **Recipe:** In transition half-bar, remove everything except lead synth → stagger percussion, kick and bass re-entries → high-pass lead from ~600 Hz toward open/off (**presenter self-corrects filter name**) → automate panning right↔left across fragments.
- **Result/use:** Sparse, funky, stereo-moving transition.

### 77. **Tran Flange** — [1:38:25](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5905s)

- **Recipe:** Remove drums for two-bar transition and group remaining melodies/risers → automate high-pass ~516 Hz→open with curve → Flanger Rate 2, Amount 0, Feedback ~75%, Wet ~43% → enable only in transition and automate Time ~0.1→20 ms.
- **Result/use:** Creaking, stretching “door opening” transition.

### 78. **Slow Down** — [1:39:41](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=5981s)

- **Recipe:** Resample song section → Slice to New MIDI Track at quarter notes → perform a new pattern → pitch −12 semitones → reverb 2.3 s/~13.5% wet → blend.
- **Result/use:** Slowed resampled phrase, suggested for outros.

### 79. **One At A Time** — [1:40:59](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6059s)

- **Recipe:** Put four effects on master (example Overdrive, Reverb, Pitch Shifter, EQ) → automate each on for successive quarter notes, turning one off as the next turns on; timing may vary with BPM.
- **Result/use:** Rapid serial effect changes for transitions/ear candy.

### 80. **Get Gated** — [1:41:48](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6108s)

- **Recipe:** Group melodies → keep a chopped trigger (example vocal chop) on separate track → put sidechained Gate on melody group, keyed from chop → set threshold so melodies pass only with trigger.
- **Result/use:** Melody group inherits the chop’s rhythm.

### 81. **Foley Fun** — [1:43:01](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6181s)

- **Recipe:** Use resonant EQ peaks at note-related frequencies across low/mid/high registers (presenter cites C at ~261/523/1046 Hz) to pull a chord from Foley → sample it with root at middle C → band-pass ~200–900 Hz → play chords.
- **Result/use:** Converts environmental audio into a pitched chord instrument. Exact additional frequency examples are unclear in captions.

### 82. **Lead Bounce** — [1:44:30](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6270s)

- **Recipe:** Start with one-shot (dog bark) → fast delay at device setting “1,” 95% feedback/50% wet → Flux Mini (free volume/LFO tool) for rhythmic gating → OTT ~60% → high-pass ~190 Hz → blend.
- **Result/use:** Bouncy lead/rhythm from a one-shot.

### 83. **Oneshot Bonanza** — [1:45:42](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6342s)

- **Recipe:** Take a short one-shot → strong Decapitator-style saturation (Mix 100%, Drive 6, Punish on in example) → reverb 8 s/100% wet → band-pass 200 Hz–2.5 kHz.
- **Result/use:** Distant, aggressive one-shot ear candy.

### 84. **Club Toilet** — [1:46:38](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6398s)

- **Recipe:** On master, low-pass around 800 Hz → reverb ~3.5 s/~38% wet.
- **Result/use:** Mimics club music heard through a bathroom wall.

### 85. **Low & Slow** — [1:47:16](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6436s)

- **Recipe:** Resample part of song → stretch 2× using Ableton Complex → pitch shift −12 semitones, 100% wet.
- **Result/use:** Dark, eerie, octave-down slow section.

### 86. **ONA** — [1:48:08](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6488s)

- **Recipe:** Start from one note → parallel pitch shifts −12 semitones/37% wet, −5/31%, −3/34% (adjust to form desired harmony) → reverb 18 s/100% wet.
- **Result/use:** “One Note Atmosphere”: a chordal pad from one sound.

### 87. **Re-Resampled** — [1:49:07](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6547s)

- **Recipe:** Resample a song section → freely reverse, duplicate, micro-repeat and pitch fragments → make a second edited variation → cut/deactivate alternating regions so original resample and edited version trade places.
- **Result/use:** Trippy arrangement assembled by repeatedly re-editing a bounce.

### 88. **Voice Gate** — [1:50:56](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6656s)

- **Recipe:** Put Gate on a pad → create live microphone audio track → sidechain Gate from microphone → set threshold → speak/vocalize to open the pad.
- **Result/use:** Voice rhythm dynamically plays/gates another sound.

### 89. **Trip** — [1:51:39](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6699s)

- **Recipe:** Warp vocal → stretch ×2 twice → consolidate → divide duration by two twice to restore length → Ableton Beats mode → reverse.
- **Result/use:** A trippy, artifact-rich reversed vocal.

### 90. **Counter Melody** — [1:52:21](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6741s)

- **Recipe:** Duplicate MIDI melody → Ableton Recombine, choose Shuffle, press Transform → select all generated notes and pitch +12 semitones → rerun Transform for alternatives.
- **Result/use:** Generates octave-up counter-melodies from existing notes.

### 91. **Vocal Repeat** — [1:53:23](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6803s)

- **Recipe:** Add Beat Repeat with spoken settings: quarter Interval, Chance 100%, Gate “7/16,” Grid “1/8,” Variation 10, Mix mode, output +3 dB; adjust its filter as shown. **Unclear:** auto-caption concatenates some displayed values, so only clearly spoken fields are preserved.
- **Result/use:** Rhythmic repeated-vocal accent.

### 92. **Drum Extraction** — [1:54:00](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6840s)

- **Recipe:** Right-click drum loop → Slice to New MIDI Track → slicing mode Transient → choose “Drums Gritty” preset → open rack and delete original/reference item as demonstrated → sequence pads into a new pattern.
- **Result/use:** Extracts hits from a loop for custom drums.

### 93. **Granulator 3** — [1:54:56](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6896s)

- **Recipe:** Load sample into Ableton Granulator III → automate Position to different regions each bar → at the same points automate Grain Size to tempo-related millisecond values (presenter cites 3.5 ms as 1/16 at 174 BPM, but that recognition/value may be unclear) → choose rhythmic sizes by conversion/listening.
- **Result/use:** Trippy, time-varying granular sample playback.

### 94. **Melody Repeater** — [1:56:08](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=6968s)

- **Recipe:** Add Beat Repeat; clearly captioned settings include quarter Interval, Chance 100%, 1/16 Grid, Pitch −12, Mix mode, output +6 dB, plus a filter. **Unclear:** captions merge additional displayed values (“sh 76k,” “7 times”), so they are not reconstructed.
- **Result/use:** Adds a pitched repeated layer to melody.

### 95. **Ableton EZ Stuts** — [1:56:53](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7013s)

- **Recipe:** Warp audio in Ableton Beats mode → Preserve at desired division (example 1/16) → select the demonstrated transient-loop mode → lower Transient Envelope until stuttering appears; change division for speed.
- **Result/use:** Native Ableton stutter without a dedicated plugin.

### 96. **EZ Chops** — [1:57:34](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7054s)

- **Recipe:** Slice vocal to New MIDI Track (quarter-note slicing in example) → clear generated MIDI → in Ableton Seed, raise Voices (presenter usually uses maximum) → click Generate repeatedly for new patterns.
- **Result/use:** Automatically generates vocal chops.

### 97. **EZ Chops** — [1:58:18](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7098s)

- **Recipe:** Starting from the previous generated chop, add pitch shifter +12 semitones at 50% wet → OTT 64% → high-pass ~200 Hz (source-specific) → reverb 4.3 s/40% wet.
- **Result/use:** A processed octave-up variation of “EZ Chops.” This is the second, separately authored chapter with the same exact title.

### 98. **Evolving Tremelo** — [1:59:01](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7141s)

- **Recipe:** Ableton Auto Pan in Tremolo mode: Amount 100%, synced 1/16, add Attack, invert shape → automate Shape 100→−100→100% in a triangle.
- **Result/use:** Tremolo waveform morphs over time. Chapter spelling “Tremelo” is preserved exactly.

### 99. **Ping Pong Ball** — [2:00:23](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7223s)

- **Recipe:** Load one-shot into Ableton Drum Sampler → Attack ~750 ms, Decay 1 s, Mode Gate, Effects Loop; enter middle-C MIDI note → automate filter 250 Hz→22 kHz and Loop Length 10→365 ms → reverb 8 s/30% wet → optionally invert both automation ramps in a later section.
- **Result/use:** Opening, accelerating/decelerating loop that resembles a bouncing ping-pong ball.

### 100. **Sample Offset** — [2:01:59](https://www.youtube.com/watch?v=D5dc7mQ30hU&t=7319s)

- **Recipe:** In clip editor, draw small Sample Offset changes while auditioning → then automate Transposition, periodically dipping about −5 semitones → refine both envelopes by ear; presenter explicitly treats placements as experimental.
- **Result/use:** Reorders/replaces micro-fragments and adds pitch dips for a glitchy vocal.

## Taxonomy summary

These categories overlap; they describe the main use rather than a strict ontology.

- **Melodic/harmonic transformation:** 1–3, 5, 7–8, 12–16, 32, 34, 47, 49, 53–54, 62–63, 81–82, 86, 90, 93–94, 98–99.
- **Vocal design and vocal ear candy:** 18–31, 38–41, 46, 51–52, 56, 59, 61, 63, 66, 80, 88–89, 91, 96–97, 100.
- **Drums, bass and groove:** 4, 9, 17, 32–37, 43, 64, 73–74, 77, 80, 92, 95.
- **Delay/reverb/spatial processing:** 2–3, 10, 13–15, 20–23, 28, 30–31, 38–48, 50, 52–54, 59–60, 62, 71–72, 83–84, 86, 99.
- **Transitions/build-ups/drop devices:** 17–18, 21–22, 31, 35, 42–45, 51, 58, 61, 64–79, 91, 99.
- **Resampling, chopping and arrangement:** 3, 5–6, 11, 13–16, 19, 27, 29–30, 36, 38, 54, 58, 60, 69, 72, 75, 78–79, 85, 87, 89–90, 92–97, 100.
- **Lo-fi/distortion/filter/spectral sound design:** 4, 6, 9–10, 24, 26–28, 37, 39–41, 47, 49, 52, 55–58, 63–64, 77, 83–84, 93–95.

## Verification note

A scripted check confirms 100 sequential chapter headings, 100 recipe bullets, and 100 result/use bullets. It also compares every title, timestamp, and chapter order with the 100 chapter objects in the video metadata. Both distinct chapters titled **“EZ Chops”** are present as entries 96 and 97.

## Primary-source citation

morsbeats_. [“f\*\*k it, steal these 100 producer effects.”](https://www.youtube.com/watch?v=D5dc7mQ30hU) YouTube, creator-authored chapters and English-original automatic captions, accessed 2026-08-21.
