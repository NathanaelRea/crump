#!/usr/bin/env python3
"""Copy an example into a Project and print the import to add."""

import argparse
import json
import shutil
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def copy_instrument(ident, project):
    catalog = {e["id"]: e for e in json.loads((ROOT / "catalog.json").read_text())}
    if ident not in catalog:
        raise ValueError(f"Unknown instrument: {ident}")
    project = project.resolve()
    if not (project / "project.crump").is_file():
        raise ValueError("Destination must contain project.crump")
    parent = project / "instruments"
    if parent.is_symlink():
        raise ValueError("The instruments directory must not be a symlink")
    parent.mkdir(exist_ok=True)
    destination = parent / ident
    if destination.exists() or destination.is_symlink():
        raise ValueError(f"Refusing to replace {destination}")
    source = ROOT / ident
    # Every example is one self-contained module: profiles live in the file, no media.
    files = ["instrument.crump"]
    # Stage a complete folder, then rename without editing the destination source.
    with tempfile.TemporaryDirectory(dir=parent, prefix=".instrument-") as temporary:
        staged = Path(temporary) / ident
        staged.mkdir()
        for filename in files:
            target = staged / filename
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / filename, target)
        staged.rename(destination)
    return (
        "# Add before the Project declaration (a `crump 2` root):\n"
        f'import "instruments/{ident}/instrument.crump"\n'
        f"\n# Track Instrument ID: {ident}\n"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("instrument", help="ID from catalog.json")
    parser.add_argument("project", type=Path, help="Existing Project directory")
    args = parser.parse_args()
    try:
        print(copy_instrument(args.instrument, args.project), end="")
    except (ValueError, OSError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
