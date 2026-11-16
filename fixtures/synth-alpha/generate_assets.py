#!/usr/bin/env python3
"""Regenerate the original, deterministic native-synthesis alpha WAV fixtures."""

from __future__ import annotations

import math
import struct
import wave
from collections.abc import Callable
from pathlib import Path

SAMPLE_RATE = 48_000
TABLE_LENGTH = 2_048
MEDIA = Path(__file__).with_name("media")


def pcm16(value: float) -> bytes:
    sample = round(max(-1.0, min(1.0, value)) * 32_767.0)
    return struct.pack("<h", sample)


def write_mono(path: Path, samples: list[float]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(b"".join(pcm16(sample) for sample in samples))


def table_frame(shape: Callable[[float], float]) -> list[float]:
    return [shape(index / TABLE_LENGTH) * 0.8 for index in range(TABLE_LENGTH)]


def basic_table() -> list[float]:
    sine = table_frame(lambda phase: math.sin(math.tau * phase))
    saw = table_frame(lambda phase: 2.0 * phase - 1.0)
    square = table_frame(lambda phase: 1.0 if phase < 0.5 else -1.0)
    triangle = table_frame(lambda phase: 1.0 - 4.0 * abs(phase - 0.5))
    return sine + saw + square + triangle


def sample_texture() -> list[float]:
    state = 0x5EED_1234
    samples: list[float] = []
    for index in range(SAMPLE_RATE):
        time = index / SAMPLE_RATE
        state = (state * 1_664_525 + 1_013_904_223) & 0xFFFF_FFFF
        noise = ((state >> 8) / 0xFF_FFFF) * 2.0 - 1.0
        frequency = 110.0 + 660.0 * time
        harmonic = math.sin(math.tau * frequency * time)
        overtone = math.sin(math.tau * frequency * 2.01 * time) * 0.35
        envelope = math.exp(-3.5 * time)
        samples.append((harmonic + overtone + noise * 0.18) * envelope * 0.55)
    return samples


def zone(frequency: float, brightness: float) -> list[float]:
    length = SAMPLE_RATE // 2
    samples: list[float] = []
    for index in range(length):
        time = index / SAMPLE_RATE
        envelope = min(1.0, time / 0.005) * math.exp(-2.8 * time)
        fundamental = math.sin(math.tau * frequency * time)
        upper = math.sin(math.tau * frequency * 2.0 * time) * brightness
        samples.append((fundamental + upper) * envelope * 0.55)
    return samples


def main() -> None:
    write_mono(MEDIA / "basic-table.wav", basic_table())
    write_mono(MEDIA / "sample-source.wav", sample_texture())
    write_mono(MEDIA / "multisample-low.wav", zone(130.81278265, 0.18))
    write_mono(MEDIA / "multisample-high.wav", zone(523.2511306, 0.38))


if __name__ == "__main__":
    main()
