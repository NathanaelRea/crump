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

Winds and brass were first fitted to one to three notes near their reference and sounded right only there. They are now fitted to six to thirteen Philharmonia notes spread across each instrument's range. Their waveforms were also 2–6 dB peakier than the recordings, heard as buzz: a saw or pulse puts every harmonic in phase, one sharp edge per cycle. A few all-pass filters spread the phases, which brought the saxophones, English horn, contrabassoon and trumpet to the recordings' crest factor for at most 0.02 similarity. Formant fits were smoothed over a quarter octave and limited to Q 2.5 and ±15 dB per filter. Fits that still exceeded about ±12 dB overall were refitted to a clipped, half-octave-smoothed curve when that cost little similarity. Each module was level-matched to its predecessor, levelled across its range with a register curve sampled every three semitones, and given about 9 dB between velocities 0.48 and 0.92.

The reference list now slices single notes and records each clip's measured `hz`. Several clips had been compared at the wrong note (all three clarinet clips at D4, for example), and three multi-note clips were dropped. Philharmonia notes were added across the range of the saxophones, woodwinds and brass; their trombone labels are often an octave high, so every pitch was measured from harmonic spacing. The classical guitar (Philharmonia) and harp (VSCO-2 CE) gained references. Wikimedia Commons recordings replaced the harmonica, hurdy-gurdy and recorder references (the recorder's was a keyboard preset the proxy itself labels a synthesizer) and added held notes for the erhu, duduk, didgeridoo and bagpipes.

## Results

Similarity to the references, old → new, with both sides peak-normalised. Instruments whose references changed are scored against the new ones:

| Category | Instruments |
| --- | --- |
| Saxophones | Alto 0.60 → 0.74, Tenor 0.57 → 0.75, Baritone 0.69 → 0.77 |
| Woodwinds | Flute 0.52 → 0.77, Oboe 0.64 → 0.72, Clarinet 0.58 → 0.70, Bassoon 0.52 → 0.66, English Horn 0.59 → 0.73, Bass Clarinet 0.61 → 0.71, Contrabassoon 0.53 → 0.80 |
| Brass | Trumpet 0.58 → 0.79, Muted Trumpet 0.55 → 0.65, Flugelhorn 0.65 → 0.82, French Horn 0.56 → 0.68, Tenor Trombone 0.65 → 0.72, Bass Trombone 0.45 → 0.75, Euphonium 0.54 → 0.73, Tuba 0.59 → 0.71 |
| World winds | Recorder 0.36 → 0.49, Pan Flute 0.33 → 0.66, Shakuhachi 0.34 → 0.78, Bansuri 0.37 → 0.76, Ocarina 0.31 → 0.56, Duduk 0.55 → 0.60, Harmonica 0.54 → 0.63, Accordion 0.78 → 0.85, Bagpipes 0.37 → 0.43, Didgeridoo 0.40 → 0.53 |
| World strings | Erhu 0.61 → 0.63, Hurdy-Gurdy 0.47 → 0.65, Shamisen 0.41 → 0.57, Oud 0.40 → 0.67, Hammered Dulcimer 0.56 → 0.70 |
| Guitar | Nylon Thumb 0.49 → 0.54, Mandolin 0.29 → 0.37, Ukulele 0.45 → 0.60, Clean Electric 0.58 → 0.68, Crunch Guitar 0.60 → 0.66, Small Harp 0.50 → 0.61 |

Piccolo, Brass Section, Banjo, Koto, Sitar and Disco Mute keep their previous models: no new candidate came closer to the recordings. A first sitar scored higher only because its bridge buzz, inside the string loop, had more than unity gain and sustained low notes indefinitely; buzz now acts on the string's output, and every plucked string sets its ring time per register. Steel Finger has no reference; its new model, derived from the classical guitar with a stiffer, brighter string, is labelled a steel-string acoustic guitar by the proxy where the old one was labelled a synth pluck.

## Limits

The proxy is not a listener, and the references vary: several are keyboard presets with their own room, and the bagpipe references disagree with each other. Plucked steel strings (banjo, mandolin, koto, shamisen), the recorder and the bagpipes remain furthest from their recordings. Baritone sax, flugelhorn and muted trumpet still have a single reference each, so their range is less certain. The fitting and evaluation scripts were exploratory and are not part of the repository.
