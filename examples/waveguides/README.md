# Waveguide instruments

These three Instruments are written entirely in `.crump`. They use no media, no profiles and no offline generator. Each one is a delay loop built from the generic primitives in [ADR 0026](../../docs/adr/0026-feedback-and-waveguide-primitives.md): `delay_line`, `math`, `shaper`, `to_audio`, `dc_block` and `linear_phase_lowpass`. Open a module to change how the instrument behaves, not only its Faceplate.

| Instrument | Model |
| --- | --- |
| [Plucked String](plucked-string/instrument.crump) | A plectrum contact is integrated into a step and comb-filtered at the pick position. It then circulates in a pitch-tuned `delay_line` loop with a one-frame linear-phase loss filter and a per-trip sustain gain. Note Off fades the loop gain over the Release time. |
| [Sitar](sitar/instrument.crump) | The plucked string plus a jawari bridge. A pitch-tracked lowpass of the string gives a displacement-like swing. Past the bridge offset, `shaper curve one_sided` drives the string's `time_mod`, which shortens the string for as long as it touches the bridge. |
| [Clarinet](clarinet/instrument.crump) | A breath envelope, converted with `to_audio`, pushes against a `shaper curve table` reed. The reed feeds a half-period bore loop whose bell reflection is inverted, so the instrument sounds an octave below the loop length with mostly odd harmonics. |

Audition one like any example Project:

```bash
crump --project examples/waveguides/sitar validate
crump --project examples/waveguides/sitar publish --origin human --actor musician --summary "Audition Sitar"
crump --project examples/waveguides/sitar play
```

To use one in your own `crump 2` Project, copy its `instrument.crump` next to `project.crump` and add `import "instrument.crump"`.

## Tuning

`delay_line mode pitch` sets the loop delay from `note.pitch`. `tune_compensation 1` removes the one-frame delay of `linear_phase_lowpass`, so a loop's period is exact. [`tests/feedback_foundation.rs`](../../tests/feedback_foundation.rs) checks every module from E1 to C7: the plucked string is within 0.5 cent and the sitar within 1 cent. Bridge contact shortens a sitar string slightly, so its low notes measure up to 0.2 cent sharp. Each interpolation law tunes a bare loop within 2 cents from C1 to C7, and `lagrange3` keeps that accuracy at 44.1 and 96 kHz.

## Comparison with recordings

These figures come from rendering isolated notes and comparing them with the reference clips listed in `tools/instrument_assets/references.json`. The comparison uses that tool's onset, decay, centroid and sixth-octave spectrum functions. The harmonic envelope is the mean absolute difference of harmonics 1–16 in dB, measured 50–500 ms after the onset. The sixth-octave distance also counts the room noise, body noise and reverb between harmonics in the recordings, which a bare waveguide lacks. It is therefore larger than the harmonic-envelope distance.

| Model | Reference clip | Sixth-octave distance | Harmonic envelope | Centroid, model / reference |
| --- | --- | --- | --- | --- |
| Plucked String | Koto A2 | 14.0 dB | 5.8 dB | — |
| Plucked String | Koto G3 | 18.3 dB | 5.3 dB | — |
| Plucked String | Banjo A4 | 18.0 dB | 7.3 dB | — |
| Plucked String | Clean electric guitar D♯4 | 15.0 dB | 11.5 dB | — |
| Plucked String | Clean electric guitar C6 | 24.7 dB | 18.3 dB | darker than the reference |
| Sitar | Sitar G3 | 12.7 dB | 7.3 dB | 1466 / 1453 Hz |
| Sitar | Sitar F4 | 10.8 dB | 4.0 dB | 1835 / 2787 Hz |
| Clarinet | Bass clarinet A2 | 8.1 dB | 15.3 dB | 208 / 291 Hz |

Without the jawari, the same sitar string measures 16–20 dB from both sitar clips. The plucked string is darker than an electric guitar in its top octave, where the per-trip loss filter acts many times a second. The clarinet's even harmonics stay near −60 dB, while a real bore keeps them nearer −35 dB. Two delays for a conical bore would also be needed to model saxophones.

## Cost

A waveguide voice is admitted at about a hundred work units, most of them the `exciter`'s contact pool. Each `delay_line` costs four units plus `max_time × 48 000` f32 frames. Offline, the 22-second audition phrase renders in 1.5–2.2 seconds on one core, and the callback does not allocate.
