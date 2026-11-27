#!/usr/bin/env python3
"""Regenerate the deterministic deep-house kick WAV fixture with the standard library."""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

SAMPLE_RATE = 48_000
CLICK_GAIN = 6.2565  # sets the click 26 dB under the body (2-8 kHz vs 30-200 Hz, first 30 ms)
MEDIA = Path(__file__).with_name("media")


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


if __name__ == "__main__":
    main()
