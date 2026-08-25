# Deep-house starter Instrument Modules

This source-readable library contains nine stable `crump-module 2` Instruments. Every module has tags and bounded `presentation.preview` metadata; audition sequencing therefore remains outside patch state. `auditions.crump` supplies one readable two-bar Audition Phrase and looping Clip per patch across a useful pitch/velocity range (or the four stock drum voices). `dh-keys` deliberately combines the exact multisample zones with one exact `sample_source` layer so the keys/pluck role demonstrates both source families.

`manifest.json` records exact module and Audition Project hashes, dependencies, tags, and the provenance of all copied WAV bytes. The four media files are byte-identical copies of the procedural MPL-2.0 assets in `fixtures/synth-alpha/media`; no external paths or third-party recordings are used.

Install or inspect a module with:

```bash
crump module install fixtures/starter/instruments/dh-bass.crump --name dh-bass
crump module inspect dh-bass --json
```

Asset-backed module preview requires a Project declaring and inventorying the dependencies named in the manifest. Both the authored Audition Project and the integrated Project at `fixtures/deep-house-reference` provide that context:

```bash
crump --project fixtures/starter/auditions.crump --json validate
crump --project fixtures/starter/auditions.crump --json play \
  --target pattern/audition-bass/audition-bass --loop --plan
```

Drop `--plan` only on the documented PipeWire setup. The Pattern, Track, and Clip IDs remain source-readable for exact navigation and revision.
