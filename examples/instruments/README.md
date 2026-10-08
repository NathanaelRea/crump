# Example instruments

60 house instruments, each with a readable Instrument Module and an audition Project. Acoustic instruments generate sound in the native graph. Multisample instruments include original procedural PCM banks, key/velocity mappings and generation recipes. No downloaded reference recordings are included.

## Audition

From the repository root, with `crump` built and on your path:

```bash
crump --project examples/instruments/hollow-block validate
crump --project examples/instruments/hollow-block publish --origin human --actor musician --summary "Audition Hollow Block"
crump --project examples/instruments/hollow-block play
```

Use `render --output renders/hollow-block` for an offline WAV. Each phrase includes soft/hard hits, short notes, repetition and, for pitched instruments, multiple registers. The Projects are instrument auditions; `check` additionally evaluates the separate Reference Musical Sketch contract.

## Copy into a Project

The destination must use `crump 2`. This helper requires only Python's standard library:

```bash
python3 examples/instruments/copy_instrument.py hollow-block /path/to/my-song
```

It copies the module and its media into `instruments/hollow-block/` and prints the import and asset declarations to paste into `project.crump`. It refuses to replace an existing instrument folder. The helper does not edit or publish your Project. Assign `instrument hollow-block` to a Track, then validate and publish through the normal Project workflow.

For multisamples, paste both the import before `project` and the printed asset declarations inside `project`, before `song`. Media paths are relative to the destination Project root; hashes and globally unique asset IDs are preserved. You can also copy these files manually and adjust the paths in the example Project's declarations.

Acoustic modules embed their profiles and require no media. They can also be exported or installed with the [module commands](../../docs/cli-and-agents.md). Multisample media must accompany its module; installing a module alone does not copy its assets.

## Playing and editing

Start with the default Faceplate settings and a Track at 0 dB. Pitched multisamples cover MIDI 24–91 with eight roots and two velocity layers. Percussion banks map all keys, use two velocity layers and two round robins; C4 plays their original pitch. Native percussion ignores key tracking, while native keys and mallets track pitch.

One-shot banks retain their natural decay after Note Off. Native bodies also ring independently of the gate. Voice stealing, the 30-second release ceiling and the Project's render range still bound tails. Raise polyphony only within resource admission. Dense chords or extreme control settings may need a lower Level.

The [acoustic model reference](../../docs/native-acoustics.md) explains the native controls and profiles. [`tools/instrument_assets`](../../tools/instrument_assets/README.md) regenerates the PCM banks from numerical recipes with deterministic seeds. `assets.json` records file hashes, generator identity, roots, velocity, seeds and gain; `recipe.json` retains the source model and treatment.

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
