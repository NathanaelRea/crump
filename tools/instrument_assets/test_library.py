"""Check distributed dependencies and copying several modules into a fresh Project."""

import hashlib
import importlib.util
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ROOT = REPO / "examples/instruments"
spec = importlib.util.spec_from_file_location(
    "copy_instrument", ROOT / "copy_instrument.py"
)
copier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(copier)


SETUP = (
    "Generate the example media first: uv run tools/instrument_assets/build.py --setup"
)


class LibraryTests(unittest.TestCase):
    def test_complete_catalog_has_unique_ids_and_exact_media(self):
        entries = json.loads((ROOT / "catalog.json").read_text())
        self.assertEqual(len(entries), 92)
        self.assertEqual(len({e["id"] for e in entries}), 92)
        assets, profiles = set(), set()
        missing = 0
        for entry in entries:
            folder = ROOT / entry["id"]
            source = (folder / "instrument.crump").read_text()
            self.assertIn(f'instrument {entry["id"]} "{entry["name"]}"', source)
            for name in re.findall(r"^profile (\S+)", source, re.MULTILINE):
                self.assertNotIn(name, profiles)
                profiles.add(name)
            if entry["kind"] == "multisample":
                manifest = json.loads((folder / "assets.json").read_text())
                self.assertEqual(
                    manifest["recipe_sha256"],
                    hashlib.sha256((folder / "recipe.json").read_bytes()).hexdigest(),
                )
                code = (REPO / "tools/instrument_assets/build.py").read_bytes() + (
                    REPO / "tools/instrument_assets/materials.py"
                ).read_bytes()
                self.assertEqual(
                    manifest["generator_sha256"], hashlib.sha256(code).hexdigest()
                )
                for name, digest in manifest.get("shared_sha256", {}).items():
                    shared = REPO / "tools/instrument_assets/shared" / name
                    self.assertEqual(
                        digest, hashlib.sha256(shared.read_bytes()).hexdigest()
                    )
                for asset in manifest["assets"]:
                    self.assertNotIn(asset["id"], assets)
                    assets.add(asset["id"])
                    if not (folder / asset["path"]).exists():
                        missing += 1
                        continue
                    self.assertEqual(
                        asset["sha256"],
                        hashlib.sha256(
                            (folder / asset["path"]).read_bytes()
                        ).hexdigest(),
                    )
        if missing:
            self.skipTest(f"{missing} samples are not generated. {SETUP}")

    def test_copied_native_and_sample_modules_validate_together(self):
        binary = REPO / "target/release/crump"
        self.assertTrue(
            binary.is_file(), "Build the release crump binary before running this test"
        )
        if not any((ROOT / "finger-bass/media").glob("*.wav")):
            self.skipTest(SETUP)
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            subprocess.run(
                [str(binary), "--project", str(folder), "--json", "new"],
                check=True,
                capture_output=True,
            )
            original = (folder / "project.crump").read_text()
            imports = []
            for ident in [
                "wood-rim",
                "cuban-clave",
                "hollow-block",
                "house-grand",
                "finger-bass",
                "egg-shaker",
            ]:
                snippets = copier.copy_instrument(ident, folder)
                self.assertEqual((folder / "project.crump").read_text(), original)
                imports += re.findall(r"^import .*", snippets, re.MULTILINE)
                self.assertNotIn("asset ", snippets)
                with self.assertRaises(ValueError):
                    copier.copy_instrument(ident, folder)
            source = original.replace("crump 1", "crump 2").replace(
                "\nproject ", "\n" + "\n".join(imports) + "\nproject ", 1
            )
            (folder / "project.crump").write_text(source)
            result = subprocess.run(
                [str(binary), "--project", str(folder), "--json", "validate"],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue(json.loads(result.stdout)["ok"])


if __name__ == "__main__":
    unittest.main()
