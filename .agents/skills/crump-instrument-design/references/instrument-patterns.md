# Instrument architecture patterns

## Warm polyphonic pad

- Two or more phase-stable harmonic sources or one wavetable source.
- Slow amplitude attack/release and independent timbre envelope.
- Key-tracked low-pass filtering with modest resonance.
- Per-voice drift kept below obvious detuning unless requested.
- Shared chorus, delay, and reverb after voice summing.
- Faceplate: tone, motion, width, space.

## Melodic pluck

- Bright oscillator/wavetable or short sample excitation.
- Fast amplitude decay and faster filter decay.
- Velocity to amplitude and cutoff.
- Optional tempo-synchronized shared delay.
- Faceplate: body, snap, brightness, echo.

## Deep bass

- Mono voicing with explicit note priority and glide.
- Fundamental-focused source plus controlled harmonic layer.
- Mono-compatible low end; stereo only above the bass band.
- Saturation before final low-pass when harmonics should remain bounded.
- Faceplate: weight, bite, glide, release.

## Granular texture

- Exact owned asset with reviewed region.
- Grain size/density chosen for event versus cloud behavior.
- Position scan plus bounded seeded spray.
- Explicit window and edge law.
- Note Off stops spawning while existing grains finish.
- Faceplate: position, grain, motion, spread.

## Wavetable lead

- Nyquist-bounded compiled table with modest frame morph.
- Velocity and note-age modulation for phrase movement.
- Mono/legato voicing when slides matter.
- Shared delay/reverb after the voice graph.
- Faceplate: position, bite, movement, space.

## Hybrid sampled instrument

- Multisample zones for identity and transient realism.
- Synth layer for sustain, pitch movement, or sub support.
- Explicit cross-layer gain and velocity laws.
- Deterministic round robin where repetition exposes static samples.
- Faceplate controls musical balance rather than raw zone details.

## Spectral or vocoded instrument

- Declare analysis/carrier/modulator identity and routing.
- Expose resolution/hold/smear/formant controls in bounded musical ranges.
- Account for algorithmic latency and tail state.
- Preserve a dry or intelligible reference path when appropriate.
