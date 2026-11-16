#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Compare reference and candidate WAVs in a standalone listening/analysis report."""

import argparse
import base64
import hashlib
import io
import json
import math
from pathlib import Path

import numpy as np
import scipy
from scipy import signal
from scipy.io import wavfile

RATE = 48000
VERSION = "crump-audio-compare-v1"
FREQUENCIES = np.geomspace(60, 20000, 160)
BANDS = [
    (60, 250),
    (250, 1000),
    (1000, 2000),
    (2000, 4000),
    (4000, 8000),
    (8000, 20000),
]


def read(path):
    rate, raw = wavfile.read(path)
    if not 8000 <= rate <= 384000 or raw.ndim not in (1, 2):
        raise ValueError(f"Unsupported WAV shape/rate: {path}")
    if raw.dtype.kind == "u":
        midpoint = float(2 ** (raw.dtype.itemsize * 8 - 1))
        audio = (raw.astype(float) - midpoint) / midpoint
    elif raw.dtype.kind == "i":
        audio = raw.astype(float) / float(2 ** (raw.dtype.itemsize * 8 - 1))
    elif raw.dtype.kind == "f":
        audio = raw.astype(float)
    else:
        raise ValueError(f"Unsupported WAV encoding: {path}")
    if audio.size == 0 or not np.all(np.isfinite(audio)):
        raise ValueError(f"Empty or non-finite WAV: {path}")
    source = {
        "filename": path.name,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "sample_rate": int(rate),
        "frames": len(audio),
        "channels": 1 if audio.ndim == 1 else audio.shape[1],
        "peak": float(np.max(np.abs(audio))),
        "clipped_samples": int(np.sum(np.abs(audio) >= 1)),
    }
    if audio.ndim == 2:
        audio = audio.mean(axis=1)
    if rate != RATE:
        common = math.gcd(int(rate), RATE)
        audio = signal.resample_poly(audio, RATE // common, int(rate) // common)
    if np.max(np.abs(audio)) < 1e-9:
        raise ValueError(f"Silent mono signal: {path}")
    return audio, source


def align(audio, seconds):
    """Place the first 2%-of-peak crossing at 2 ms, even without leading silence."""
    peak = np.max(np.abs(audio))
    if not np.isfinite(peak) or peak < 1e-9:
        raise ValueError("Cannot align a silent/non-finite signal")
    onset = int(np.flatnonzero(np.abs(audio) >= peak * 0.02)[0])
    preroll = 96
    start = max(0, onset - preroll)
    audio = np.pad(audio[start:], (max(0, preroll - onset), 0))
    frames = round(seconds * RATE)
    return np.pad(audio[:frames], (0, max(0, frames - len(audio)))), onset


def active_rms(audio):
    power = np.square(audio)
    frames = 480
    blocks = np.pad(power, (0, (-len(power)) % frames)).reshape(-1, frames).mean(axis=1)
    return float(np.sqrt(blocks[blocks >= blocks.max() * 0.001].mean()))


def match_levels(signals):
    gains = [0.1 / max(active_rms(x), 1e-12) for x in signals]
    ceiling = max(float(np.max(np.abs(x))) * g for x, g in zip(signals, gains))
    headroom = min(1, 0.8 / max(ceiling, 1e-12))
    gains = [g * headroom for g in gains]
    return [x * g for x, g in zip(signals, gains)], gains


def stft(audio, size):
    f, t, z = signal.stft(
        audio,
        RATE,
        window="hann",
        nperseg=size,
        noverlap=size * 3 // 4,
        boundary="zeros",
        padded=True,
    )
    return f, t, np.abs(z)


def metrics(reference, candidate):
    results = {}
    for size in (512, 2048):
        _, _, a = stft(reference, size)
        _, _, b = stft(candidate, size)
        peak = max(float(a.max()), 1e-12)
        floor = peak * 1e-4
        adb = 20 * np.log10(np.maximum(a, floor))
        bdb = 20 * np.log10(np.maximum(b, floor))
        mask = np.maximum(a, b) >= peak * 0.001
        results[str(size)] = {
            "spectral_convergence": float(
                np.linalg.norm(b - a) / max(np.linalg.norm(a), 1e-12)
            ),
            "log_magnitude_mae_db": float(np.mean(np.abs(bdb[mask] - adb[mask]))),
        }

    def envelope(x):
        return np.sqrt(np.pad(x * x, (0, (-len(x)) % 48)).reshape(-1, 48).mean(axis=1))

    a, b = envelope(reference), envelope(candidate)
    results["envelope_relative_error"] = float(
        np.linalg.norm(b - a) / max(np.linalg.norm(a), 1e-12)
    )
    return results


def describe(audio, seconds):
    f, t, magnitude = stft(audio, 512)
    # Resample display columns only. Scoring retains the original STFT resolution.
    times = np.linspace(0, seconds, min(500, len(t)))
    frequency_grid = np.array(
        [np.interp(FREQUENCIES, f, column) for column in magnitude.T]
    ).T
    grid = np.array([np.interp(times, t, row) for row in frequency_grid])
    early = audio[: min(len(audio), 4800)]
    envelope = np.sqrt(
        np.pad(early * early, (0, (-len(early)) % 48)).reshape(-1, 48).mean(axis=1)
    )
    spectra = []
    for start, end in [(0, 0.03), (0.03, 0.1), (0.1, 0.3)]:
        segment = audio[round(start * RATE) : round(end * RATE)]
        spectrum = np.abs(
            np.fft.rfft(segment * np.hanning(len(segment)), n=16384)
        ) / max(len(segment), 1)
        spectra.append(
            np.interp(FREQUENCIES, np.fft.rfftfreq(16384, 1 / RATE), spectrum).tolist()
        )
    band_curves = []
    for low, high in BANDS:
        filtered = signal.sosfilt(
            signal.butter(3, [low, high], btype="bandpass", fs=RATE, output="sos"),
            audio,
        )
        # Fixed 5 ms RMS preserves band development and exposes long upper wash.
        power = (
            np.pad(filtered * filtered, (0, (-len(audio)) % 240))
            .reshape(-1, 240)
            .mean(axis=1)
        )
        curve = np.sqrt(power)
        band_curves.append(
            np.interp(times, np.arange(len(curve)) * 0.005 + 0.0025, curve).tolist()
        )
    energy = np.cumsum(audio * audio)
    return {
        "times": times.tolist(),
        "grid": grid.tolist(),
        "envelope": envelope.tolist(),
        "spectra": spectra,
        "bands": band_curves,
        "energy90_ms": float(np.searchsorted(energy, energy[-1] * 0.9) / RATE * 1000),
        "peak_ms": float(np.argmax(envelope) + 0.5),
    }


def audio_uri(audio):
    output = io.BytesIO()
    wavfile.write(output, RATE, np.round(audio * 32767).astype(np.int16))
    return "data:audio/wav;base64," + base64.b64encode(output.getvalue()).decode()


def report(reference, candidates, seconds, output):
    entries, signals = [], []
    for label, path in [("Reference", reference), *candidates]:
        audio, source = read(path)
        aligned, onset = align(audio, seconds)
        signals.append(aligned)
        entries.append({"label": label, "source": source, "onset_frame_48k": onset})
    matched, gains = match_levels(signals)
    for i, (entry, audio, gain) in enumerate(zip(entries, matched, gains)):
        entry.update(
            gain_db=20 * math.log10(gain),
            analysis=describe(audio, seconds),
            audio=audio_uri(audio),
        )
        if i:
            entry["metrics"] = metrics(matched[0], audio)
    payload = {
        "algorithm": VERSION,
        "sample_rate": RATE,
        "seconds": seconds,
        "environment": {"numpy": np.__version__, "scipy": scipy.__version__},
        "configuration": {
            "onset_peak_fraction": 0.02,
            "preroll_frames": 96,
            "active_rms_window_ms": 10,
            "active_power_floor_db": -30,
            "analysis_windows": [512, 2048],
            "hop_fraction": 0.25,
            "spectrogram_window": 512,
            "band_rms_ms": 5,
            "spectrum_windows_ms": [[0, 30], [30, 100], [100, 300]],
        },
        "frequencies": FREQUENCIES.tolist(),
        "bands": BANDS,
        "entries": entries,
    }
    template = Path(__file__).with_name("report.html").read_text()
    encoded = json.dumps(payload, separators=(",", ":"), allow_nan=False).replace(
        "<", "\\u003c"
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(template.replace("__REPORT_DATA__", encoded))
    for entry in entries:
        del entry["audio"]
    output.with_suffix(".json").write_text(
        json.dumps(payload, indent=2, allow_nan=False) + "\n"
    )
    return payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument(
        "--candidate", action="append", required=True, metavar="LABEL=PATH"
    )
    parser.add_argument(
        "--seconds",
        type=float,
        default=1,
        help="Aligned analysis/listening duration, 0.3–30 seconds",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 0.3 <= args.seconds <= 30:
        parser.error("--seconds must be between 0.3 and 30")
    if args.output.suffix.lower() != ".html":
        parser.error("--output must end in .html")
    candidates = []
    for value in args.candidate:
        label, separator, path = value.partition("=")
        if not separator or not label or not path:
            parser.error("--candidate requires LABEL=PATH")
        candidates.append((label, Path(path)))
    inputs = [args.reference, *(p for _, p in candidates)]
    if any(
        p.resolve()
        in [args.output.resolve(), args.output.with_suffix(".json").resolve()]
        for p in inputs
    ):
        parser.error("Output cannot replace an input")
    try:
        report(args.reference, candidates, args.seconds, args.output)
    except (ValueError, OSError) as error:
        parser.exit(1, f"{error}\n")
    print(args.output)


if __name__ == "__main__":
    main()
