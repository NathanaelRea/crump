"""Numerical and report contract checks; run with the comparator's pinned environment."""

import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
from compare import RATE, align, match_levels, metrics, read, report
from scipy.io import wavfile


class ComparisonTests(unittest.TestCase):
    def sound(self, frequency=1400):
        t = np.arange(RATE) / RATE
        return (
            np.sin(2 * np.pi * frequency * t)
            * (1 - np.exp(-t / 0.002))
            * np.exp(-t / 0.08)
        )

    def test_alignment_is_invariant_to_leading_silence(self):
        sound = self.sound()
        a, _ = align(sound, 0.5)
        for frames in [1, 32, 96, 12000]:
            b, _ = align(np.pad(sound, (frames, 0)), 0.5)
            np.testing.assert_array_equal(a, b)
        with self.assertRaises(ValueError):
            align(np.zeros(1000), 1)

    def test_level_matching_preserves_identity_and_reserves_headroom(self):
        a, b = self.sound(), self.sound() * 7
        matched, gains = match_levels([a, b])
        np.testing.assert_allclose(matched[0], matched[1], atol=1e-14)
        self.assertAlmostEqual(gains[0] / gains[1], 7)
        self.assertLessEqual(max(np.max(np.abs(x)) for x in matched), 0.80000001)
        result = metrics(*matched)
        self.assertLess(result["512"]["spectral_convergence"], 1e-12)

    def test_shifted_resonance_is_detected_at_both_resolutions(self):
        a, b = match_levels([self.sound(1400), self.sound(1900)])[0]
        same = metrics(a, a)
        changed = metrics(a, b)
        for size in ["512", "2048"]:
            self.assertEqual(same[size]["spectral_convergence"], 0)
            self.assertGreater(changed[size]["spectral_convergence"], 0.5)
            self.assertGreater(changed[size]["log_magnitude_mae_db"], 3)

    def test_wav_encodings_stereo_and_resampling(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "signal.wav"
            for rate, dtype in [(48000, "int16"), (44100, "float32"), (24000, "uint8")]:
                t = np.arange(rate) / rate
                sound = np.sin(t * 2 * np.pi * 1000) * 0.4
                encoded = (
                    np.round(sound * 32767).astype(dtype)
                    if dtype == "int16"
                    else np.round((sound + 1) * 128).astype(dtype)
                    if dtype == "uint8"
                    else sound.astype(dtype)
                )
                wavfile.write(path, rate, np.column_stack([encoded, encoded]))
                decoded, source = read(path)
                self.assertEqual(len(decoded), RATE)
                self.assertEqual(source["channels"], 2)
                self.assertLess(
                    abs(np.sqrt(np.mean(decoded**2)) - 0.4 / np.sqrt(2)), 0.005
                )
            wavfile.write(path, RATE, np.array([0, np.nan, 1], dtype=np.float32))
            with self.assertRaises(ValueError):
                read(path)
            wavfile.write(path, RATE, np.zeros(100, dtype=np.int16))
            with self.assertRaises(ValueError):
                read(path)

    def test_report_is_self_contained_and_escapes_labels(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference, candidate = root / "ref.wav", root / "candidate.wav"
            wavfile.write(reference, RATE, self.sound().astype(np.float32))
            wavfile.write(candidate, RATE, self.sound(1900).astype(np.float32))
            output = root / "report.html"
            report(reference, [("Test </script><img>", candidate)], 0.3, output)
            html = output.read_text()
            self.assertNotIn("__REPORT_DATA__", html)
            self.assertNotIn("Test </script>", html)
            self.assertEqual(html.count("data:audio/wav;base64,"), 2)
            payload = json.loads(output.with_suffix(".json").read_text())
            self.assertEqual(payload["entries"][1]["label"], "Test </script><img>")
            self.assertEqual(len(payload["entries"][0]["source"]["sha256"]), 64)
            self.assertNotIn("audio", payload["entries"][0])
            self.assertEqual(payload["configuration"]["preroll_frames"], 96)


if __name__ == "__main__":
    unittest.main()
