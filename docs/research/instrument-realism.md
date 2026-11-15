# Instrument realism pass

The world strings, world winds, saxophones, woodwinds, brass and guitars in the [example library](../../examples/instruments/README.md) sounded synthetic next to their [reference recordings](../../tools/instrument_assets/README.md#reference-recordings). This note records what made them sound that way, what changed, and how the result was measured.

## Method

Each instrument was rendered at the pitch of each reference clip and compared with it. A listening proxy, the [CLAP](https://huggingface.co/laion/clap-htsat-unfused) audio embedding, scored similarity as the cosine between the embeddings of the model and the recording, both peak-normalised. For scale, two different recordings of the same instrument average 0.78, two instruments of the same family 0.55, and unrelated instruments 0.41. Harmonic levels, the noise floor between harmonics and the onset or decay envelope were compared alongside, so a model could not score well on the proxy alone.

Resynthesis ablations of the recordings showed what the proxy, and the ear, attend to:

- A smooth spectral envelope is enough: smoothing the harmonic levels over half an octave cost almost nothing.
- The harmonics' own movement matters more: brightness that follows the breath and wanders with it, and partials that build and decay at their own rates.
- Plucked strings depend most on what lies between the harmonics. Exact exponential partials alone scored 0.33–0.57; adding back the recording's residual (pick and finger noise, the body's dense upper modes, the room) raised them to 0.65–0.91.
- Pitch jitter barely mattered.

## What changed

The old models had noise floors 30–70 dB cleaner than the recordings, plucked strings that lost their upper partials within 100 ms, and fixed spectra. The new sustained voice brightens with breath and velocity, carries pulsed breath noise and amplitude vibrato for flutes, and fits a few broad formant biquads to the recordings. The new plucked string releases the string with an integrated contact, loses partials through a linear-phase loop filter, settles from a sharp onset, and adds a knock of filtered noise and a small room. The [instrument README](../../examples/instruments/README.md#playing-and-editing) describes both patterns.

Formant fits were smoothed over a quarter octave and limited to Q 2.5 and ±15 dB per filter. Fits that still exceeded about ±12 dB overall were refitted to a clipped, half-octave-smoothed curve when that cost little similarity. Each module was level-matched to its predecessor, levelled across its range with a register curve sampled every three semitones, and given about 9 dB between velocities 0.48 and 0.92.

The reference list now slices single notes and records each clip's measured `hz`. Several clips had been compared at the wrong note (all three clarinet clips at D4, for example), and three multi-note clips were dropped. The classical guitar (Philharmonia) and harp (VSCO-2 CE) gained references.

## Results

Similarity to the references, old → new, with both sides peak-normalised:

| Category | Instruments |
| --- | --- |
| Saxophones | Alto 0.52 → 0.74, Tenor 0.58 → 0.76, Baritone 0.69 → 0.75 |
| Woodwinds | Flute 0.45 → 0.60, Oboe 0.61 → 0.66, Clarinet 0.50 → 0.64, Bassoon 0.46 → 0.63, English Horn 0.56 → 0.63, Bass Clarinet 0.57 → 0.73, Contrabassoon 0.52 → 0.75 |
| Brass | Trumpet 0.60 → 0.74, Muted Trumpet 0.55 → 0.67, Flugelhorn 0.65 → 0.83, French Horn 0.51 → 0.68, Euphonium 0.61 → 0.79, Bass Trombone 0.25 → 0.73, Tuba 0.53 → 0.72 |
| World winds | Recorder 0.39 → 0.85, Pan Flute 0.33 → 0.66, Shakuhachi 0.34 → 0.78, Bansuri 0.37 → 0.79, Ocarina 0.31 → 0.56, Harmonica 0.25 → 0.40, Accordion 0.78 → 0.85, Bagpipes 0.29 → 0.35, Didgeridoo 0.45 → 0.61 |
| World strings | Shamisen 0.41 → 0.57, Oud 0.40 → 0.67, Hammered Dulcimer 0.56 → 0.70 |
| Guitar | Nylon Thumb 0.49 → 0.54, Mandolin 0.29 → 0.37, Ukulele 0.45 → 0.60, Clean Electric 0.58 → 0.68, Crunch Guitar 0.60 → 0.66, Small Harp 0.50 → 0.61 |

Piccolo, Tenor Trombone, Brass Section, Duduk, Erhu, Hurdy-Gurdy, Banjo, Koto, Sitar and Disco Mute keep their previous models: no new candidate came closer to the recordings. A first sitar scored higher only because its bridge buzz, inside the string loop, had more than unity gain and sustained low notes indefinitely; buzz now acts on the string's output, and every plucked string sets its ring time per register. Steel Finger has no reference; its new model, derived from the classical guitar with a stiffer, brighter string, is labelled a steel-string acoustic guitar by the proxy where the old one was labelled a synth pluck.

## Limits

The proxy is not a listener, and the references vary: several are keyboard presets with their own room, and the bagpipe references disagree with each other. Plucked steel strings (banjo, mandolin, koto, shamisen) and the bagpipes remain furthest from their recordings. The fitting and evaluation scripts were exploratory and are not part of the repository.
