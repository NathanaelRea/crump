#!/usr/bin/env python3
"""Regenerate the original, deterministic deep-house kick and vocal WAV fixtures.

The vocal is additive formant synthesis, not a recording: harmonics of a sung
pitch line are weighted by interpolated vowel formants, with vibrato, slow
pitch drift, and breath noise. Every region the Project plays is listed in
REGIONS so the arrangement and this file agree on exact source frames.
"""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

SAMPLE_RATE = 48_000
CLICK_GAIN = 6.2565  # sets the click 26 dB under the body (2-8 kHz vs 30-200 Hz, first 30 ms)
BEAT = 60.0 / 122.0
MEDIA = Path(__file__).with_name("media")

# Soprano formants: (frequency Hz, bandwidth Hz, amplitude) for F1..F4.
VOWELS = {
    "oo": ((330, 90, 1.0), (760, 110, 0.35), (2700, 200, 0.06), (3400, 250, 0.03)),
    "oh": ((460, 90, 1.0), (850, 110, 0.5), (2800, 200, 0.08), (3500, 250, 0.04)),
    "ah": ((820, 110, 1.0), (1200, 130, 0.6), (2900, 220, 0.12), (3700, 260, 0.05)),
    "eh": ((580, 100, 1.0), (1750, 140, 0.4), (2650, 200, 0.12), (3600, 260, 0.05)),
}

NOTE_INDEX = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def frequency(name: str) -> float:
    octave = int(name[-1])
    midi = 12 * (octave + 1) + NOTE_INDEX[name[0]]
    return 440.0 * 2.0 ** ((midi - 69) / 12.0)


def pcm16(value: float) -> bytes:
    return struct.pack("<h", round(max(-1.0, min(1.0, value)) * 32_767.0))


def write_mono(path: Path, samples: list[float]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(b"".join(pcm16(sample) for sample in samples))


class Noise:
    def __init__(self, seed: int) -> None:
        self.state = seed

    def next(self) -> float:
        self.state = (self.state * 1_664_525 + 1_013_904_223) & 0xFFFF_FFFF
        return ((self.state >> 8) / 0xFF_FFFF) * 2.0 - 1.0


def formant_gain(vowel: tuple, hz: float) -> float:
    gain = 0.015
    for centre, bandwidth, amplitude in vowel:
        distance = (hz - centre) / (bandwidth * 0.5)
        gain += amplitude / (1.0 + distance * distance)
    return gain


def blend(a: tuple, b: tuple, amount: float) -> tuple:
    return tuple(
        tuple(x + (y - x) * amount for x, y in zip(fa, fb)) for fa, fb in zip(a, b)
    )


def sing(notes: list[tuple[float, float, str, str]], length: float, seed: int, vibrato: float) -> list[float]:
    """Render a legato line. Each note is (start s, duration s, pitch, vowel)."""
    noise = Noise(seed)
    drift = Noise(seed ^ 0x9E37)
    frames = int(length * SAMPLE_RATE)
    phases = [0.0] * 24
    out: list[float] = []
    slow = 0.0
    breath_low = 0.0
    breath_band = 0.0
    for index in range(frames):
        time = index / SAMPLE_RATE
        current = None
        previous = None
        for position, note in enumerate(notes):
            if note[0] <= time:
                current = note
                previous = notes[position - 1] if position > 0 else None
        if current is None:
            out.append(0.0)
            continue
        start, duration, pitch, vowel_name = current
        local = time - start
        target = frequency(pitch)
        vowel = VOWELS[vowel_name]
        if previous is not None and previous[0] + previous[1] >= start - 0.02:
            glide = min(1.0, local / 0.07)
            glide = glide * glide * (3.0 - 2.0 * glide)
            source = frequency(previous[2])
            target = source * (target / source) ** glide
            morph = min(1.0, local / 0.09)
            vowel = blend(VOWELS[previous[3]], vowel, morph)
        # Amplitude: soft onset, breath dip at note joins, release at the end.
        attack = min(1.0, local / 0.045)
        release = max(0.0, min(1.0, (duration - local) / 0.08))
        amplitude = attack * attack * (3.0 - 2.0 * attack) * release
        if local >= duration:
            amplitude = 0.0
        vibrato_depth = vibrato * min(1.0, max(0.0, (local - 0.18) / 0.35))
        slow += (drift.next() * 0.5 - slow) * 0.0004
        cents = vibrato_depth * math.sin(math.tau * 5.3 * time) + slow * 9.0
        f0 = target * 2.0 ** (cents / 1200.0)
        voiced = 0.0
        for harmonic in range(1, 25):
            hz = f0 * harmonic
            if hz > 7_500.0:
                break
            phases[harmonic - 1] = (phases[harmonic - 1] + hz / SAMPLE_RATE) % 1.0
            weight = formant_gain(vowel, hz) / harmonic**0.7
            voiced += weight * math.sin(math.tau * phases[harmonic - 1])
        white = noise.next()
        breath_low += 0.35 * (white - breath_low)
        breath_band += 0.08 * ((white - breath_low) - breath_band)
        breath = (white - breath_low) * 0.5 + breath_band
        out.append((voiced * 0.55 + breath * 0.05) * amplitude)
    return out


def b(beats: float) -> float:
    return beats * BEAT


def vocal() -> tuple[list[float], dict[str, tuple[int, int]]]:
    """Return the vocal asset and its named half-open source regions."""
    hook = sing(
        [
            (b(0.5), b(0.95), "E5", "oo"),
            (b(1.5), b(0.5), "G5", "ah"),
            (b(2.0), b(1.4), "A5", "ah"),
            (b(3.5), b(0.5), "G5", "eh"),
            (b(4.0), b(1.9), "E5", "oh"),
            (b(6.0), b(0.5), "D5", "oo"),
            (b(6.5), b(1.3), "E5", "ah"),
        ],
        b(8.0),
        0x0A11,
        22.0,
    )
    answer = sing(
        [
            (b(0.5), b(0.95), "C5", "oh"),
            (b(1.5), b(0.5), "D5", "ah"),
            (b(2.0), b(1.9), "E5", "ah"),
            (b(4.0), b(0.5), "D5", "eh"),
            (b(4.5), b(0.5), "C5", "oo"),
            (b(5.0), b(2.6), "A4", "oo"),
        ],
        b(8.0),
        0x0B22,
        22.0,
    )
    chop_ah = sing([(0.0, 0.3, "A4", "ah")], 0.34, 0x0C33, 0.0)
    chop_oh = sing([(0.0, 0.3, "C5", "oh")], 0.34, 0x0D44, 0.0)
    chop_eh = sing([(0.0, 0.3, "E5", "eh")], 0.34, 0x0E55, 0.0)
    sustain_ah = sing([(0.0, 3.0, "A4", "ah")], 3.0, 0x0F66, 12.0)
    sustain_oo = sing([(0.0, 3.0, "A4", "oo")], 3.0, 0x1077, 12.0)

    gap = [0.0] * (SAMPLE_RATE // 20)
    regions: dict[str, tuple[int, int]] = {}
    samples: list[float] = []
    for name, part in [
        ("hook", hook),
        ("answer", answer),
        ("chop-ah", chop_ah),
        ("chop-oh", chop_oh),
        ("chop-eh", chop_eh),
        ("sustain-ah", sustain_ah),
        ("sustain-oo", sustain_oo),
    ]:
        regions[name] = (len(samples), len(samples) + len(part))
        samples.extend(part)
        samples.extend(gap)
    peak = max(abs(sample) for sample in samples)
    return [sample * 0.89 / peak for sample in samples], regions


def kick(click_gain: float = CLICK_GAIN) -> list[float]:
    """A tight, round kick shaped by measurements of the reference tracks' kicks.

    The references settle at 45-50 Hz within about 50 ms, fall 20 dB in 60-180 ms,
    and carry a soft click. This body falls from 110 Hz to 49 Hz (10 ms time
    constant), holds for 10 ms, reaches -20 dB at about 80 ms, and has a 2-8 kHz
    click about 26 dB under the body, so it sits inside a dense mix.
    """
    noise = Noise(0x4B1C)
    frames = 20_160
    f_start, f_end, sweep, hold, drive = 110.0, 49.0, 0.010, 0.010, 1.5
    tau = (0.0681 - hold) / math.log(10.0)
    white = [noise.next() for _ in range(frames)]
    burst = [w * math.exp(-(i / SAMPLE_RATE) / 0.0025) for i, w in enumerate(white)]
    smooth = [sum(burst[max(0, i - 1): i + 2]) / 3.0 for i in range(frames)]
    out: list[float] = []
    phase = 0.0
    for index in range(frames):
        time = index / SAMPLE_RATE
        phase += (f_end + (f_start - f_end) * math.exp(-time / sweep)) / SAMPLE_RATE
        amp = 1.0 if time < hold else math.exp(-(time - hold) / tau)
        amp *= min(1.0, time / 0.0008)
        wide = sum(smooth[max(0, index - 6): index + 6]) / 12.0
        click = (smooth[index] - wide) * click_gain
        value = math.tanh(drive * (math.sin(math.tau * phase) * amp + click)) / math.tanh(drive)
        out.append(value * min(1.0, (frames - index) / 480.0))
    peak = max(abs(sample) for sample in out)
    return [sample * 0.89 / peak for sample in out]


def main() -> None:
    write_mono(MEDIA / "kick.wav", kick())
    samples, regions = vocal()
    write_mono(MEDIA / "vocal-phrases.wav", samples)
    for name, (start, end) in regions.items():
        print(f"{name} {start} {end}")


if __name__ == "__main__":
    main()
