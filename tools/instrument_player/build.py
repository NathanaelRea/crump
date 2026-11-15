#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Prepare the browser instrument player and optionally serve it.

Every instrument is a native graph, so each is rendered through Crump, one
note at a time, and sliced into a short preview bank. Everything generated
goes to renders/instrument-player.
"""

import argparse
import functools
import hashlib
import http.server
import json
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
from scipy.io import wavfile

REPO = Path(__file__).resolve().parents[2]
LIBRARY = REPO / "examples/instruments"
OUTPUT = REPO / "renders/instrument-player"
NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
# Native acoustic previews: roots every half octave, two velocity layers.
ACOUSTIC_ROOTS = list(range(24, 103, 6))
ACOUSTIC_VELOCITIES = [0.48, 0.92]
SPACING_BEATS = 12  # 6 s at 120 BPM: enough for every acoustic body to decay.
REFERENCES = REPO / "renders/instrument-references/report.json"
# Native synths are rendered as held notes. Sustaining patches loop a slice of
# the hold and fade on Note Off; the rest play their whole hold as one shot.
SYNTH_HOLD, SYNTH_LOOP = 3.0, (1.6, 2.85, 0.1)
SYNTH_PREVIEW = {
    "noise-riser": (7.0, False),
    "fm-bell": (5.0, False),
    "fm-tines": (4.0, False),
    "log-drum-bass": (2.0, False),
    "rave-stab": (2.0, False),
}


def note_name(midi):
    return f"{NAMES[midi % 12]}{midi // 12 - 1}"


def midi(name):
    match = re.fullmatch(r"([A-G])([#b]?)(-?\d+)", name)
    if not match:
        raise ValueError(f"Unsupported pitch: {name}")
    letter, accidental, octave = match.groups()
    offset = {"#": 1, "b": -1, "": 0}[accidental]
    return NAMES.index(letter) + offset + (int(octave) + 1) * 12


def parameters(source):
    """Current value of each Instrument Parameter, without its unit suffix."""
    values = {}
    for key, value in re.findall(
        r"^\s*parameter (\S+) \"[^\"]*\" \S+ \S+ \S+ \S+ (\S+)", source, re.MULTILINE
    ):
        values[key] = float(re.sub(r"[a-z]+$", "", value))
    return values


def phrase(folder):
    source = (folder / "project.crump").read_text()
    tempo = float(re.search(r"tempo (\S+)bpm", source).group(1))
    notes = [
        [float(at), midi(pitch), float(length), float(velocity)]
        for at, pitch, length, velocity in re.findall(
            r"^\s*note (\S+)beat (\S+) (\S+)beat (\S+)$", source, re.MULTILINE
        )
    ]
    return {"tempo": tempo, "notes": notes}


def seconds(value):
    """A Parameter value such as `350ms` or `1.5s`, in seconds."""
    number = float(re.sub(r"[a-z]+$", "", value))
    return number / 1000 if value.endswith("ms") else number


def render_acoustic(entry, binary):
    """Render isolated notes through Crump and slice them into a preview bank."""
    # Unpitched previews play each stroke twice, as a round robin. A split
    # instrument has a stroke for each key range; the rest have one at C4.
    strokes = sorted(entry.get("strokes", {}).values()) or [60]
    roots = ACOUSTIC_ROOTS if entry["pitched"] else [k for k in strokes for _ in (0, 1)]
    # Synths and sustained acoustic instruments (winds, bowed strings, voices,
    # organs) are held; struck and plucked ones are short notes left to ring.
    held = "audition" in entry or entry.get("sustained", False)
    hold, looped = SYNTH_PREVIEW.get(entry["id"], (SYNTH_HOLD, True)) if held else (0.25, False)
    spacing = SPACING_BEATS + 2 * round(hold) if held else SPACING_BEATS
    span = entry["audition"]["range"] if "audition" in entry else entry.get("range")
    if span and entry["pitched"]:
        low, high = span
        roots = [r for r in ACOUSTIC_ROOTS if low - 6 < r < high + 6]
    notes = [(r, v) for r in roots for v in ACOUSTIC_VELOCITIES]
    work = OUTPUT / "work" / entry["id"]
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    shutil.copyfile(
        LIBRARY / entry["id"] / "instrument.crump", work / "instrument.crump"
    )
    bars = len(notes) * spacing // 4
    events = "\n".join(
        f"      note {k * spacing}beat {note_name(r)} {2 * hold:g}beat {v:g}"
        for k, (r, v) in enumerate(notes)
    )
    (work / "project.crump").write_text(f'''crump 2
import "instrument.crump"
project "{entry["name"]} preview" {{
  id player-{entry["id"]}
  seed {entry["seed"]}
  sample_rate 48000
  render_tail 6s
  song {{
    tempo 120bpm
    meter 4/4
    end {bars}bar
    pattern notes {bars}bar {{
{events}
    }}
    track preview "{entry["name"]}" instrument {entry["id"]} level 0db pan 0 {{
      clip notes pattern notes start 0bar duration {bars}bar mode once
    }}
  }}
}}
''')
    for args in [
        ["publish", "--origin", "agent", "--actor", "instrument-player"]
        + ["--summary", "Preview render", "--explanation", "Player preview bank"],
        ["render", "--output", str(work / "render")],
    ]:
        result = subprocess.run(
            [str(binary), "--project", str(work), "--json", *args],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode or not json.loads(result.stdout).get("ok"):
            raise RuntimeError(f"{entry['id']} {args[0]}: {result.stdout[:600]}")
    rate, audio = wavfile.read(work / "render/master.wav")
    span = spacing * rate // 2
    slices = [
        audio[k * span : (k + 1) * span].astype(np.float64) for k in range(len(notes))
    ]
    peak = max(np.max(np.abs(s)) for s in slices)
    # Store PCM16 near full scale; `gain` restores the exact rendered level.
    scale = 0.9 / peak
    destination = OUTPUT / entry["id"]
    shutil.rmtree(destination, ignore_errors=True)
    destination.mkdir(parents=True)
    zones = []
    for k, ((root, velocity), audio) in enumerate(zip(notes, slices)):
        layer = "hard" if velocity > 0.65 else "soft"
        # Unpitched previews repeat each layer as a two-way round robin.
        name = f"{root}-{layer}.wav" if entry["pitched"] else f"{root}-{layer}-{k // 2 % 2}.wav"
        if np.array_equal(audio[:, 0], audio[:, 1]):
            audio = audio[:, 0]
        level = np.abs(audio).max(axis=1) if audio.ndim > 1 else np.abs(audio)
        audio = (
            audio[: int(np.flatnonzero(level * scale > 1 / 32768)[-1]) + 480] * scale
        )
        fade = np.linspace(1, 0, min(480, len(audio)))
        audio[-len(fade) :] *= fade[:, None] if audio.ndim > 1 else fade
        wavfile.write(
            destination / name, rate, np.round(audio * 32767).astype(np.int16)
        )
        version = hashlib.sha256((destination / name).read_bytes()).hexdigest()[:16]
        keys = sorted(set(roots))
        index = keys.index(root)
        pitched = entry["pitched"]
        last = index == len(keys) - 1
        zones.append(
            {
                "url": f"renders/instrument-player/{entry['id']}/{name}?v={version}",
                "low": 0 if index == 0 else (keys[index - 1] + root) // 2 + 1,
                "high": 127 if last else (root + keys[index + 1]) // 2,
                "velocity": [0.65, 1] if layer == "hard" else [0, 0.65],
                "root": root,
                "group": f"g{root}" if pitched else f"g{root}-{layer}",
                "index": int(layer == "hard") if pitched else k // 2 % 2,
                "gain": 1 / scale,
                "render_velocity": velocity,
                # Native percussion ignores key tracking; its bodies have one pitch.
                "track": pitched,
                "loop": [round(s * rate) for s in SYNTH_LOOP] if looped else None,
            }
        )
    shutil.rmtree(work)
    return zones


def sustain(entry, values, source):
    """Attack and release the player applies on Note Off, or None for one-shots.

    A synth preview already contains its own attack, so only its Release applies.
    A decaying synth preview with no Release control is a one-shot.
    """
    if "audition" in entry or entry.get("sustained"):
        match = re.search(
            r'^\s*parameter release "[^"]*" \S+ \S+ \S+ \S+ (\S+)', source, re.MULTILINE
        )
        # A decaying synth without a Release control rings out like a one-shot.
        if not match and not SYNTH_PREVIEW.get(entry["id"], (0, True))[1]:
            return None
        return {"attack": 0.002, "release": seconds(match.group(1)) if match else 0.1}
    if "attack" in values and "release" in values:
        return {"attack": values["attack"], "release": values["release"]}
    return None


def build(binary, workers, only=None):
    """Render previews for every instrument, or re-render `only` these and keep the rest."""
    entries = json.loads((LIBRARY / "catalog.json").read_text())
    references = json.loads(REFERENCES.read_text()) if REFERENCES.exists() else {"instruments": {}}
    OUTPUT.mkdir(parents=True, exist_ok=True)
    previous = {}
    if only and (OUTPUT / "manifest.json").exists():
        previous = {i["id"]: i["zones"] for i in json.loads((OUTPUT / "manifest.json").read_text())["instruments"]}
    selected = [e for e in entries if not only or e["id"] in only or e["id"] not in previous]
    with ThreadPoolExecutor(max_workers=workers) as pool:
        rendered = dict(zip([e["id"] for e in selected], pool.map(lambda e: render_acoustic(e, binary), selected)))
    rendered = previous | rendered
    shutil.rmtree(OUTPUT / "work", ignore_errors=True)
    instruments = []
    for entry in entries:
        folder = LIBRARY / entry["id"]
        source = (folder / "instrument.crump").read_text()
        instruments.append(
            {
                "id": entry["id"],
                "name": entry["name"],
                "category": entry["category"],
                "pitched": entry["pitched"],
                "source": "Synth graph" if "audition" in entry else "Acoustic graph",
                "model": entry.get("model"),
                "fixed": False,
                "level": 1,
                "touch": 0,
                "sustain": sustain(entry, parameters(source), source),
                "collection": entry.get("collection"),
                "about": entry.get("about"),
                "references": references["instruments"].get(entry["id"], {}).get("clips", []),
                "zones": rendered[entry["id"]],
                "phrase": phrase(folder),
            }
        )
        print(entry["id"], len(instruments[-1]["zones"]), "zones", flush=True)
    (OUTPUT / "manifest.json").write_text(
        json.dumps({"instruments": instruments, "bands": references.get("bands", [])})
        + "\n"
    )


def serve(port):
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=REPO)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    print(
        f"Instrument player: http://127.0.0.1:{port}/tools/instrument_player/",
        flush=True,
    )
    server.serve_forever()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crump", type=Path, default=REPO / "target/release/crump")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument(
        "--serve", action="store_true", help="Serve the player after building"
    )
    parser.add_argument(
        "--serve-only", action="store_true", help="Serve without rebuilding"
    )
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--only", nargs="+", help="Re-render these instruments and keep the other previews")
    args = parser.parse_args()
    if not args.serve_only:
        if not args.crump.is_file():
            parser.error(f"Build Crump first or pass --crump: {args.crump} is missing")
        build(args.crump.resolve(), args.workers, set(args.only) if args.only else None)
    if args.serve or args.serve_only:
        serve(args.port)


if __name__ == "__main__":
    main()
