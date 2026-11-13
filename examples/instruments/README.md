# Example instruments

92 instruments, each with a readable Instrument Module and an audition Project: 60 house instruments and a 32-piece orchestra. Acoustic instruments generate sound in the native graph. Multisample instruments include key/velocity mappings and the recipes that generate their original procedural PCM banks. No downloaded reference recordings are included.

## Setup

The multisample WAVs are generated rather than committed. From the repository root, generate them once after cloning, and again after pulling instrument changes:

```bash
uv run tools/instrument_assets/build.py --setup
```

It needs [uv](https://docs.astral.sh/uv/), which installs the pinned NumPy and SciPy, and checks every sample against the hash in its `assets.json`. Acoustic instruments need no media.

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

It copies the module and its media into `instruments/hollow-block/` and prints the one `import` line to add before `project`. Multisample modules declare their own media, with paths relative to the module folder, so nothing else is pasted. It refuses to replace an existing instrument folder. The helper does not edit or publish your Project. Assign `instrument hollow-block` to a Track, then validate and publish through the normal Project workflow. Copying a folder by hand works the same way.

Acoustic modules embed their profiles and require no media. They can also be exported or installed with the [module commands](../../docs/cli-and-agents.md). A personal-library module is one file, so multisample modules, which own media, are imported from inside a Project instead.

## Playing and editing

Start with the default Faceplate settings and a Track at 0 dB. House pitched multisamples cover MIDI 24–91 with eight roots and two velocity layers. Orchestral multisamples cover each instrument's playable range at sounding pitch with five to ten roots and two velocity layers. House percussion banks map all keys, use two velocity layers and two round robins; C4 plays their original pitch. Orchestral percussion instead stays within two semitones of its original pitch on every key from C2 to C#6. Native percussion ignores key tracking, while native keys and mallets track pitch.

Bowed, blown, sung and organ instruments loop their sustain for as long as a note is held, then play a natural release; their zones use multisample `loop` regions. Note Off also starts an amplitude envelope whose Attack and Release are on their Faceplates. Other one-shot banks retain their natural decay after Note Off. Native bodies also ring independently of the gate. Voice stealing, the 30-second release ceiling and the Project's render range still bound tails. Raise polyphony only within resource admission. Dense chords or extreme control settings may need a lower Level.

The [acoustic model reference](../../docs/native-acoustics.md) explains the native controls and profiles. [`tools/instrument_assets`](../../tools/instrument_assets/README.md) regenerates the PCM banks from numerical recipes with deterministic seeds. `assets.json` records file hashes, generator identity, roots, velocity, seeds and gain; `recipe.json` retains the source model and treatment. The bowed strings' recipes extend a shared family and name shared body curves in [`tools/instrument_assets/shared`](../../tools/instrument_assets/shared), whose hashes `assets.json` also records.

## Catalog

| Instrument | Category | Source |
| --- | --- | --- |
| [Felt Floor](felt-floor/instrument.crump) | Kicks | Procedural multisample |
| [Warehouse Skin](warehouse-skin/instrument.crump) | Kicks | Procedural multisample |
| [Disco Beater](disco-beater/instrument.crump) | Kicks | Procedural multisample |
| [Deep Pillow](deep-pillow/instrument.crump) | Kicks | Procedural multisample |
| [Dry Birch](dry-birch/instrument.crump) | Backbeats | Procedural multisample |
| [Disco Wire](disco-wire/instrument.crump) | Backbeats | Procedural multisample |
| [Brush Ghost](brush-ghost/instrument.crump) | Backbeats | Procedural multisample |
| [Three Hands](three-hands/instrument.crump) | Backbeats | Procedural multisample |
| [Basement Crowd](basement-crowd/instrument.crump) | Backbeats | Procedural multisample |
| [Palm Snap](palm-snap/instrument.crump) | Backbeats | Procedural multisample |
| [Brass Tick](brass-tick/instrument.crump) | Hats and metal | Procedural multisample |
| [Dark Closed](dark-closed/instrument.crump) | Hats and metal | Procedural multisample |
| [Offbeat Sizzle](offbeat-sizzle/instrument.crump) | Hats and metal | Procedural multisample |
| [Pedal Chick](pedal-chick/instrument.crump) | Hats and metal | Procedural multisample |
| [Ride Shoulder](ride-shoulder/instrument.crump) | Hats and metal | Acoustic graph |
| [Thin Crash](thin-crash/instrument.crump) | Hats and metal | Acoustic graph |
| [Conga Open](conga-open/instrument.crump) | Hand percussion | Procedural multisample |
| [Conga Mute](conga-mute/instrument.crump) | Hand percussion | Procedural multisample |
| [Conga Slap](conga-slap/instrument.crump) | Hand percussion | Procedural multisample |
| [Bongo High](bongo-high/instrument.crump) | Hand percussion | Procedural multisample |
| [Bongo Low](bongo-low/instrument.crump) | Hand percussion | Procedural multisample |
| [Djembe Center](djembe-center/instrument.crump) | Hand percussion | Procedural multisample |
| [Tabla Sketch](tabla-sketch/instrument.crump) | Hand percussion | Procedural multisample |
| [Wood Rim](wood-rim/instrument.crump) | Hand percussion | Acoustic graph |
| [Cuban Clave](cuban-clave/instrument.crump) | Hand percussion | Acoustic graph |
| [Hollow Block](hollow-block/instrument.crump) | Hand percussion | Acoustic graph |
| [Egg Shaker](egg-shaker/instrument.crump) | Shakers | Procedural multisample |
| [Gourd Maraca](gourd-maraca/instrument.crump) | Shakers | Procedural multisample |
| [Cabasa Turn](cabasa-turn/instrument.crump) | Shakers | Procedural multisample |
| [Tambourine Shake](tambourine-shake/instrument.crump) | Shakers | Acoustic graph |
| [Finger Bass](finger-bass/instrument.crump) | Bass | Procedural multisample |
| [Palm Bass](palm-bass/instrument.crump) | Bass | Procedural multisample |
| [Pick Bass](pick-bass/instrument.crump) | Bass | Procedural multisample |
| [Upright Pluck](upright-pluck/instrument.crump) | Bass | Procedural multisample |
| [Slap String](slap-string/instrument.crump) | Bass | Procedural multisample |
| [Deep Rubber](deep-rubber/instrument.crump) | Bass | Procedural multisample |
| [House Grand](house-grand/instrument.crump) | Keys | Acoustic graph |
| [Sunday Upright](sunday-upright/instrument.crump) | Keys | Acoustic graph |
| [Felt Afterhours](felt-afterhours/instrument.crump) | Keys | Procedural multisample |
| [Tack Keys](tack-keys/instrument.crump) | Keys | Acoustic graph |
| [Warm Tines](warm-tines/instrument.crump) | Keys | Procedural multisample |
| [Barking Tines](barking-tines/instrument.crump) | Keys | Procedural multisample |
| [Reed Keys](reed-keys/instrument.crump) | Keys | Procedural multisample |
| [Clav Pluck](clav-pluck/instrument.crump) | Keys | Procedural multisample |
| [Nylon Thumb](nylon-thumb/instrument.crump) | Guitar | Procedural multisample |
| [Disco Mute](disco-mute/instrument.crump) | Guitar | Procedural multisample |
| [Steel Finger](steel-finger/instrument.crump) | Guitar | Procedural multisample |
| [Small Harp](small-harp/instrument.crump) | Guitar | Procedural multisample |
| [Rosewood Marimba](rosewood-marimba/instrument.crump) | Mallets | Procedural multisample |
| [Soft Vibraphone](soft-vibraphone/instrument.crump) | Mallets | Procedural multisample |
| [Thumb Kalimba](thumb-kalimba/instrument.crump) | Mallets | Procedural multisample |
| [Buzz Balafon](buzz-balafon/instrument.crump) | Mallets | Acoustic graph |
| [Glass Drop](glass-drop/instrument.crump) | Mallets | Procedural multisample |
| [Bell Chime](bell-chime/instrument.crump) | Mallets | Procedural multisample |
| [Piano Cut](piano-cut/instrument.crump) | Resampling | Acoustic graph |
| [Dusty Keys](dusty-keys/instrument.crump) | Resampling | Procedural multisample |
| [Dub Tines](dub-tines/instrument.crump) | Resampling | Procedural multisample |
| [Reverse Nylon](reverse-nylon/instrument.crump) | Resampling | Procedural multisample |
| [Motor Vibes](motor-vibes/instrument.crump) | Resampling | Procedural multisample |
| [Room Chop](room-chop/instrument.crump) | Resampling | Procedural multisample |
| [Solo Violin](solo-violin/instrument.crump) | Strings | Procedural multisample |
| [Viola](viola/instrument.crump) | Strings | Procedural multisample |
| [Cello](cello/instrument.crump) | Strings | Procedural multisample |
| [Contrabass](contrabass/instrument.crump) | Strings | Procedural multisample |
| [String Section](string-section/instrument.crump) | Strings | Procedural multisample |
| [Pizzicato Strings](pizzicato-strings/instrument.crump) | Strings | Procedural multisample |
| [Concert Harp](concert-harp/instrument.crump) | Strings | Procedural multisample |
| [Piccolo](piccolo/instrument.crump) | Woodwinds | Procedural multisample |
| [Concert Flute](concert-flute/instrument.crump) | Woodwinds | Procedural multisample |
| [Oboe](oboe/instrument.crump) | Woodwinds | Procedural multisample |
| [Clarinet](clarinet/instrument.crump) | Woodwinds | Procedural multisample |
| [Bassoon](bassoon/instrument.crump) | Woodwinds | Procedural multisample |
| [Trumpet](trumpet/instrument.crump) | Brass | Procedural multisample |
| [French Horn](french-horn/instrument.crump) | Brass | Procedural multisample |
| [Tenor Trombone](tenor-trombone/instrument.crump) | Brass | Procedural multisample |
| [Tuba](tuba/instrument.crump) | Brass | Procedural multisample |
| [Brass Section](brass-section/instrument.crump) | Brass | Procedural multisample |
| [Timpani](timpani/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Concert Bass Drum](concert-bass-drum/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Concert Snare](concert-snare/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Clash Cymbals](clash-cymbals/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Suspended Cymbal](suspended-cymbal/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Tam-tam](tam-tam/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Triangle](triangle/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Castanets](castanets/instrument.crump) | Orchestral percussion | Procedural multisample |
| [Glockenspiel](glockenspiel/instrument.crump) | Tuned percussion | Procedural multisample |
| [Xylophone](xylophone/instrument.crump) | Tuned percussion | Procedural multisample |
| [Tubular Bells](tubular-bells/instrument.crump) | Tuned percussion | Procedural multisample |
| [Celesta](celesta/instrument.crump) | Orchestral keys | Procedural multisample |
| [Pipe Organ](pipe-organ/instrument.crump) | Orchestral keys | Procedural multisample |
| [Harpsichord](harpsichord/instrument.crump) | Orchestral keys | Procedural multisample |
| [Choir Aahs](choir-aahs/instrument.crump) | Voices | Procedural multisample |
