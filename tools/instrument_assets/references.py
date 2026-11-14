#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Fetch reference recordings and compare the example instruments against them.

References are listening and analysis aids only. They are downloaded into
renders/instrument-references, never committed or distributed; references.json
records each clip's source, licence and slice. Fetching needs ffmpeg.

  references.py --fetch              download and slice every clip
  references.py --compare [ID ...]   write the model/reference report the player reads
  references.py --fit ID [--write]   fit a smooth reference_eq from the comparison
"""

import argparse
import hashlib
import json
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
from materials import RATE, load_recipe, make, treatment
from scipy import signal
from scipy.io import wavfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LIBRARY = REPO / "examples/instruments"
OUTPUT = REPO / "renders/instrument-references"
LIST = HERE / "references.json"
COMMONS = "https://commons.wikimedia.org/w/api.php"
AGENT = {"User-Agent": "crump-instrument-references/1 (https://github.com/NathanaelRea/crump)"}
# Sixth-octave bands from 50 Hz to 16 kHz.
BANDS = 50 * 2 ** (np.arange(0, 51) / 6)


def request(url, attempts=6):
    """GET with a polite retry: Wikimedia answers bursts with 429 and Retry-After."""
    for attempt in range(attempts):
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=60)
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == attempts - 1:
                raise
            time.sleep(float(error.headers.get("Retry-After") or 20 * (attempt + 1)))


def source_url(clip):
    """Direct download URL and human-readable page for a clip."""
    if "commons" not in clip:
        return urllib.parse.quote(clip["url"], safe=":/"), clip.get("page", clip["url"])
    query = urllib.parse.urlencode(
        {"action": "query", "titles": f"File:{clip['commons']}", "prop": "imageinfo", "iiprop": "url", "format": "json"}
    )
    page = next(iter(json.load(request(f"{COMMONS}?{query}"))["query"]["pages"].values()))
    return page["imageinfo"][0]["url"], page["imageinfo"][0]["descriptionurl"]


def fetch(ident, index, clip):
    """Download (cached) and slice one clip to a 48 kHz mono WAV."""
    url, page = source_url(clip)
    cache = OUTPUT / "cache" / hashlib.sha256(url.encode()).hexdigest()[:24]
    cache.parent.mkdir(parents=True, exist_ok=True)
    if not cache.exists():
        cache.write_bytes(request(url).read())
        time.sleep(3)
    destination = OUTPUT / ident / f"{index}.wav"
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-ss", str(clip["start"]), "-t", str(clip["seconds"]), "-i", str(cache),
         "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le", str(destination)],
        check=True,
    )
    # Peak-normalize so references play at a comparable level.
    rate, audio = wavfile.read(destination)
    audio = audio.astype(np.float64)
    audio *= 0.5 * 32767 / max(np.max(np.abs(audio)), 1)
    wavfile.write(destination, rate, np.round(audio).astype(np.int16))
    return {"page": page, "path": destination}


def envelope(x, ms=10):
    window = max(1, int(RATE * ms / 1000))
    return np.sqrt(np.convolve(x * x, np.ones(window) / window, mode="same"))


def onset(x):
    level = envelope(x, 5)
    return int(np.argmax(level > 0.1 * np.max(level)))


def pitch(x, low=30, high=2500):
    """Median autocorrelation pitch over steady 80 ms frames after the onset, or None."""
    start = onset(x) + int(0.08 * RATE)
    estimates = []
    frame = int(0.08 * RATE)
    for at in range(start, min(len(x) - frame, start + int(1.5 * RATE)), frame // 2):
        chunk = x[at : at + frame] * np.hanning(frame)
        spectrum = np.fft.rfft(chunk, 2 * frame)
        correlation = np.fft.irfft(np.abs(spectrum) ** 2)[:frame]
        lags = np.arange(int(RATE / high), int(RATE / low))
        lag = lags[np.argmax(correlation[lags])]
        if correlation[lag] > 0.5 * correlation[0]:
            # The shortest lag that nearly matches the best one avoids octave errors.
            for candidate in lags[lags < lag]:
                if correlation[candidate] > 0.9 * correlation[lag] and candidate > lag / 2.2:
                    lag = candidate
                    break
            estimates.append(RATE / lag)
    return float(np.median(estimates)) if len(estimates) >= 3 else None


def spectrum(x):
    """Long-term spectrum of the sounding part in sixth-octave bands, dB, level-neutral."""
    start = onset(x)
    level = envelope(x, 20)
    sounding = np.flatnonzero(level[start:] > np.max(level) * 10 ** (-40 / 20))
    end = start + (sounding[-1] if len(sounding) else len(x))
    end = min(end, start + int(3 * RATE))
    f, psd = signal.welch(x[start:end], RATE, nperseg=min(8192, max(256, end - start)))
    db = []
    for low, high in zip(BANDS / 2 ** (1 / 12), BANDS * 2 ** (1 / 12)):
        inside = (f >= low) & (f < high)
        db.append(10 * np.log10(np.mean(psd[inside]) + 1e-20) if inside.any() else np.nan)
    db = np.asarray(db)
    db = np.interp(np.arange(len(db)), np.flatnonzero(np.isfinite(db)), db[np.isfinite(db)])
    # Relative to the loudest band, so kicks and bells compare where their energy is.
    return np.maximum(db - np.max(db), -60)


def facts(x):
    level = envelope(x, 10)
    peak = int(np.argmax(level))
    quiet = np.flatnonzero(level[peak:] < level[peak] * 0.1)
    f, psd = signal.welch(x[onset(x) :], RATE, nperseg=4096)
    return {
        "decay_20db": round(float(quiet[0] / RATE), 3) if len(quiet) else None,
        "attack": round(float((peak - onset(x)) / RATE), 3),
        "centroid": round(float(np.sum(f * psd) / np.sum(psd))),
    }


def nearest_stroke(recipe, clip):
    """The recipe variant a reference compares against: a named stroke, or the recipe."""
    stroke = next((s for s in recipe.get("strokes", []) if s["name"] == clip.get("stroke")), None)
    stroke = stroke or (recipe["strokes"][0] if recipe.get("strokes") else None)
    if stroke:
        return recipe | {"params": recipe["params"] | stroke.get("params", {})}
    return recipe


def model_note(recipe, note):
    x = make(recipe, note, 0.92, recipe["seed_base"] + note * 10)
    x = treatment(x / np.max(np.abs(x)) * 0.5, recipe.get("treatment"))
    return x.mean(axis=0) if x.ndim > 1 else x


def compare(ident):
    clips = json.loads(LIST.read_text())[ident]
    recipe = load_recipe(LIBRARY / ident / "recipe.json")[0]
    results = []
    for index, clip in enumerate(clips):
        path = OUTPUT / ident / f"{index}.wav"
        if not path.exists():
            continue
        x = wavfile.read(path)[1].astype(np.float64) / 32768
        hz = clip.get("hz") or (pitch(x) if recipe["pitched"] else None)
        if recipe["pitched"]:
            low, high = recipe.get("range", [24, 91])
            detected = round(69 + 12 * np.log2(hz / 440)) if hz else None
            # A detection far outside the instrument's range is a failure, not a note.
            if detected is None or not low - 3 <= detected <= high + 3:
                detected, hz = recipe.get("audition", 60), None
            midi = int(np.clip(detected, low, high))
        else:
            midi = 60
        variant = nearest_stroke(recipe, clip)
        model = model_note(variant, midi)
        reference_db, model_db = spectrum(x), spectrum(model)
        # Score only bands either sound actually uses.
        audible = (BANDS >= 40) & (BANDS <= 14000) & (np.maximum(reference_db, model_db) > -40)
        results.append(
            {
                "index": index,
                "note": midi,
                "detected_hz": round(hz, 2) if hz else None,
                "reference_db": np.round(reference_db, 1).tolist(),
                "model_db": np.round(model_db, 1).tolist(),
                "spectral_distance_db": round(float(np.mean(np.abs(reference_db - model_db)[audible])), 1),
                "reference": facts(x),
                "model": facts(model),
            }
        )
    return ident, results


def fit(ident, cap=8.0):
    """Mean smoothed reference/model difference, capped and level-neutral."""
    report = json.loads((OUTPUT / "report.json").read_text())["instruments"][ident]
    differences = [np.asarray(c["reference_db"]) - np.asarray(c["model_db"]) for c in report["clips"]]
    difference = np.mean(differences, axis=0)
    difference = np.convolve(np.pad(difference, 3, mode="edge"), np.ones(7) / 7, mode="valid")
    keep = (BANDS >= 60) & (BANDS <= 14000)
    hz, db = BANDS[keep], np.clip(difference[keep], -cap, cap)
    # Level-neutral where the instrument is loudest.
    weight = 10 ** (np.mean([c["reference_db"] for c in report["clips"]], axis=0)[keep] / 10)
    db -= np.sum(db * weight) / np.sum(weight)
    recipe = json.loads((LIBRARY / ident / "recipe.json").read_text())
    # The comparison already includes any earlier fit, so corrections compound.
    if recipe.get("reference_eq"):
        old_hz, old_db = np.asarray(recipe["reference_eq"]).T
        db += np.interp(np.log(hz), np.log(old_hz), old_db)
    db = np.clip(db, -cap * 1.5, cap * 1.5)
    # Every third band is enough for a smooth curve.
    return [[round(float(h)), round(float(d), 1)] for h, d in zip(hz[::3], db[::3])]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fetch", action="store_true")
    parser.add_argument("--compare", nargs="*")
    parser.add_argument("--fit", nargs="+")
    parser.add_argument("--write", action="store_true", help="With --fit, store the curve in each recipe")
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    references = json.loads(LIST.read_text())
    if args.fetch:
        pages = {}
        for ident, clips in references.items():
            for index, clip in enumerate(clips):
                try:
                    pages[f"{ident}/{index}"] = fetch(ident, index, clip)["page"]
                    print("fetched", ident, index, flush=True)
                except Exception as error:  # A missing source must not stop the rest.
                    print("FAILED", ident, index, error, flush=True)
        (OUTPUT / "pages.json").write_text(json.dumps(pages, indent=2) + "\n")
    if args.compare is not None:
        selected = args.compare or list(references)
        report_path = OUTPUT / "report.json"
        report = json.loads(report_path.read_text()) if report_path.exists() else {"instruments": {}}
        pages = json.loads((OUTPUT / "pages.json").read_text())
        with ProcessPoolExecutor(args.jobs) as pool:
            for ident, results in pool.map(compare, selected):
                clips = []
                for result in results:
                    clip = references[ident][result["index"]]
                    path = OUTPUT / ident / f"{result['index']}.wav"
                    version = hashlib.sha256(path.read_bytes()).hexdigest()[:16]
                    clips.append(
                        result
                        | {
                            "url": f"renders/instrument-references/{ident}/{result['index']}.wav?v={version}",
                            "label": clip.get("commons") or Path(urllib.parse.unquote(clip["url"])).name,
                            "page": pages.get(f"{ident}/{result['index']}", clip.get("page")),
                            "license": clip["license"],
                        }
                    )
                report["instruments"][ident] = {"clips": clips}
                summary = ", ".join(f"{c['spectral_distance_db']} dB" for c in clips)
                print(f"{ident:18} {summary}", flush=True)
        report["bands"] = np.round(BANDS, 1).tolist()
        report_path.write_text(json.dumps(report) + "\n")
    if args.fit:
        for ident in args.fit:
            curve = fit(ident)
            print(ident, json.dumps(curve))
            if args.write:
                path = LIBRARY / ident / "recipe.json"
                recipe = json.loads(path.read_text())
                recipe["reference_eq"] = curve
                path.write_text(json.dumps(recipe, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
