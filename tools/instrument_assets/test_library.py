"""Check distributed dependencies and copying several modules into a fresh Project."""

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


class LibraryTests(unittest.TestCase):
    def test_complete_catalog_has_unique_ids_and_no_media(self):
        entries = json.loads((ROOT / "catalog.json").read_text())
        self.assertEqual(len(entries), 184)
        self.assertEqual(len({e["id"] for e in entries}), 184)
        profiles = set()
        for entry in entries:
            folder = ROOT / entry["id"]
            source = (folder / "instrument.crump").read_text()
            self.assertIn(f'instrument {entry["id"]} "{entry["name"]}"', source)
            for name in re.findall(r"^profile (\S+)", source, re.MULTILINE):
                self.assertNotIn(name, profiles)
                profiles.add(name)
            # Every instrument is a native graph: no assets, banks or recipes.
            self.assertIn(entry["kind"], ("acoustic", "native"))
            self.assertNotRegex(source, r"^asset ")
            for leftover in ("assets.json", "recipe.json", "media"):
                self.assertFalse((folder / leftover).exists(), f"{entry['id']}/{leftover}")

    def test_copied_modules_validate_together(self):
        binary = REPO / "target/release/crump"
        self.assertTrue(
            binary.is_file(), "Build the release crump binary before running this test"
        )
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
