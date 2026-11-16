# Expressive native Instrument Modules

The fixture in [`fixtures/expressive/`](../fixtures/expressive/) contains readable `crump-module 1` Instruments with concise Faceplates, deeper Parameter/graph definitions, and no opaque preset state. The companion Project imports the modules and exercises velocity, amplitude and timbre envelopes, key tracking, glide, deterministic Modulation, stereo unison, shared post-voice effects, and release tails.

The module names are descriptive labels, not objective claims about subjective sound quality.

| Module | Faceplate / deeper Parameters | Authored voices | Graph work per stored voice | Replacement-aware graph work | Shared post-voice processors |
| --- | ---: | ---: | ---: | ---: | ---: |
| `warm-pad.crump` | 4 / 4 | 6 | 9 node-equivalents (6 unison subvoices) | 108 | delay + reverb |
| `melodic-pluck.crump` | 4 / 4 | 8 | 9 | 144 | delay |
| `deep-bass.crump` | 4 / 4 | 1 | 11 | 22 | — |
| `textured-percussion.crump` | 4 / 4 | 8 | 11 | 176 | reverb |
| `transition-noise.crump` | 4 / 4 | 4 | 7 | 56 | reverb |

“Replacement-aware” multiplies authored polyphony by the two preallocated graph-state slots used for the fixed 64-frame voice-replacement crossfade. Unison counts each active subvoice as one node-equivalent rather than hiding its work behind one authored Node. Processor estimates use each processor's own weighted work contract.

For the complete fixture at 48 kHz, `crump --project fixtures/expressive --json inspect` reports:

- 27 authored graph voices;
- 506 worst-case graph node-equivalents per frame, including replacement storage/work;
- 6 shared processor instances, including Master limiting;
- 2,431 total abstract work units per frame;
- 116,232 bytes of graph-voice state;
- 1,641,284 bytes of processor state;
- 1,887,977 bytes per admitted runtime and 3,775,954 bytes for two resident publication slots; and
- `admitted: true` under the named structural, work, and memory budgets.

`tests/expressive_instruments.rs` parses and validates every standalone module, verifies progressive Faceplate/Parameter/graph depth, compiles the imported fixture, freezes the facts above, and proves finite, non-silent, stereo, sample-exact output at 64/128/256-frame processing groupings. Focused graph tests separately cover velocity amplitude/timbre response, envelope-to-filter typing, key input, all four filter outputs, glide trajectory, deterministic unison drift, and bounded replacement discontinuity.

The pad, pluck, percussion, and transition ambience is declared as Instrument `processor` chains. It therefore runs once after voice summing for each Track instance; it is not multiplied into every graph voice. This is the supported seam for coherent delay/reverb tails across note and Clip boundaries.
