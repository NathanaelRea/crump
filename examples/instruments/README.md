# Example instruments

184 instruments, each with a readable Instrument Module and an audition Project: 60 house instruments, a 32-piece orchestra, and 92 synths, drum machines and world and band instruments ([why these](../../docs/research/instrument-library-expansion.md)). Every one is a native graph written in its `instrument.crump`: there are no samples, recipes or generated media, and nothing to set up. Open a module to change how the instrument behaves, not only its Faceplate. No downloaded reference recordings are included.

## Audition

From the repository root, with `crump` built and on your path:

```bash
crump --project examples/instruments/hollow-block validate
crump --project examples/instruments/hollow-block publish --origin human --actor musician --summary "Audition Hollow Block"
crump --project examples/instruments/hollow-block play
```

Use `render --output renders/hollow-block` for an offline WAV. To try everything at once, use the [instrument player](../../tools/instrument_player/README.md). Each phrase includes soft/hard hits, short notes, repetition and, for pitched instruments, multiple registers. The Projects are instrument auditions; `check` additionally evaluates the separate Reference Musical Sketch contract.

## Copy into a Project

The destination must use `crump 2`. This helper requires only Python's standard library:

```bash
python3 examples/instruments/copy_instrument.py hollow-block /path/to/my-song
```

It copies the module into `instruments/hollow-block/` and prints the one `import` line to add before `project`. Each module carries its own profiles, so nothing else is pasted. It refuses to replace an existing instrument folder. The helper does not edit or publish your Project. Assign `instrument hollow-block` to a Track, then validate and publish through the normal Project workflow. Copying a folder by hand works the same way.

Modules can also be exported or installed with the [module commands](../../docs/cli-and-agents.md).

## Playing and editing

Start with the default Faceplate settings and a Track at 0 dB. Pitched instruments play across their range; a hidden `key_curve` register route evens out their loudness from low to high. Unpitched percussion sounds the same on every key. Two-stroke hand drums and effects (Cajón, Darbuka, Frame Drum, Taiko, Udu, Agogô, Guiro, Cuíca, Mark Tree) play one stroke up to C4 and the other from C#4; each catalog entry's `about` names them and `strokes` gives a key for each. A `note_key` modulation route switches between the two sub-graphs. The bagpipes drone on fixed pitches (A) under every note, and the hurdy-gurdy on fixed G drones.

The graphs are built from a few patterns, named in a comment at the top of each module:

- **Struck bodies** (drums, bars, bells, cymbals, pianos): an `exciter` strike into a `modal_bank` whose profile follows the module. Noise layers are a `noise` source times an exponential envelope, a strike into a `onepole` leak. Pitch bends (kicks, toms, talking drum, opera gong) use an oscillator's `fm` port or a delay whose length changes.
- **Plucked strings** (guitars, harp, mandolin, ukulele, oud, shamisen, hammered dulcimer): an `exciter` contact, integrated by a one-pole leak into a released shape and notched by a pluck-position comb, feeds one or two pitch-tracked `delay_line` loops a few cents apart, the string's two polarizations or a course's pair. A `linear_phase_lowpass` in each loop makes upper partials die first; an all-pass stiffens steel strings, and a short envelope on the loop length makes a hard pluck start sharp and settle. The shamisen's string output passes a one-sided shaper, its buzzing bridge, so the buzz fades as the string decays. Each register's ring time is set directly, so low notes cannot sustain indefinitely. A `modal_bank` body, or a pickup comb, coil resonance, valve shaper and cabinet for the electric guitars, then a `biquad_chain` of broad formants fitted to recordings, a decaying knock of filtered noise and a small shared room (the Room control; 0 is dry) follow. Clean Electric and Hammered Dulcimer instead use a struck, unintegrated contact and a key-tracked low-pass loss, whose delay a `key_curve` on the loop length cancels to keep them in tune, with no knock or room; the electric guitars have no acoustic knock. Banjo, Koto, Disco Mute, the basses and the resampled keys keep an earlier, simpler string loop; the Sitar is the [waveguide example](../waveguides/README.md) sitar.
- **Sustained tones** (woodwinds, saxophones, brass, world winds, erhu, hurdy-gurdy): a band-limited saw, narrow pulse, or square with a high-passed saw for the clarinets' odd-harmonic low register, whose pitch moves with delayed vibrato, slow drift, fast flutter and an onset scoop. A low-pass that opens with the breath envelope and velocity, and wanders with the drift, makes louder notes brighter. Band-passed breath noise, partly pulsed by the waveform, rides on the tone; the flutes' tone locks in after their breath, and their vibrato also swells the level. A `biquad_chain` of all-passes spreads the harmonics' phases, as a reed and bore do, so a cycle is not one buzzy edge. Both pass through a `biquad_chain` of broad formants fitted to recordings across the instrument's range. The bagpipes' and hurdy-gurdy's drones take their pitch from `constant` Nodes; the didgeridoo adds its bore's resonances at fixed ratios to the note, and its mouth sweeps a band-pass. Velocity sets the attack, brightness, breathiness and loudness. Piccolo and Brass Section keep self-oscillating waveguides (an air jet or lips driving a `delay_line` bore). Voices, organs and the orchestral strings keep a saw through a fixed resonator bank fitted to their measured spectral envelope.
- **Drum machines**: swept sines and square clusters. Oscillators that must not follow the key take their pitch from a `constant` Node.

Struck and plucked instruments ring out after Note Off. Sustained ones hold while the key is down and fade with their Faceplate Release. Voice stealing, the 30-second release ceiling and the Project's render range still bound tails. Polyphony is sized so each instrument stays near 12,000 work units per frame; raise it only within resource admission. Dense chords or extreme control settings may need a lower Level.

The [acoustic model reference](../../docs/native-acoustics.md) explains `exciter`, `modal_bank` and profiles, and the [language reference](../../docs/language.md#feedback-loops) explains delay loops, `math`, `shaper`, signal conversion, `biquad_chain`, `constant` and `key_curve`. [`tools/instrument_assets`](../../tools/instrument_assets/README.md) compares the instruments against reference recordings.

## Catalog

| Instrument | Category | Source |
| --- | --- | --- |
| [Felt Floor](felt-floor/instrument.crump) | Kicks | Acoustic graph |
| [Warehouse Skin](warehouse-skin/instrument.crump) | Kicks | Acoustic graph |
| [Disco Beater](disco-beater/instrument.crump) | Kicks | Acoustic graph |
| [Deep Pillow](deep-pillow/instrument.crump) | Kicks | Acoustic graph |
| [Dry Birch](dry-birch/instrument.crump) | Backbeats | Acoustic graph |
| [Disco Wire](disco-wire/instrument.crump) | Backbeats | Acoustic graph |
| [Brush Ghost](brush-ghost/instrument.crump) | Backbeats | Acoustic graph |
| [Three Hands](three-hands/instrument.crump) | Backbeats | Acoustic graph |
| [Basement Crowd](basement-crowd/instrument.crump) | Backbeats | Acoustic graph |
| [Palm Snap](palm-snap/instrument.crump) | Backbeats | Acoustic graph |
| [Brass Tick](brass-tick/instrument.crump) | Hats and metal | Acoustic graph |
| [Dark Closed](dark-closed/instrument.crump) | Hats and metal | Acoustic graph |
| [Offbeat Sizzle](offbeat-sizzle/instrument.crump) | Hats and metal | Acoustic graph |
| [Pedal Chick](pedal-chick/instrument.crump) | Hats and metal | Acoustic graph |
| [Ride Shoulder](ride-shoulder/instrument.crump) | Hats and metal | Acoustic graph |
| [Thin Crash](thin-crash/instrument.crump) | Hats and metal | Acoustic graph |
| [Conga Open](conga-open/instrument.crump) | Hand percussion | Acoustic graph |
| [Conga Mute](conga-mute/instrument.crump) | Hand percussion | Acoustic graph |
| [Conga Slap](conga-slap/instrument.crump) | Hand percussion | Acoustic graph |
| [Bongo High](bongo-high/instrument.crump) | Hand percussion | Acoustic graph |
| [Bongo Low](bongo-low/instrument.crump) | Hand percussion | Acoustic graph |
| [Djembe Center](djembe-center/instrument.crump) | Hand percussion | Acoustic graph |
| [Tabla Sketch](tabla-sketch/instrument.crump) | Hand percussion | Acoustic graph |
| [Wood Rim](wood-rim/instrument.crump) | Hand percussion | Acoustic graph |
| [Cuban Clave](cuban-clave/instrument.crump) | Hand percussion | Acoustic graph |
| [Hollow Block](hollow-block/instrument.crump) | Hand percussion | Acoustic graph |
| [Egg Shaker](egg-shaker/instrument.crump) | Shakers | Acoustic graph |
| [Gourd Maraca](gourd-maraca/instrument.crump) | Shakers | Acoustic graph |
| [Cabasa Turn](cabasa-turn/instrument.crump) | Shakers | Acoustic graph |
| [Tambourine Shake](tambourine-shake/instrument.crump) | Shakers | Acoustic graph |
| [Finger Bass](finger-bass/instrument.crump) | Bass | Acoustic graph |
| [Palm Bass](palm-bass/instrument.crump) | Bass | Acoustic graph |
| [Pick Bass](pick-bass/instrument.crump) | Bass | Acoustic graph |
| [Upright Pluck](upright-pluck/instrument.crump) | Bass | Acoustic graph |
| [Slap String](slap-string/instrument.crump) | Bass | Acoustic graph |
| [Deep Rubber](deep-rubber/instrument.crump) | Bass | Acoustic graph |
| [House Grand](house-grand/instrument.crump) | Keys | Acoustic graph |
| [Sunday Upright](sunday-upright/instrument.crump) | Keys | Acoustic graph |
| [Felt Afterhours](felt-afterhours/instrument.crump) | Keys | Acoustic graph |
| [Tack Keys](tack-keys/instrument.crump) | Keys | Acoustic graph |
| [Warm Tines](warm-tines/instrument.crump) | Keys | Acoustic graph |
| [Barking Tines](barking-tines/instrument.crump) | Keys | Acoustic graph |
| [Reed Keys](reed-keys/instrument.crump) | Keys | Acoustic graph |
| [Clav Pluck](clav-pluck/instrument.crump) | Keys | Acoustic graph |
| [Nylon Thumb](nylon-thumb/instrument.crump) | Guitar | Acoustic graph |
| [Disco Mute](disco-mute/instrument.crump) | Guitar | Acoustic graph |
| [Steel Finger](steel-finger/instrument.crump) | Guitar | Acoustic graph |
| [Small Harp](small-harp/instrument.crump) | Guitar | Acoustic graph |
| [Rosewood Marimba](rosewood-marimba/instrument.crump) | Mallets | Acoustic graph |
| [Soft Vibraphone](soft-vibraphone/instrument.crump) | Mallets | Acoustic graph |
| [Thumb Kalimba](thumb-kalimba/instrument.crump) | Mallets | Acoustic graph |
| [Buzz Balafon](buzz-balafon/instrument.crump) | Mallets | Acoustic graph |
| [Glass Drop](glass-drop/instrument.crump) | Mallets | Acoustic graph |
| [Bell Chime](bell-chime/instrument.crump) | Mallets | Acoustic graph |
| [Piano Cut](piano-cut/instrument.crump) | Resampling | Acoustic graph |
| [Dusty Keys](dusty-keys/instrument.crump) | Resampling | Acoustic graph |
| [Dub Tines](dub-tines/instrument.crump) | Resampling | Acoustic graph |
| [Reverse Nylon](reverse-nylon/instrument.crump) | Resampling | Acoustic graph |
| [Motor Vibes](motor-vibes/instrument.crump) | Resampling | Acoustic graph |
| [Room Chop](room-chop/instrument.crump) | Resampling | Acoustic graph |
| [Solo Violin](solo-violin/instrument.crump) | Strings | Acoustic graph |
| [Viola](viola/instrument.crump) | Strings | Acoustic graph |
| [Cello](cello/instrument.crump) | Strings | Acoustic graph |
| [Contrabass](contrabass/instrument.crump) | Strings | Acoustic graph |
| [String Section](string-section/instrument.crump) | Strings | Acoustic graph |
| [Pizzicato Strings](pizzicato-strings/instrument.crump) | Strings | Acoustic graph |
| [Concert Harp](concert-harp/instrument.crump) | Strings | Acoustic graph |
| [Piccolo](piccolo/instrument.crump) | Woodwinds | Acoustic graph |
| [Concert Flute](concert-flute/instrument.crump) | Woodwinds | Acoustic graph |
| [Oboe](oboe/instrument.crump) | Woodwinds | Acoustic graph |
| [Clarinet](clarinet/instrument.crump) | Woodwinds | Acoustic graph |
| [Bassoon](bassoon/instrument.crump) | Woodwinds | Acoustic graph |
| [Trumpet](trumpet/instrument.crump) | Brass | Acoustic graph |
| [French Horn](french-horn/instrument.crump) | Brass | Acoustic graph |
| [Tenor Trombone](tenor-trombone/instrument.crump) | Brass | Acoustic graph |
| [Tuba](tuba/instrument.crump) | Brass | Acoustic graph |
| [Brass Section](brass-section/instrument.crump) | Brass | Acoustic graph |
| [Timpani](timpani/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Concert Bass Drum](concert-bass-drum/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Concert Snare](concert-snare/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Clash Cymbals](clash-cymbals/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Suspended Cymbal](suspended-cymbal/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Tam-tam](tam-tam/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Triangle](triangle/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Castanets](castanets/instrument.crump) | Orchestral percussion | Acoustic graph |
| [Glockenspiel](glockenspiel/instrument.crump) | Tuned percussion | Acoustic graph |
| [Xylophone](xylophone/instrument.crump) | Tuned percussion | Acoustic graph |
| [Tubular Bells](tubular-bells/instrument.crump) | Tuned percussion | Acoustic graph |
| [Celesta](celesta/instrument.crump) | Orchestral keys | Acoustic graph |
| [Pipe Organ](pipe-organ/instrument.crump) | Orchestral keys | Acoustic graph |
| [Harpsichord](harpsichord/instrument.crump) | Orchestral keys | Acoustic graph |
| [Choir Aahs](choir-aahs/instrument.crump) | Voices | Acoustic graph |
| [Acid Line](acid-line/instrument.crump) | Synth bass | Native synth graph |
| [Sub Sine](sub-sine/instrument.crump) | Synth bass | Native synth graph |
| [Reese Bass](reese-bass/instrument.crump) | Synth bass | Native synth graph |
| [Wobble Bass](wobble-bass/instrument.crump) | Synth bass | Native synth graph |
| [Log Drum Bass](log-drum-bass/instrument.crump) | Synth bass | Native synth graph |
| [Supersaw Lead](supersaw-lead/instrument.crump) | Synth leads | Native synth graph |
| [Chip Square](chip-square/instrument.crump) | Synth leads | Native synth graph |
| [Sync Lead](sync-lead/instrument.crump) | Synth leads | Native synth graph |
| [Hoover](hoover/instrument.crump) | Synth leads | Native synth graph |
| [Synth Brass](synth-brass/instrument.crump) | Synth pads and keys | Native synth graph |
| [String Machine](string-machine/instrument.crump) | Synth pads and keys | Native synth graph |
| [Analog Pad](analog-pad/instrument.crump) | Synth pads and keys | Native synth graph |
| [FM Bell](fm-bell/instrument.crump) | Synth pads and keys | Native synth graph |
| [FM Tines](fm-tines/instrument.crump) | Synth pads and keys | Native synth graph |
| [Rave Stab](rave-stab/instrument.crump) | Synth pads and keys | Native synth graph |
| [Noise Riser](noise-riser/instrument.crump) | Synth FX | Native synth graph |
| [Boom Kick](boom-kick/instrument.crump) | Drum machines | Acoustic graph |
| [Punch Kick](punch-kick/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Snare](machine-snare/instrument.crump) | Drum machines | Acoustic graph |
| [Snap Snare](snap-snare/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Clap](machine-clap/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Cowbell](machine-cowbell/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Hat](machine-hat/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Open Hat](machine-open-hat/instrument.crump) | Drum machines | Acoustic graph |
| [Machine Tom](machine-tom/instrument.crump) | Drum machines | Acoustic graph |
| [Kit Toms](kit-toms/instrument.crump) | Drum kit | Acoustic graph |
| [China Cymbal](china-cymbal/instrument.crump) | Drum kit | Acoustic graph |
| [Ride Bell](ride-bell/instrument.crump) | Drum kit | Acoustic graph |
| [Cajón](cajon/instrument.crump) | World percussion | Acoustic graph |
| [Darbuka](darbuka/instrument.crump) | World percussion | Acoustic graph |
| [Frame Drum](frame-drum/instrument.crump) | World percussion | Acoustic graph |
| [Taiko](taiko/instrument.crump) | World percussion | Acoustic graph |
| [Udu](udu/instrument.crump) | World percussion | Acoustic graph |
| [Talking Drum](talking-drum/instrument.crump) | World percussion | Acoustic graph |
| [Agogô](agogo/instrument.crump) | World percussion | Acoustic graph |
| [Guiro](guiro/instrument.crump) | World percussion | Acoustic graph |
| [Cuíca](cuica/instrument.crump) | World percussion | Acoustic graph |
| [Sleigh Bells](sleigh-bells/instrument.crump) | World percussion | Acoustic graph |
| [Temple Blocks](temple-blocks/instrument.crump) | World percussion | Acoustic graph |
| [Slit Drum](slit-drum/instrument.crump) | World percussion | Acoustic graph |
| [Vibraslap](vibraslap/instrument.crump) | World percussion | Acoustic graph |
| [Mark Tree](mark-tree/instrument.crump) | World percussion | Acoustic graph |
| [Steel Pan](steel-pan/instrument.crump) | World mallets | Acoustic graph |
| [Handpan](handpan/instrument.crump) | World mallets | Acoustic graph |
| [Singing Bowl](singing-bowl/instrument.crump) | World mallets | Acoustic graph |
| [Gamelan Saron](gamelan-saron/instrument.crump) | World mallets | Acoustic graph |
| [Gamelan Bonang](gamelan-bonang/instrument.crump) | World mallets | Acoustic graph |
| [Music Box](music-box/instrument.crump) | World mallets | Acoustic graph |
| [Opera Gong](opera-gong/instrument.crump) | World mallets | Acoustic graph |
| [Banjo](banjo/instrument.crump) | Guitar | Acoustic graph |
| [Mandolin](mandolin/instrument.crump) | Guitar | Acoustic graph |
| [Ukulele](ukulele/instrument.crump) | Guitar | Acoustic graph |
| [Clean Electric](clean-electric/instrument.crump) | Guitar | Acoustic graph |
| [Crunch Guitar](crunch-guitar/instrument.crump) | Guitar | Acoustic graph |
| [Fretless Bass](fretless-bass/instrument.crump) | Bass | Acoustic graph |
| [Sitar](sitar/instrument.crump) | World strings | Acoustic graph |
| [Koto](koto/instrument.crump) | World strings | Acoustic graph |
| [Shamisen](shamisen/instrument.crump) | World strings | Acoustic graph |
| [Oud](oud/instrument.crump) | World strings | Acoustic graph |
| [Hammered Dulcimer](hammered-dulcimer/instrument.crump) | World strings | Acoustic graph |
| [Erhu](erhu/instrument.crump) | World strings | Acoustic graph |
| [Hurdy-Gurdy](hurdy-gurdy/instrument.crump) | World strings | Acoustic graph |
| [Tremolo Strings](tremolo-strings/instrument.crump) | Strings | Acoustic graph |
| [Alto Sax](alto-sax/instrument.crump) | Saxophones | Acoustic graph |
| [Tenor Sax](tenor-sax/instrument.crump) | Saxophones | Acoustic graph |
| [Baritone Sax](baritone-sax/instrument.crump) | Saxophones | Acoustic graph |
| [English Horn](english-horn/instrument.crump) | Woodwinds | Acoustic graph |
| [Bass Clarinet](bass-clarinet/instrument.crump) | Woodwinds | Acoustic graph |
| [Contrabassoon](contrabassoon/instrument.crump) | Woodwinds | Acoustic graph |
| [Recorder](recorder/instrument.crump) | World winds | Acoustic graph |
| [Pan Flute](pan-flute/instrument.crump) | World winds | Acoustic graph |
| [Shakuhachi](shakuhachi/instrument.crump) | World winds | Acoustic graph |
| [Bansuri](bansuri/instrument.crump) | World winds | Acoustic graph |
| [Ocarina](ocarina/instrument.crump) | World winds | Acoustic graph |
| [Duduk](duduk/instrument.crump) | World winds | Acoustic graph |
| [Harmonica](harmonica/instrument.crump) | World winds | Acoustic graph |
| [Accordion](accordion/instrument.crump) | World winds | Acoustic graph |
| [Bagpipes](bagpipes/instrument.crump) | World winds | Acoustic graph |
| [Didgeridoo](didgeridoo/instrument.crump) | World winds | Acoustic graph |
| [Flugelhorn](flugelhorn/instrument.crump) | Brass | Acoustic graph |
| [Muted Trumpet](muted-trumpet/instrument.crump) | Brass | Acoustic graph |
| [Euphonium](euphonium/instrument.crump) | Brass | Acoustic graph |
| [Bass Trombone](bass-trombone/instrument.crump) | Brass | Acoustic graph |
| [Drawbar Organ](drawbar-organ/instrument.crump) | Keys | Acoustic graph |
| [Tape Flute](tape-flute/instrument.crump) | Keys | Acoustic graph |
| [Tape Strings](tape-strings/instrument.crump) | Keys | Acoustic graph |
| [Glass Harmonica](glass-harmonica/instrument.crump) | Keys | Acoustic graph |
| [Honky-Tonk](honky-tonk/instrument.crump) | Keys | Acoustic graph |
| [Prepared Piano](prepared-piano/instrument.crump) | Keys | Acoustic graph |
| [Choir Oohs](choir-oohs/instrument.crump) | Voices | Acoustic graph |
| [Soprano Voice](soprano-voice/instrument.crump) | Voices | Acoustic graph |
| [Bass Voice](bass-voice/instrument.crump) | Voices | Acoustic graph |
