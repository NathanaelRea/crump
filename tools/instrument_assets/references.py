#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Fetch reference recordings and compare the example instruments against them.

References are listening and analysis aids only. They are downloaded into
renders/instrument-references, never committed or distributed; references.json
records each clip's source, licence and slice. A clip comes from a URL, a
Wikimedia Commons file, a member of a zip archive, or a YouTube video. Fetching
needs ffmpeg, and yt-dlp for YouTube clips.

Comparisons render each Instrument Module through Crump, so build the release
binary first (or pass --crump).

  references.py --fetch [ID ...]     download and slice every clip, or some
  references.py --compare [ID ...]   write the model/reference report the player reads
  references.py --profile ID         measure the harmonic spectral envelope of the
                                     pitched references, as [[hz, db], ...]
"""

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
from scipy import signal
from scipy.io import wavfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LIBRARY = REPO / "examples/instruments"
OUTPUT = REPO / "renders/instrument-references"
LIST = HERE / "references.json"
CATALOG = LIBRARY / "catalog.json"
BINARY = REPO / "target/release/crump"
RATE = 48000
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
    if "youtube" in clip:
        page = f"https://www.youtube.com/watch?v={clip['youtube']}"
        return page, page
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
        if "youtube" in clip:
            # Only the opening minutes; every clip slices well inside them.
            subprocess.run(
                ["yt-dlp", "-q", "--no-warnings", "-f", "bestaudio", "--download-sections", "*0-200",
                 "-o", str(cache) + ".%(ext)s", "--", clip["youtube"]],
                check=True,
            )
            next(cache.parent.glob(cache.name + ".*")).rename(cache)
        else:
            cache.write_bytes(request(url).read())
            time.sleep(3)
    source = cache
    if "member" in clip:
        source = cache.with_name(cache.name + "-" + hashlib.sha256(clip["member"].encode()).hexdigest()[:12])
        with zipfile.ZipFile(cache) as archive:
            source.write_bytes(archive.read(clip["member"]))
    destination = OUTPUT / ident / f"{index}.wav"
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-ss", str(clip["start"]), "-t", str(clip["seconds"]), "-i", str(source),
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


def note_name(midi):
    return ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][midi % 12] + str(midi // 12 - 1)


def render(ident, notes, spacing=8.0):
    """Render (midi, velocity, held seconds) notes of one module through Crump, one array each."""
    with tempfile.TemporaryDirectory() as temporary:
        folder = Path(temporary)
        shutil.copyfile(LIBRARY / ident / "instrument.crump", folder / "instrument.crump")
        beats = spacing * 2
        bars = int(np.ceil(len(notes) * beats / 4))
        events = "\n".join(
            f"      note {k * beats:g}beat {note_name(m)} {h * 2:g}beat {v:g}" for k, (m, v, h) in enumerate(notes)
        )
        (folder / "project.crump").write_text(f"""crump 2
import "instrument.crump"
project "Reference comparison" {{
  id reference-{ident}
  seed 1
  sample_rate 48000
  render_tail 4s
  song {{
    tempo 120bpm
    meter 4/4
    end {bars}bar
    pattern notes {bars}bar {{
{events}
    }}
    track model "Model" instrument {ident} level 0db pan 0 {{
      clip notes pattern notes start 0bar duration {bars}bar mode once
    }}
  }}
}}
""")
        for args in [
            ["publish", "--origin", "agent", "--actor", "references", "--summary", "Comparison", "--explanation", "Reference comparison"],
            ["render", "--output", str(folder / "render")],
        ]:
            result = subprocess.run([str(BINARY), "--project", str(folder), "--json", *args], capture_output=True, text=True)
            if result.returncode or not json.loads(result.stdout).get("ok"):
                raise RuntimeError(f"{ident} {args[0]}: {result.stdout[:600]}")
        audio = wavfile.read(folder / "render/master.wav")[1].astype(np.float64)
    audio = audio.mean(axis=1) if audio.ndim > 1 else audio
    span = int(spacing * RATE)
    return [audio[k * span : (k + 1) * span] for k in range(len(notes))]


def compare(ident):
    clips = json.loads(LIST.read_text())[ident]
    entry = next(e for e in json.loads(CATALOG.read_text()) if e["id"] == ident)
    pending = []
    for index, clip in enumerate(clips):
        path = OUTPUT / ident / f"{index}.wav"
        if not path.exists():
            continue
        x = wavfile.read(path)[1].astype(np.float64) / 32768
        hz = clip.get("hz") or (pitch(x) if entry["pitched"] else None)
        if entry["pitched"]:
            low, high = entry.get("range", [24, 91])
            detected = round(69 + 12 * np.log2(hz / 440)) if hz else None
            # A detection far outside the instrument's range is a failure, not a note.
            if detected is None or not low - 3 <= detected <= high + 3:
                detected, hz = entry.get("audition_note", 60), None
            midi = int(np.clip(detected, low, high))
        else:
            # A split instrument plays each stroke on its own keys.
            strokes = entry.get("strokes", {})
            midi = strokes.get(clip.get("stroke"), next(iter(strokes.values()), 60))
        pending.append((index, x, hz, midi))
    held = 1.5 if entry.get("sustained") else 0.25
    models = render(ident, [(midi, 0.92, held) for _, _, _, midi in pending]) if pending else []
    results = []
    for (index, x, hz, midi), model in zip(pending, models):
        model = model / max(np.max(np.abs(model)), 1e-12) * 0.5
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


def label(clip):
    if "title" in clip:
        return clip["title"]
    if "member" in clip:
        return Path(clip["member"]).name
    return clip.get("commons") or Path(urllib.parse.unquote(clip["url"])).name


def harmonic_profile(ident, count=32):
    """Spectral envelope sampled at the harmonics of every steady, pitched frame.

    Each frame contributes its harmonic levels relative to its total energy,
    at their frequencies, so the result describes the instrument's body rather
    than any one note. Returns `[[hz, db], ...]` on sixth-octave centres.
    """
    frame = int(0.08 * RATE)
    points = []
    for index in range(len(json.loads(LIST.read_text())[ident])):
        path = OUTPUT / ident / f"{index}.wav"
        if not path.exists():
            continue
        x = wavfile.read(path)[1].astype(np.float64) / 32768
        level = envelope(x, 20)
        for at in range(0, len(x) - frame, frame // 2):
            if level[at + frame // 2] < 0.25 * np.max(level):
                continue
            chunk = x[at : at + frame] * np.hanning(frame)
            hz = pitch(np.concatenate([np.zeros(int(0.1 * RATE)), x[at : at + int(0.5 * RATE)]]))
            if not hz or hz < 50:
                continue
            n = 1 << 15
            spectrum = np.abs(np.fft.rfft(chunk, n)) ** 2
            f = np.fft.rfftfreq(n, 1 / RATE)
            harmonics = np.arange(1, count + 1) * hz
            harmonics = harmonics[harmonics < 14000]
            energy = np.array([spectrum[np.abs(f - h) < hz * 0.12].sum() for h in harmonics])
            floor = np.median(spectrum[(f > 40) & (f < harmonics[-1])]) * (hz * 0.24 / (f[1] - f[0]))
            # A harmonic buried in noise, or a wrong pitch's empty bin, says
            # nothing about the body.
            keep = energy > 30 * floor
            if keep.sum() < 3:
                continue
            # Relative to the frame's total, so notes of any pitch agree on the
            # level the body gives each frequency.
            db = 10 * np.log10(energy / energy[keep].sum())
            points += list(zip(harmonics[keep], db[keep]))
    if not points:
        raise ValueError(f"{ident}: no steady pitched frames")
    hz, db = np.asarray(points).T
    centres = BANDS[(BANDS >= hz.min() / 1.06) & (BANDS <= min(hz.max() * 1.06, 14000))]
    curve = []
    for c in centres:
        inside = np.abs(np.log2(hz / c)) <= 1 / 12
        if inside.sum() >= 3:
            curve.append([c, float(np.median(db[inside]))])
    hz, db = np.asarray(curve).T
    # Harmonics of one note leave gaps between them; the local maximum bridges
    # them so the curve follows the body, not the spacing of a few notes.
    db = np.max([np.pad(db, 1, mode="edge")[k : k + len(db)] for k in range(3)], axis=0)
    db = np.convolve(np.pad(db, 1, mode="edge"), [0.25, 0.5, 0.25], mode="valid")
    db -= db.max()
    return [[round(float(h)), round(float(d), 1)] for h, d in zip(hz, db)]


def main():
    global BINARY
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fetch", nargs="*")
    parser.add_argument("--compare", nargs="*")
    parser.add_argument("--profile", nargs="+")
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--crump", type=Path, default=REPO / "target/release/crump")
    args = parser.parse_args()
    BINARY = args.crump.resolve()
    references = json.loads(LIST.read_text())
    if args.fetch is not None:
        pages_path = OUTPUT / "pages.json"
        pages = json.loads(pages_path.read_text()) if pages_path.exists() else {}
        for ident in args.fetch or list(references):
            # Drop slices left over from an earlier, longer clip list.
            for stale in (OUTPUT / ident).glob("*.wav"):
                if int(stale.stem) >= len(references[ident]):
                    stale.unlink()
            for index, clip in enumerate(references[ident]):
                try:
                    pages[f"{ident}/{index}"] = fetch(ident, index, clip)["page"]
                    print("fetched", ident, index, flush=True)
                except Exception as error:  # A missing source must not stop the rest.
                    print("FAILED", ident, index, error, flush=True)
        pages_path.write_text(json.dumps(pages, indent=2) + "\n")
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
                            "label": label(clip),
                            "page": pages.get(f"{ident}/{result['index']}", clip.get("page")),
                            "license": clip["license"],
                        }
                    )
                report["instruments"][ident] = {"clips": clips}
                summary = ", ".join(f"{c['spectral_distance_db']} dB" for c in clips)
                print(f"{ident:18} {summary}", flush=True)
        report["bands"] = np.round(BANDS, 1).tolist()
        report_path.write_text(json.dumps(report) + "\n")
    if args.profile:
        for ident in args.profile:
            print(ident, json.dumps(harmonic_profile(ident)))


if __name__ == "__main__":
    main()
