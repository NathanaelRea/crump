#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.3.3", "scipy==1.16.2"]
# ///
"""Regenerate procedural multisamples and audition Projects in examples/instruments.

The example media is not committed. `--setup` generates whatever is missing and
checks every file against the hashes in each `assets.json`, without editing any
source.
"""

import argparse
import hashlib
import io
import json
import os
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
from materials import RATE, SHARED, VERSION, load_recipe, make, treatment
from scipy import signal
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
    # Mono audio is (frames,); stereo is (2, frames) and trims both channels together.
    level = np.max(np.abs(audio), axis=0) if audio.ndim > 1 else np.abs(audio)
    audible = np.flatnonzero(level > 1 / 32768)
    end = min(len(level), int(audible[-1]) + 2401) if len(audible) else len(level)
    audio = audio[..., :end].copy()
    fade = min(480, end)
    audio[..., -fade:] *= np.linspace(1, 0, fade)
    output = io.BytesIO()
    wavfile.write(output, RATE, np.round(audio.T * 32767).astype(np.int16))
    return output.getvalue(), end


def band_limit(audio, root, upper, recipe):
    """Low-pass a sample so its highest key cannot fold partials past Nyquist.

    Playing a key above the root speeds the sample up; a partial pushed above
    24 kHz folds back as a stray tone that wobbles against the vibrato. Only
    recipes with their own `roots` (the orchestral banks) are treated, so older
    banks keep their exact bytes.
    """
    if "roots" not in recipe or upper <= root:
        return audio
    cutoff = min(20000, 0.45 * RATE / 2 ** ((upper - root) / 12))
    return signal.sosfilt(signal.butter(8, cutoff, fs=RATE, output="sos"), audio)


def flatten_loop(audio, recipe):
    """Even out slow level changes across the sustain loop.

    A loop replays its slice forever, so any swell in it repeats on every pass.
    Levels inside the loop are pulled toward their median (at most 6 dB either
    way), easing in and out over 100 ms around the loop region.
    """
    start, end, _ = (round(s * RATE) for s in recipe["loop"])
    end += round(0.025 * RATE)  # The loop end may move this far later.
    mono = audio.mean(axis=0) if audio.ndim > 1 else audio
    # Ensembles beat faster than one player swells, so they may use a shorter window.
    window = round(recipe.get("loop_leveling", 0.12) * RATE)
    level = np.sqrt(np.convolve(mono**2, np.ones(window) / window, mode="same") + 1e-20)
    gain = np.clip(np.median(level[start:end]) / level, 0.5, 2.0)
    frames = np.arange(len(mono))
    ease = round(0.1 * RATE)
    weight = np.clip(
        np.minimum((frames - (start - ease)) / ease, ((end + ease) - frames) / ease),
        0,
        1,
    )
    return audio * (1 + (gain - 1) * weight)


def loop_points(audio, recipe):
    """Sustain loop in frames: (start, end, crossfade).

    The engine crossfades the loop's last `crossfade` frames into its first ones,
    so the end is moved by up to 25 ms (a period of the lowest note) to where
    those two windows correlate best.
    """
    start, target, fade = (round(s * RATE) for s in recipe["loop"])
    audio = audio.mean(axis=0) if audio.ndim > 1 else audio
    search = round(0.025 * RATE)
    reference = audio[start : start + fade]
    window = audio[target - fade - search : target + search]
    correlation = signal.correlate(window, reference, mode="valid")
    energy = np.sqrt(np.convolve(window**2, np.ones(fade), mode="valid"))
    score = correlation / np.maximum(energy * np.linalg.norm(reference), 1e-12)
    return start, target - search + int(np.argmax(score)), fade


def recorded(folder):
    """Media hashes by path from a bank's committed `assets.json`."""
    manifest = json.loads((folder / "assets.json").read_text())
    return {asset["path"]: asset["sha256"] for asset in manifest["assets"]}


def complete(folder):
    return all(
        (folder / path).exists() and sha((folder / path).read_bytes()) == digest
        for path, digest in recorded(folder).items()
    )


def bank(entry, folder, setup=False):
    """Generate one bank. In setup mode, check it against `assets.json` and
    write only the media, leaving the module and its manifest untouched."""
    expected = recorded(folder) if setup else {}
    recipe_bytes = (folder / "recipe.json").read_bytes()
    recipe, shared = load_recipe(folder / "recipe.json")
    roots = recipe.get("roots", ROOTS) if entry["pitched"] else [60]
    low_key, high_key = recipe.get("range", [24, 91])
    fixed = recipe.get("fixed_keys")
    media = folder / "media"
    media.mkdir(exist_ok=True)
    assets, zones = [], []
    code_hash = sha(
        Path(__file__).read_bytes()
        + Path(__file__).with_name("materials.py").read_bytes()
    )
    for k, root in enumerate(roots):
        lower = low_key if k == 0 else (roots[k - 1] + root) // 2 + 1
        upper = high_key if k == len(roots) - 1 else (root + roots[k + 1]) // 2
        if not entry["pitched"]:
            lower, upper = 0, 127
        for rr in range(1 if entry["pitched"] else 2):
            seed = recipe["seed_base"] + root * 10 + rr
            hard = band_limit(make(recipe, root, 0.92, seed), root, upper, recipe)
            scale = 0.76 / np.max(np.abs(hard))
            for layer, velocity, low, high in [
                ("soft", 0.48, 0, 0.65),
                ("hard", 0.92, 0.65, 1),
            ]:
                dry = (
                    hard
                    if layer == "hard"
                    else band_limit(
                        make(recipe, root, velocity, seed), root, upper, recipe
                    )
                )
                dry = dry * scale * (1 if layer == "hard" else 0.65)
                audio = treatment(dry, recipe.get("treatment"))
                if recipe.get("treatment") in ("dub", "room"):
                    audio *= 0.7
                if "loop" in recipe:
                    audio = flatten_loop(audio, recipe)
                loop = loop_points(audio, recipe) if "loop" in recipe else None
                data, frames = encode(audio)
                aid = f"{entry['id']}-{root}-{layer}-{rr}"
                path = f"media/{root}-{layer}-{rr}.wav"
                destination = folder / path
                if setup and sha(data) != expected.get(path):
                    raise ValueError(
                        f"{destination} does not match assets.json; this platform does not reproduce the committed bank"
                    )
                # Setup replaces outdated media: assets.json is the authority.
                if (
                    not setup
                    and destination.exists()
                    and destination.read_bytes() != data
                ):
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
                    | ({"loop": list(loop)} if loop else {})
                )
                if fixed:
                    # Five-key groups rooted at their centre: no key moves more
                    # than two semitones from the recorded pitch. One zone per
                    # key would exceed the engine's per-frame zone-scan budget.
                    zones += [
                        f"      zone z{key + 2}-{layer}-{rr} asset {aid} keys {note(key)} {note(key + 4)} velocity {low} {high} root {note(key + 2)}"
                        for key in range(fixed[0], fixed[1] + 1, 5)
                    ]
                    continue
                suffix = (
                    f" loop {loop[0]} {loop[1]} crossfade {loop[2]}" if loop else ""
                )
                # Each zone plays its whole sample. Zones sharing a key and
                # velocity range are round-robin alternatives, chosen at random.
                zones.append(
                    f"      zone z{root}-{layer}-{rr} asset {aid} keys {note(lower)} {note(upper)} velocity {low} {high} root {note(root)}{suffix}"
                )
    if setup:
        return assets
    manifest = {
        "algorithm": VERSION,
        "generator_sha256": code_hash,
        "recipe_sha256": sha(recipe_bytes),
    }
    if shared:
        manifest["shared_sha256"] = {
            path.relative_to(SHARED).as_posix(): sha(path.read_bytes())
            for path in dict.fromkeys(shared)
        }
    manifest["format"] = (
        f"{'stereo' if recipe['params'].get('stereo') else 'mono'} PCM16"
    )
    manifest["assets"] = assets
    (folder / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n")
    # Sustained tones are long one-shot samples; an amplitude envelope on the
    # sample gain supplies the Note Off release.
    release = recipe.get("release")
    sustain_parameters = sustain_controls = sustain_nodes = sustain_buses = ""
    if release:
        sustain_parameters = f"""
  parameter attack "Attack" seconds 0.001s 2s 0.002s 0.002s
  parameter release "Release" seconds 0.02s 4s {release:g}s {release:g}s"""
        sustain_controls = """
    control attack "Attack" "Fade-in added to the natural onset" maps attack
    control release "Release" "Fade-out after Note Off" maps release"""
        sustain_nodes = """
    node amplitude envelope mode adsr attack $attack decay 0.01s sustain 1 release $release"""
        sustain_buses = """
    bus amplitude amplitude.control -> source.gain
    bus amplitude-gate note.gate -> amplitude.gate"""
    # The module owns its media; paths are relative to the module's folder.
    declarations = "\n".join(
        f'asset {a["id"]} "{entry["name"]}" wav "{a["path"]}" sha256 "{a["sha256"]}" {{\n  usage multisample\n}}'
        for a in assets
    )
    module = f'''crump-module 2
instrument {entry["id"]} "{entry["name"]}" graph {{
  polyphony 16
  parameter level "Level" ratio 0 1 0.2 0.2
  parameter tune "Tune" semitones -12st 12st 0st 0st
  parameter pan "Pan" pan -1 1 0 0
  parameter touch "Touch" ratio 0 1 0.5 0.5{sustain_parameters}
  faceplate {{
    control level "Level" "Output level" maps level
    control tune "Tune" "Repitch the instrument" maps tune
    control pan "Pan" "Stereo position" maps pan
    control touch "Touch" "Velocity amplitude response" maps touch{sustain_controls}
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
    }}{sustain_nodes}
    node out output
    bus pitch note.pitch -> source.pitch
    bus gate note.gate -> source.gate
    bus velocity note.velocity -> source.velocity{sustain_buses}
    bus output source.audio -> out.audio
  }}
}}
{declarations}
'''
    (folder / "instrument.crump").write_text(module)
    return assets


CHORDS = [
    (16, [0, 4, 7, 12]),
    (20, [-3, 0, 4, 9]),
    (24, [-7, 0, 5, 9]),
    (28, [-5, 2, 7, 11]),
]


def orchestral_events(recipe):
    """Audition inside the instrument's playable range, with a phrase per family."""
    base = recipe["audition"]
    phrase = recipe.get("phrase", "sustained" if recipe.get("release") else "struck")
    if phrase == "timpani":
        events = [(0, base, 1, 0.92), (2, base - 7, 1, 0.6), (4, base, 2, 0.48)]
        events += [(8 + i / 8, base, 0.125, 0.3 + 0.6 * i / 31) for i in range(32)]
        events += [(13, base - 7, 0.5, 0.85), (14, base, 0.5, 0.85)]
        events += [
            (16 + i / 2, base + n, 0.4, 0.75)
            for i, n in enumerate([0, 3, 5, 7, 5, 3, 0, -5])
        ]
        events += [
            (24 + i / 8, base - 7, 0.125, 0.85 - 0.5 * i / 23) for i in range(24)
        ]
    elif phrase == "struck":
        events = [(0, base, 0.25, 0.92), (4, base, 2, 0.48), (8, base - 7, 0.5, 0.85)]
        events += [(10, base + 7, 0.25, 0.65), (12, base + 12, 0.75, 0.8)]
        events += [
            (16 + i / 2, base + n, 0.4, 0.75)
            for i, n in enumerate([0, 4, 7, 12, 16, 12, 7, 4])
        ]
        events += [
            (24 + i / 4, base + n, 0.2, 0.55 + 0.05 * (i % 3))
            for i, n in enumerate([0, 2, 4, 5, 7, 9, 11, 12])
        ]
    else:
        events = [
            (0, base, 2, 0.48),
            (2, base + 2, 1, 0.7),
            (3, base + 4, 1, 0.7),
            (4, base + 7, 3, 0.92),
        ]
        events += [
            (8 + i / 2, base + n, 0.35, 0.85) for i, n in enumerate([0, 4, 7, 12])
        ]
        events += [(10, base + 12, 2, 0.6), (12, base - 5, 3.5, 0.75)]
        if phrase in ("ensemble", "keys"):
            events += [(at, base + n, 3.5, 0.62) for at, chord in CHORDS for n in chord]
        else:
            events += [
                (16 + i, base + n, 1, 0.55 + 0.05 * i)
                for i, n in enumerate([0, 2, 4, 5, 7, 9, 11])
            ]
            events += [(24, base + 12, 6, 0.8)]
    low, high = recipe["range"]
    return [e for e in events if low <= e[1] <= high]


def project(entry, recipe=None):
    recipe = recipe or {}
    base = 36 if entry["category"] == "Bass" else 60
    events = [(0, base, 0.25, 0.92), (4, base, 2, 0.48)]
    if "audition" in recipe:
        events = orchestral_events(recipe)
    elif entry["pitched"]:
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
    return f'''crump 2
import "instrument.crump"
project "{entry["name"]}" {{
  id example-{entry["id"]}
  seed {entry["seed"]}
  sample_rate 48000
  render_tail 2s
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


def setup_bank(entry):
    folder = ROOT / entry["id"]
    if complete(folder):
        return f"{entry['id']} present"
    return f"{entry['id']} {len(bank(entry, folder, setup=True))} samples generated"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", nargs="+", help="Instrument IDs to generate")
    parser.add_argument("--output", type=Path, default=ROOT)
    parser.add_argument(
        "--setup",
        action="store_true",
        help="Generate missing example media and check it against assets.json",
    )
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = parser.parse_args()
    entries = json.loads((ROOT / "catalog.json").read_text())
    if args.only and not set(args.only) <= {e["id"] for e in entries}:
        parser.error("Unknown instrument ID")
    if args.setup:
        if args.output != ROOT:
            parser.error("--setup writes the example library; omit --output")
        selected = [
            e
            for e in entries
            if e["kind"] == "multisample" and (not args.only or e["id"] in args.only)
        ]
        with ProcessPoolExecutor(max_workers=args.jobs) as pool:
            for line in pool.map(setup_bank, selected):
                print(line, flush=True)
        return
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
        recipe_path = folder / "recipe.json"
        recipe = load_recipe(recipe_path)[0] if recipe_path.exists() else {}
        (folder / "project.crump").write_text(project(entry, recipe))
        print(entry["id"], len(assets), "assets", flush=True)


if __name__ == "__main__":
    main()
