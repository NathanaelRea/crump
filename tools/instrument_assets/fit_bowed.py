#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2", "soundfile==0.13.1"]
# ///
"""Fit and score the bowed-string body response against reference recordings.

References are University of Iowa MIS arco recordings with their original file
names, for example `Violin.arco.mf.sulD.D4B4.mono.aif`. They are analysis input
only and are not distributed. Each chromatic run is split at its silences; a
note's pitch is detected from the audio and must fall inside the file's labelled
range.

The body response is the median difference between model and reference partial
levels at each 1/24-octave frequency, pooled over notes and dynamics. Even MIDI
notes fit it; odd notes score it, so the reported error is on held-out notes.
"""

import argparse
import json
import re
import sys
from pathlib import Path

import numpy as np
import soundfile
from scipy import signal

sys.path.insert(0, str(Path(__file__).resolve().parent))
import materials

RATE = materials.RATE
LIBRARY = Path(__file__).resolve().parents[2] / "examples/instruments"
INSTRUMENTS = {
    "Violin": "solo-violin",
    "Cello": "cello",
    "Bass": "contrabass",
}
DYNAMICS = {"pp": 0.48, "mf": 0.7, "ff": 0.92}
STEPS = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
THIRDS = 63 * 2 ** (np.arange(27) / 3)
EQ_HZ = 60 * 2 ** (np.arange(192) / 24)


def midi_of(name):
    letter, flat, octave = re.fullmatch(r"([A-G])(b?)(\d)", name).groups()
    return STEPS[letter] - (1 if flat else 0) + (int(octave) + 1) * 12


def notes(x):
    """Half-open frame ranges of notes longer than 1.2 s, split at -32 dB silences."""
    hop = 480
    level = np.array(
        [np.sqrt(np.mean(x[i : i + hop] ** 2)) for i in range(0, len(x) - hop, hop)]
    )
    sounding = 20 * np.log10(level / level.max() + 1e-12) > -32
    edges = np.flatnonzero(np.diff(sounding.astype(int)))
    starts, ends = edges[::2] + 1, edges[1::2] + 1
    if sounding[0]:
        starts = np.r_[0, starts]
    return [
        (s * hop, e * hop) for s, e in zip(starts, ends) if (e - s) * hop > 1.2 * RATE
    ]


def detect_pitch(note, low, high):
    """Harmonic product spectrum refined by autocorrelation; None when unsure."""
    window = note[int(0.3 * RATE) : int(1.3 * RATE)]
    spectrum = np.abs(np.fft.rfft(window * np.hanning(len(window)), 8 * len(window)))
    freqs = np.fft.rfftfreq(8 * len(window), 1 / RATE)
    product = np.log(spectrum + 1e-9)
    for h in (2, 3, 4, 5):
        product[: len(product) // h] += np.log(
            spectrum[::h][: len(product) // h] + 1e-9
        )
    band = (freqs >= materials.hz(low) * 0.94) & (freqs <= materials.hz(high) * 1.06)
    f0 = materials.fundamental(window, freqs[band][np.argmax(product[band])])
    if not np.isfinite(f0) or f0 <= 0:
        return None
    midi = round(69 + 12 * np.log2(f0 / 440))
    return midi if abs(1200 * np.log2(f0 / materials.hz(midi))) < 35 else None


def features(x, f0):
    """Partial levels, third-octave levels and steady-state behaviour of one note."""
    steady = x[int(0.35 * RATE) : min(len(x) - int(0.25 * RATE), int(2.4 * RATE))]
    spectrum = np.abs(np.fft.rfft(steady * np.hanning(len(steady)))) + 1e-12
    freqs = np.fft.rfftfreq(len(steady), 1 / RATE)
    db = 20 * np.log10(spectrum)
    partials = [
        (k * f0, db[np.abs(freqs - k * f0) < max(0.03 * k * f0, 6)].max())
        for k in range(1, 81)
        if k * f0 <= 16000
    ]
    hz, levels = np.array(partials).T
    power = spectrum**2
    thirds = [
        power[(freqs >= c / 2 ** (1 / 6)) & (freqs < c * 2 ** (1 / 6))].sum()
        for c in THIRDS
    ]
    noise = []
    for k in range(max(1, int(5000 / f0)), int(10000 / f0)):
        peak = db[np.abs(freqs - k * f0) < 0.03 * k * f0].max()
        noise.append(peak - np.median(db[np.abs(freqs - (k + 0.5) * f0) < 0.15 * f0]))
    _, _, frames = signal.stft(steady, RATE, nperseg=4096, noverlap=3584)
    level = 20 * np.log10(np.sqrt(np.mean(np.abs(frames) ** 2, axis=0)) + 1e-12)
    return {
        "hz": hz,
        "partials": levels - 10 * np.log10(np.sum(10 ** (levels / 10))),
        "thirds": 10 * np.log10(np.array(thirds) + 1e-20) - 10 * np.log10(power.sum()),
        "centroid": float(np.sum(freqs * power) / np.sum(power)),
        "harmonic_to_noise": float(np.median(noise)) if noise else np.nan,
        "level_flux": float(np.std(level[2:-2])),
    }


def references(directory):
    out = []
    for path in sorted(directory.rglob("*.aif")):
        parts = path.name.split(".")
        instrument = INSTRUMENTS.get(parts[0])
        dynamic = next((p for p in parts if p in DYNAMICS), None)
        ranges = re.findall(r"([A-G]b?\d)", ".".join(parts[2:]))
        if not instrument or not dynamic or not ranges:
            continue
        low, high = midi_of(ranges[-2 if len(ranges) > 1 else -1]), midi_of(ranges[-1])
        x, rate = soundfile.read(path, always_2d=True)
        x = signal.resample_poly(x.mean(axis=1), RATE, rate)
        recipe = materials.load_recipe(LIBRARY / instrument / "recipe.json")[0]
        for start, end in notes(x):
            midi = detect_pitch(x[start:end], *recipe["range"])
            if midi is not None and low - 1 <= midi <= high + 1:
                out.append(
                    {
                        "instrument": instrument,
                        "midi": midi,
                        "velocity": DYNAMICS[dynamic],
                        "features": features(x[start:end], materials.hz(midi)),
                    }
                )
    return out


def pairs(recipe, refs, keep):
    selected = [
        r
        for r in refs
        if keep(r["midi"]) and recipe["range"][0] <= r["midi"] <= recipe["range"][1]
    ]
    rendered = []
    for r in selected:
        audio = materials.make(
            dict(recipe, seconds=3.0),
            r["midi"],
            r["velocity"],
            recipe["seed_base"] + r["midi"] * 10,
        )
        rendered.append((r["features"], features(audio, materials.hz(r["midi"]))))
    return rendered


def body_correction(matched, smoothing):
    """dB to add at each EQ_HZ point: minus the median model excess there."""
    excess = [[] for _ in EQ_HZ]
    for ref, model in matched:
        count = min(len(ref["partials"]), len(model["partials"]))
        for hz, d in zip(
            ref["hz"][:count], model["partials"][:count] - ref["partials"][:count]
        ):
            excess[int(np.argmin(np.abs(np.log(EQ_HZ / hz))))].append(d)
    median = np.array([np.median(d) if len(d) >= 3 else np.nan for d in excess])
    known = np.isfinite(median)
    median = np.interp(np.log(EQ_HZ), np.log(EQ_HZ[known]), median[known])
    padded = np.pad(median, smoothing // 2, mode="edge")
    return -np.clip(
        np.convolve(padded, np.ones(smoothing) / smoothing, "valid"), -40, 40
    )


def describe(label, matched):
    band = (THIRDS > 80) & (THIRDS < 12000)
    error = np.mean(
        [np.mean(np.abs((m["thirds"] - r["thirds"])[band])) for r, m in matched]
    )
    facts = " ".join(
        f"{key} {np.nanmedian([r[key] for r, _ in matched]):.1f}/{np.nanmedian([m[key] for _, m in matched]):.1f}"
        for key in ("centroid", "harmonic_to_noise", "level_flux")
    )
    print(f"  {label}: band error {error:.2f} dB; reference/model {facts}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--instrument", nargs="+", default=list(INSTRUMENTS.values()))
    parser.add_argument("--smoothing", type=int, default=3, help="1/24-octave bins")
    parser.add_argument("--passes", type=int, default=3)
    parser.add_argument(
        "--limit", type=float, default=20, help="Largest correction in dB, either way"
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="Replace each instrument's shared body curve with its fit",
    )
    args = parser.parse_args()
    refs = references(args.references)
    for instrument in args.instrument:
        recipe = materials.load_recipe(LIBRARY / instrument / "recipe.json")[0]
        mine = [r for r in refs if r["instrument"] == instrument]
        print(f"{instrument}: {len(mine)} reference notes", flush=True)
        describe("held-out, current recipe", pairs(recipe, mine, lambda m: m % 2))
        recipe["params"].pop("eq", None)
        correction = np.zeros(len(EQ_HZ))
        for _ in range(args.passes):
            recipe["params"]["eq"] = np.column_stack([EQ_HZ, correction]).tolist()
            fit = pairs(recipe, mine, lambda m: m % 2 == 0)
            correction = correction + body_correction(fit, args.smoothing)
            # A real body swings about +/-20 dB. Larger corrections would hide a
            # model fault (such as double slip) rather than describe a body, and
            # the curve stays level-neutral between 300 Hz and 3 kHz.
            correction = np.clip(correction, -args.limit, args.limit)
            correction -= correction[(EQ_HZ >= 300) & (EQ_HZ <= 3000)].mean()
        recipe["params"]["eq"] = [
            [round(float(h), 1), round(float(d), 2)] for h, d in zip(EQ_HZ, correction)
        ]
        describe("held-out, fitted body", pairs(recipe, mine, lambda m: m % 2))
        if args.write:
            authored = json.loads((LIBRARY / instrument / "recipe.json").read_text())
            body = materials.SHARED / "bodies" / f"{authored['params']['eq']}.json"
            points = ",\n".join(
                f"  {json.dumps(point)}" for point in recipe["params"]["eq"]
            )
            body.write_text(f"[\n{points}\n]\n")
            print(f"  wrote {body.relative_to(materials.SHARED.parent)}", flush=True)


if __name__ == "__main__":
    main()
