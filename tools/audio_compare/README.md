# Audio comparison

Build a standalone HTML report and a machine-readable JSON report from any reference WAV and one or more candidate WAVs:

```bash
uv run tools/audio_compare/compare.py \
  --reference /path/to/reference.wav \
  --candidate 'Body A=/path/to/body-a.wav' \
  --candidate 'Body B=/path/to/body-b.wav' \
  --seconds 1 \
  --output renders/comparison.html
```

The HTML embeds its audio and charts, works offline and stops other players when a new sound starts. Candidate, spectrum-window, time-range and frequency-band selectors support direct comparisons. The JSON retains source hashes, preprocessing settings, versions, scores and chart arrays. Input WAVs remain unchanged. Generated reports belong in `renders/`; they can contain reference audio and are not part of the distributed library.

## What is measured

- **Impact development:** 1 ms RMS over the first 100 ms reveals immediate impulses, delayed body buildup and a muddy or weak onset.
- **Frequency balance:** spectra from 0–30, 30–100 and 100–300 ms separate contact, body and decay. Both sounds use a common scale.
- **Spectrogram and difference:** a 512-sample Hann window with 75% overlap exposes missing resonances, excess upper noise and persistent pitched tails. Log-frequency display uses 60 Hz–20 kHz; differences show candidate minus reference, capped at ±24 dB.
- **Band development:** third-order bandpasses and 5 ms RMS show when low body, presence and upper texture emerge and decay. Band-filter latency affects both signals equally.
- **Measurements:** spectral convergence and log-magnitude mean absolute error at 512 and 2,048 samples, relative envelope error, early RMS peak time and 90% cumulative-energy time. Spectral convergence is the magnitude difference norm divided by the reference norm. Log error uses a −80 dB reference floor and scores bins where either sound exceeds −60 dB relative to the reference peak. Zero error means identical analyzed signals.

WAV PCM and float inputs are folded to mono and resampled to 48 kHz. Alignment puts the first crossing of 2% of each file's peak at 2 ms, padding sounds that start immediately. It preserves the subsequent rise rather than aligning peak times. Listening and analysis use the same selected duration, including zero padding after shorter files.

Active RMS uses 10 ms blocks within 30 dB of the strongest block. Each signal is brought to the same active RMS, then one shared gain keeps every playback peak at or below 0.8. Source peaks, clipping counts, alignment positions and applied gains remain visible. Spectrogram colors and spectrum levels share the selected pair's scale. The tool does not compare stereo width or absolute source loudness.

## Using the result

Listen at matched level, locate the largest audible discrepancy, and change the smallest relevant model control. Compare multiple strikes or notes before choosing a change. Reserve different recordings and procedural seeds for evaluation when fitting parameters; a close fit to one recording can fail on another.

These are diagnostic errors, not a realism ranking. A noisy candidate can reduce spectral error while losing a musical resonance. Threshold onset alignment can misidentify a quiet precursor or a slow swell; inspect the reported onset and pre-trim inputs when necessary. Window size trades temporal precision for frequency resolution. Analysis is limited to the requested duration; use a longer duration for sustained sounds and tails. This tool does not run an optimizer or infer synthesis parameters.

## Checks

```bash
uv run --with numpy==2.3.3 --with scipy==1.16.2 \
  python -m unittest discover -s tools/audio_compare -p 'test_*.py'
```

Tests cover silence/non-finite rejection, unsigned and signed PCM, stereo folding, resampling, leading-silence invariance, level matching, frequency discrimination and standalone report output. The native [`acoustic_probe`](../../examples/acoustic_probe.rs) and [`filter_impact_probe`](../../examples/filter_impact_probe.rs) can supply candidate responses using Crump's own DSP.
