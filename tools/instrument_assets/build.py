#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Regenerate procedural multisamples and audition Projects in examples/instruments."""

import argparse
import hashlib
import io
import json
from pathlib import Path

import numpy as np
from materials import RATE, VERSION, make, treatment
from scipy.io import wavfile

ROOT = Path(__file__).resolve().parents[2] / "examples/instruments"
ROOTS = [36, 43, 48, 55, 60, 67, 72, 79]


def note(midi):
    return ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")[
        midi % 12
    ] + str(midi // 12 - 1)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def encode(audio):
    if not np.all(np.isfinite(audio)) or np.max(np.abs(audio)) >= 1:
        raise ValueError("Non-finite or clipped generated audio")
    # Keep the entire audible decay, then 50 ms of settling time. The generator
    # already fades the maximum horizon; this only removes the inaudible suffix.
    audible = np.flatnonzero(np.abs(audio) > 1 / 32768)
    end = min(len(audio), int(audible[-1]) + 2401) if len(audible) else len(audio)
    audio = audio[:end].copy()
    fade = min(480, len(audio))
    audio[-fade:] *= np.linspace(1, 0, fade)
    output = io.BytesIO()
    wavfile.write(output, RATE, np.round(audio * 32767).astype(np.int16))
    return output.getvalue(), len(audio)


def bank(entry, folder):
    recipe_bytes = (folder / "recipe.json").read_bytes()
    recipe = json.loads(recipe_bytes)
    roots = ROOTS if entry["pitched"] else [60]
    media = folder / "media"
    media.mkdir(exist_ok=True)
    assets, zones = [], []
    code_hash = sha(
        Path(__file__).read_bytes()
        + Path(__file__).with_name("materials.py").read_bytes()
    )
    for k, root in enumerate(roots):
        lower = 24 if k == 0 else (roots[k - 1] + root) // 2 + 1
        upper = 91 if k == len(roots) - 1 else (root + roots[k + 1]) // 2
        if not entry["pitched"]:
            lower, upper = 0, 127
        for rr in range(1 if entry["pitched"] else 2):
            seed = recipe["seed_base"] + root * 10 + rr
            hard = make(recipe, root, 0.92, seed)
            scale = 0.76 / np.max(np.abs(hard))
            for layer, velocity, low, high in [
                ("soft", 0.48, 0, 0.65),
                ("hard", 0.92, 0.65, 1),
            ]:
                dry = hard if layer == "hard" else make(recipe, root, velocity, seed)
                dry = dry * scale * (1 if layer == "hard" else 0.65)
                audio = treatment(dry, recipe.get("treatment"))
                if recipe.get("treatment") in ("dub", "room"):
                    audio *= 0.7
                data, frames = encode(audio)
                aid = f"{entry['id']}-{root}-{layer}-{rr}"
                path = f"media/{root}-{layer}-{rr}.wav"
                destination = folder / path
                if destination.exists() and destination.read_bytes() != data:
                    raise ValueError(
                        f"Refusing to replace changed media: {destination}; generate to a new --output directory"
                    )
                destination.write_bytes(data)
                assets.append(
                    {
                        "id": aid,
                        "path": path,
                        "sha256": sha(data),
                        "frames": frames,
                        "sample_rate": RATE,
                        "root_midi": root,
                        "velocity": velocity,
                        "round_robin": rr,
                        "seed": seed,
                        "normalization_gain": float(scale),
                    }
                )
                zones.append(
                    f"      zone z{root}-{layer}-{rr} asset {aid} keys {note(lower)} {note(upper)} velocity {low} {high} root {note(root)} layer body articulation default round_robin rr{root} {rr * 2 + int(layer == 'hard')} region 0 {frames} gain 0db pan 0"
                )
    (folder / "assets.json").write_text(
        json.dumps(
            {
                "algorithm": VERSION,
                "generator_sha256": code_hash,
                "recipe_sha256": sha(recipe_bytes),
                "format": "mono PCM16",
                "assets": assets,
            },
            indent=2,
        )
        + "\n"
    )
    module = f'''crump-module 2
instrument {entry["id"]} "{entry["name"]}" graph {{
  polyphony 16
  parameter level "Level" ratio 0 1 0.2 0.2
  parameter tune "Tune" semitones -12st 12st 0st 0st
  parameter pan "Pan" pan -1 1 0 0
  parameter touch "Touch" ratio 0 1 0.5 0.5
  faceplate {{
    control level "Level" "Output level" maps level
    control tune "Tune" "Repitch the instrument" maps tune
    control pan "Pan" "Stereo position" maps pan
    control touch "Touch" "Velocity amplitude response" maps touch
  }}
  graph voice 8 {{
    node source multisample {{
      playback one_shot
      tune $tune
      rate 1
      velocity_amount $touch
      level $level
      pan $pan
{chr(10).join(zones)}
    }}
    node out output
    bus pitch note.pitch -> source.pitch
    bus gate note.gate -> source.gate
    bus velocity note.velocity -> source.velocity
    bus output source.audio -> out.audio
  }}
}}
'''
    (folder / "instrument.crump").write_text(module)
    return assets


def project(entry, assets):
    base = 36 if entry["category"] == "Bass" else 60
    events = [(0, base, 0.25, 0.92), (4, base, 2, 0.48)]
    if entry["pitched"]:
        events += [
            (8, base - 12, 0.5, 0.85),
            (10, base + 7, 0.25, 0.65),
            (12, base + 12, 0.75, 0.8),
        ]
        if entry["category"] in ("Keys", "Resampling"):
            events += [(16, n, 0.5, 0.65) for n in [48, 55, 58, 63]]
        else:
            events += [
                (16 + i * 0.5, base + n, 0.2, 0.75)
                for i, n in enumerate([0, 7, 10, 3, 7, 0])
            ]
    else:
        events += [
            (8 + i * 0.5, base, 0.2, [0.8, 0.4, 0.6, 0.45][i % 4]) for i in range(12)
        ]
    notes = "\n".join(
        f"      note {at:g}beat {note(midi)} {length:g}beat {vel:g}"
        for at, midi, length, vel in events
    )
    declarations = "\n".join(
        f'  asset {a["id"]} "{entry["name"]}" wav "{a["path"]}" sha256 "{a["sha256"]}" {{\n    usage multisample\n  }}'
        for a in assets
    )
    return f'''crump 2
import "instrument.crump"
project "{entry["name"]}" {{
  id example-{entry["id"]}
  seed {entry["seed"]}
  sample_rate 48000
  render_tail 2s
{declarations}
  song {{
    tempo 120bpm
    meter 4/4
    end 8bar
    pattern phrase 8bar {{
{notes}
    }}
    track instrument "{entry["name"]}" instrument {entry["id"]} level 0db pan 0 {{
      clip phrase pattern phrase start 0bar duration 8bar mode once
    }}
  }}
}}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", nargs="+", help="Instrument IDs to generate")
    parser.add_argument("--output", type=Path, default=ROOT)
    args = parser.parse_args()
    entries = json.loads((ROOT / "catalog.json").read_text())
    if args.only and not set(args.only) <= {e["id"] for e in entries}:
        parser.error("Unknown instrument ID")
    for entry in entries:
        if args.only and entry["id"] not in args.only:
            continue
        folder = args.output / entry["id"]
        folder.mkdir(parents=True, exist_ok=True)
        origin = ROOT / entry["id"]
        if folder.resolve() != origin.resolve():
            for filename in ["recipe.json", "instrument.crump"]:
                if (origin / filename).exists():
                    (folder / filename).write_bytes((origin / filename).read_bytes())
        assets = bank(entry, folder) if entry["kind"] == "multisample" else []
        (folder / "project.crump").write_text(project(entry, assets))
        print(entry["id"], len(assets), "assets", flush=True)


if __name__ == "__main__":
    main()
