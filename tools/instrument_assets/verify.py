#!/usr/bin/env python3
"""Validate, publish and optionally render every example through Crump's JSON CLI."""

import argparse
import hashlib
import json
import subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ROOT = REPO / "examples/instruments"


def command(binary, folder, *args, allowed_errors=()):
    result = subprocess.run(
        [str(binary), "--project", str(folder), "--json", *args],
        capture_output=True,
        text=True,
        check=False,
    )
    payload = json.loads(result.stdout)
    if (result.returncode or not payload.get("ok")) and payload.get("error", {}).get(
        "code"
    ) not in allowed_errors:
        raise RuntimeError(f"{folder.name} {args}: {payload}")
    return payload


def verify(entry, binary, output, render):
    folder = ROOT / entry["id"]
    validation = command(binary, folder, "validate")["data"]
    check = command(binary, folder, "check", allowed_errors=("acceptance_failed",))
    if not check["ok"]:
        diagnostics = check["error"]["diagnostics"]
        if diagnostics["diagnostics"] or not next(
            c["passed"] for c in diagnostics["checks"] if c["id"] == "valid_project"
        ):
            raise RuntimeError(check)
    publication = command(
        binary,
        folder,
        "publish",
        "--origin",
        "agent",
        "--actor",
        "instrument-library",
        "--summary",
        f"Publish {entry['name']} example",
        "--explanation",
        "Portable Instrument Module and an audition phrase.",
        allowed_errors=("already_active",),
    )
    command(binary, folder, "inspect", "--at", "0bar")
    result = {
        "id": entry["id"],
        "revision": validation["revision"],
        "valid": True,
        "musical_sketch_check": check["ok"],
        "publication": publication,
    }
    if render:
        destination = output / entry["id"]
        plan = command(
            binary, folder, "render", "--plan", "--output", str(destination)
        )["data"]
        if (
            not plan["candidate_was_active"]
            or plan["rendered_active_revision"] != validation["revision"]
        ):
            raise RuntimeError("Candidate/Active identity mismatch")
        rendered = command(binary, folder, "render", "--output", str(destination))[
            "data"
        ]["render"]
        analysis = rendered["manifest"]["analysis"]
        if (
            analysis["non_finite_sample_count"]
            or analysis["clipped_sample_count"]
            or analysis["sample_peak_dbfs"] < -80
        ):
            raise RuntimeError(f"Invalid rendered audio: {entry['id']}: {analysis}")
        wavs = list(destination.rglob("*.wav"))
        if len(wavs) != 1:
            raise RuntimeError("Expected one Master WAV")
        result.update(
            analysis=analysis,
            wav=str(wavs[0].relative_to(REPO)),
            wav_sha256=hashlib.sha256(wavs[0].read_bytes()).hexdigest(),
            range=plan["render_range"],
        )
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crump", type=Path, default=REPO / "target/release/crump")
    parser.add_argument(
        "--output", type=Path, default=REPO / "renders/instrument-verification"
    )
    parser.add_argument("--render", action="store_true")
    parser.add_argument("--only", nargs="+")
    parser.add_argument("--workers", type=int, default=2)
    args = parser.parse_args()
    if not 1 <= args.workers <= 8:
        parser.error("--workers must be 1–8")
    entries = json.loads((ROOT / "catalog.json").read_text())
    if args.only and not set(args.only) <= {e["id"] for e in entries}:
        parser.error("Unknown instrument ID")
    selected = [e for e in entries if not args.only or e["id"] in args.only]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    results, errors = [], []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {
            pool.submit(verify, e, args.crump.resolve(), output, args.render): e["id"]
            for e in selected
        }
        for future in as_completed(futures):
            ident = futures[future]
            try:
                results.append(future.result())
                print("OK", ident, flush=True)
            except (
                ValueError,
                OSError,
                RuntimeError,
                KeyError,
                StopIteration,
            ) as error:
                errors.append({"id": ident, "error": str(error)})
                print("FAILED", ident, str(error), flush=True)
    (output / "verification.json").write_text(
        json.dumps(
            {"results": sorted(results, key=lambda r: r["id"]), "errors": errors},
            indent=2,
        )
        + "\n"
    )
    if errors:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
