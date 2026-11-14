# Instrument library expansion

The example library had 92 instruments: 60 house instruments (drums, bass, keys, guitar, mallets, resampled keys) and a 32-piece Western orchestra. This survey records what that set left out and the 92 instruments added to double it.

## What the library covered

| Area | Instruments |
| --- | --- |
| House drums | 4 kicks, 6 backbeats, 6 hats and cymbals, 10 hand percussion, 4 shakers |
| House melodic | 6 basses, 8 keys, 4 guitars, 6 mallets, 6 resampled keys |
| Orchestra | 7 strings, 5 woodwinds, 5 brass, 8 percussion, 3 tuned percussion, 3 keys, 1 choir |

Every instrument was a procedural multisample or a native acoustic graph. None used the engine's virtual-analog path (`analog_oscillator`, `unison`, the ladder filter, envelopes and LFOs).

## Gaps

1. **Synthesizers.** A text-first tool for electronic tracks had no synth bass, lead, pad, stab or FX patch. The engine already supports them natively. This was the largest gap for the target user.
2. **Drum machines.** Classic analog drum-machine voices (long sub kick, punchy kick, tone-plus-noise snares, clap, cowbell, square-cluster hats, toms) carry most house and techno. The house kits were acoustic-modelled.
3. **Drum kit.** There were no toms, china cymbal or ride bell.
4. **World percussion.** Latin hand drums were well covered. Middle Eastern, African, Japanese and Brazilian drums, scrapers, friction drums, bells and chimes were not.
5. **World melodic instruments.** There was no steel pan, handpan, gamelan, singing bowl, sitar, koto, shamisen, oud, erhu, hurdy-gurdy, dulcimer, end-blown or ethnic flute, duduk, free reed, bagpipe or didgeridoo.
6. **Band instruments.** The library lacked saxophones, electric guitars, banjo, mandolin, ukulele and fretless bass.
7. **Orchestral auxiliaries.** English horn, bass clarinet, contrabassoon, flugelhorn, muted trumpet, euphonium, bass trombone and tremolo strings were missing.
8. **Vintage keys.** There was no tonewheel organ with rotary speaker, tape-replay keyboard, honky-tonk or prepared piano, or glass harmonica.
9. **Voices.** There was one choir vowel and no solo voices.

## Additions

| Category | Instruments | Source |
| --- | --- | --- |
| Synth bass | Acid Line, Sub Sine, Reese Bass, Wobble Bass, Log Drum Bass | Native synth graph |
| Synth leads | Supersaw Lead, Chip Square, Sync Lead, Hoover | Native synth graph |
| Synth pads and keys | Synth Brass, String Machine, Analog Pad, FM Bell, FM Tines, Rave Stab | Native synth graph |
| Synth FX | Noise Riser | Native synth graph |
| Drum machines | Boom Kick, Punch Kick, Machine Snare, Snap Snare, Machine Clap, Machine Cowbell, Machine Hat, Machine Open Hat, Machine Tom | Procedural multisample |
| Drum kit | Kit Toms, China Cymbal, Ride Bell | Procedural multisample |
| World percussion | Cajón, Darbuka, Frame Drum, Taiko, Udu, Talking Drum, Agogô, Guiro, Cuíca, Sleigh Bells, Temple Blocks, Slit Drum, Vibraslap, Mark Tree | Procedural multisample |
| World mallets | Steel Pan, Handpan, Singing Bowl, Gamelan Saron, Gamelan Bonang, Music Box, Opera Gong | Procedural multisample |
| Guitar | Banjo, Mandolin, Ukulele, Clean Electric, Crunch Guitar | Procedural multisample |
| Bass | Fretless Bass | Procedural multisample |
| World strings | Sitar, Koto, Shamisen, Oud, Hammered Dulcimer, Erhu, Hurdy-Gurdy | Procedural multisample |
| Strings | Tremolo Strings | Procedural multisample |
| Saxophones | Alto Sax, Tenor Sax, Baritone Sax | Procedural multisample |
| Woodwinds | English Horn, Bass Clarinet, Contrabassoon | Procedural multisample |
| World winds | Recorder, Pan Flute, Shakuhachi, Bansuri, Ocarina, Duduk, Harmonica, Accordion, Bagpipes, Didgeridoo | Procedural multisample |
| Brass | Flugelhorn, Muted Trumpet, Euphonium, Bass Trombone | Procedural multisample |
| Keys | Drawbar Organ, Tape Flute, Tape Strings, Glass Harmonica, Honky-Tonk, Prepared Piano | Procedural multisample |
| Voices | Choir Oohs, Soprano Voice, Bass Voice | Procedural multisample |

Synth patches are readable native graphs, so a user can open and change them. They need no media and show the engine's synthesis path, which no example used before. Acoustic additions extend the procedural generator, as the orchestra did, and keep the library free of recorded audio.

### Considered and left out

Close variants of existing or added instruments were dropped: soprano sax, alto flute, tin whistle, cornet, guzheng, 12-string guitar, crotales, toy piano, surdo, timbales, rainstick, ratchet, slapstick, anvil, combo organ, theremin, splash cymbal, cross-stick and drum-machine rim, conga and maracas. Each could be added later as another recipe on the same models.

## References

Reference recordings are analysis and listening aids only. They are downloaded locally by `tools/instrument_assets/references.py` into `renders/instrument-references/` and never committed or distributed. `references.json` records each clip's source page, author licence and the slice used.

- [University of Iowa Musical Instrument Samples](https://theremin.music.uiowa.edu/MIS.html): isolated orchestral notes (alto sax, bass clarinet, bass trombone, china cymbal, ride bell).
- [VSCO-2 Community Edition](https://github.com/sgossner/VSCO-2-CE), CC0: cowbell, guiro, log drums, sleigh bells, bell tree.
- [Wikimedia Commons](https://commons.wikimedia.org/): freely licensed recordings of world instruments, drum machines and vintage keys. Licences vary per file (CC0, public domain, CC BY, CC BY-SA, Artistic 2.0).

Synth patches have no reference recording. Each instead has a short target description of the character it should have.
