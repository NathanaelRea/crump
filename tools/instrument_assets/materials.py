"""Procedural acoustic sample generation. No recorded audio inputs.

Excitation -> damped resonators / dispersive strings -> body -> optional treatment.
These are deliberately approximate material models, not calibrated instrument emulations.
Float64 computation; seeded PCG64; 48 kHz; no realtime or arbitrary-code DSL extension.
"""

import itertools

import numpy as np
from scipy import signal

RATE = 48000
VERSION = "crump-instrument-materials-v1"


def hz(note):
    return 440 * 2 ** ((note - 69) / 12)


def filtered(x, cutoff, kind="lowpass", order=2):
    return signal.sosfilt(signal.butter(order, cutoff, kind, fs=RATE, output="sos"), x)


def shift(x, frames):
    y = np.zeros_like(x)
    if 0 < frames < len(x):
        y[frames:] = x[:-frames]
    return y


def resonators(
    excitation, frequencies, decays, gains, minimum_decay=0.002, sample_rate=RATE
):
    """Parallel second-order damped modes driven by a finite force/contact signal.

    decays are amplitude e-folding times, not T60. Poles remain strictly inside unity.
    A mode's impulse peak is gain, independent of its frequency (sin(w) numerator).
    """
    out = np.zeros_like(excitation)
    for f, tau, gain in zip(frequencies, decays, gains):
        if not 18 <= f < sample_rate * 0.43:
            continue
        w = 2 * np.pi * f / sample_rate
        radius = np.exp(-1 / (sample_rate * max(tau, minimum_decay)))
        out += signal.lfilter(
            [gain * np.sin(w)], [1, -2 * radius * np.cos(w), radius**2], excitation
        )
    return out


def contact(n, seconds, rng, hardness=0.6):
    length = max(4, min(n, int(seconds * RATE)))
    x = np.zeros(n)
    t = np.linspace(0, np.pi, length)
    x[:length] = np.sin(t) ** (1 + hardness * 2)
    x[:length] *= 1 + rng.normal(0, 0.15 * hardness, length)
    return x / max(np.sum(np.abs(x)), 1e-12)


def body(x, frequencies, amount=0.15):
    colored = resonators(
        x, frequencies, [0.006] * len(frequencies), [0.012] * len(frequencies)
    )
    return x + colored * amount


def membrane(p, n, rng, velocity):
    f = p.get("freq", 150)
    decay = p.get("decay", 0.17)
    bright = p.get("bright", 0.5) * (0.55 + velocity * 0.6)
    ratios = np.array([1, 1.593, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501, 3.6, 4.06])
    weights = np.array([1, 0.62, 0.30, 0.19, 0.15, 0.13, 0.10, 0.06, 0.05, 0.025])
    if p.get("edge"):
        weights *= np.array([0.3, 1, 1.4, 1.6, 1.4, 1, 1, 1, 1, 1])
    exc = contact(n, p.get("contact", 0.0028) / (0.7 + velocity * 0.5), rng, bright)
    frequencies = f * ratios * rng.uniform(0.998, 1.002, len(ratios))
    x = resonators(
        exc,
        frequencies,
        decay / ratios**0.75,
        weights * bright ** (np.arange(10) * 0.22),
    )
    t = np.arange(n) / RATE
    # Nonlinear head tension relaxes after a kick strike: resample a modal response.
    bend = p.get("bend", 0)
    if bend:
        read = np.cumsum(1 + bend * np.exp(-t / 0.017))
        x = np.interp(read, np.arange(n), x, right=0)
    slap = filtered(rng.normal(0, 1, n), [600, 9000], "bandpass")
    x += p.get("slap", 0.045) * slap * np.exp(-t / p.get("slap_decay", 0.009))
    if p.get("wire", 0):
        wire = filtered(rng.normal(0, 1, n), [1400, 11500], "bandpass")
        chatter = 0.7 + 0.3 * filtered(rng.random(n), 650)
        x += p["wire"] * wire * chatter * np.exp(-t / p.get("wire_decay", 0.09))
    return body(x, [f * 0.73, f * 4.1, 1850], 0.16)


def clap(p, n, rng, velocity):
    x = np.zeros(n)
    t = np.arange(n) / RATE
    count = p.get("hands", 4)
    for i in range(count):
        onset = int((i * p.get("spread", 0.008) + rng.uniform(0, 0.0015)) * RATE)
        noise = filtered(
            rng.normal(0, 1, n), [p.get("low", 700), p.get("high", 7200)], "bandpass"
        )
        pulse = noise * (1 - np.exp(-t / 0.0004)) * np.exp(-t / p.get("decay", 0.024))
        x += shift(pulse, max(1, onset)) / count**0.5
    return body(x, [950, 1510, 2320], 0.3)


def metal(p, n, rng, velocity):
    count = p.get("modes", 85)
    low = p.get("low", 650)
    high = p.get("high", 16000)
    freqs = np.geomspace(low, high, count) * rng.uniform(0.96, 1.04, count)
    weights = rng.uniform(0.2, 1, count) * (freqs / 2500) ** p.get("tilt", 0.1)
    decay = p.get("decay", 0.16)
    decays = (
        decay * rng.uniform(0.6, 1.5, count) / (freqs / low) ** p.get("damping", 0.32)
    )
    exc = contact(n, p.get("contact", 0.000075), rng, 0.8)
    if p.get("rattle"):
        t = np.arange(n) / RATE
        exc += rng.normal(0, p.get("rattle_level", 0.04), n) * np.exp(
            -t / p.get("rattle_decay", 0.025)
        )
    x = resonators(exc, freqs, decays, weights / np.sqrt(count))
    # A broad contact component avoids a sparse bank sounding like a cluster of bells.
    t = np.arange(n) / RATE
    x += filtered(
        rng.normal(0, p.get("noise", 0.10), n), [2500, 17000], "bandpass"
    ) * np.exp(-t / (decay * p.get("noise_decay", 0.3)))
    return filtered(x, p.get("hipass", 800), "highpass")


def collisions(p, n, rng, velocity):
    """Independent collisions in a decaying gesture, followed by shell/jingle modes."""
    t = np.arange(n) / RATE
    gesture = (1 - np.exp(-t / p.get("rise", 0.005))) * np.exp(
        -t / p.get("decay", 0.055)
    )
    impacts = rng.random(n) < p.get("density", 1800) / RATE * gesture
    force = impacts * rng.normal(0, 1, n) * gesture
    modes = p.get("freqs", [3500, 5700, 8200])
    x = resonators(
        force,
        modes,
        [p.get("ring", 0.0018)] * len(modes),
        [0.4] * len(modes),
        minimum_decay=p.get("minimum_decay", 0.002),
    )
    # Direct grain/friction supplements the resonant collision response.
    if p.get("friction", 0):
        x += (
            p["friction"]
            * filtered(rng.normal(0, 1, n), [1000, 16000], "bandpass")
            * gesture
        )
    return filtered(x, p.get("hipass", 1800), "highpass")


def string(p, n, rng, velocity, frequency):
    """Fractional-period, lossy delay feedback excited once by a pluck.

    The delay interpolation doubles as frequency-dependent bridge loss. A separate
    loop loss sets low-frequency decay. The pickup is a spatial difference tap.
    """
    period = RATE / frequency
    delay = int(period)
    fraction = period - delay
    loss = np.exp(-period / (RATE * p.get("decay", 1.5)))
    a = np.zeros(delay + 2)
    a[0], a[delay], a[delay + 1] = 1, -loss * (1 - fraction), -loss * fraction
    excite = np.zeros(n)
    pluck = rng.normal(0, 1, delay)
    cutoff = min(15000, p.get("bright", 2600) * (0.45 + velocity * 0.65))
    excite[:delay] = filtered(pluck, cutoff) * np.hanning(delay)
    x = signal.lfilter([1], a, excite)
    pickup = max(1, int(period * p.get("position", 0.22)))
    x -= shift(x, pickup) * 0.7
    x = filtered(x, cutoff)
    t = np.arange(n) / RATE
    if p.get("fret", 0):
        x += (
            p["fret"]
            * filtered(rng.normal(0, 1, n), [1700, 9000], "bandpass")
            * np.exp(-t / 0.015)
        )
    return body(x, p.get("body", [110, 220, 430, 2100]), p.get("body_mix", 0.3))


def hammer(p, n, rng, velocity, frequency):
    """Hammer force exciting stretched string modes and slightly detuned courses."""
    partials = np.arange(1, 35)
    stiffness = p.get("stiffness", 0.00014) * (frequency / 260) ** 0.4
    modes = frequency * partials * np.sqrt(1 + stiffness * partials**2)
    strike = p.get("strike", 0.13)
    gains = np.sin(np.pi * partials * strike) / partials ** p.get("rolloff", 1.15)
    gains *= np.exp(-partials * (1 - velocity) * 0.07)
    tau = p.get("decay", 1.45) / (1 + partials**1.25 * 0.15)
    force = contact(n, p.get("contact", 0.0014) / (0.5 + velocity), rng, velocity)
    x = np.zeros(n)
    for detune in [-p.get("detune", 0.0012), 0, p.get("detune", 0.0012)]:
        x += resonators(force, modes * (1 + detune), tau, gains) / 3
    t = np.arange(n) / RATE
    x += filtered(rng.normal(0, 0.016, n), 2600) * np.exp(-t / 0.008)
    return body(x, [95, 190, 380, 740, 1400], 0.2)


def bar(p, n, rng, velocity, frequency):
    ratios = np.array(p.get("ratios", [1, 4, 9.9, 17.6]))
    gains = np.array(p.get("gains", [1, 0.24, 0.08, 0.025]))
    decays = p.get("decay", 1) / ratios ** p.get("damping", 0.75)
    x = resonators(
        contact(n, p.get("contact", 0.001), rng, velocity),
        frequency * ratios,
        decays,
        gains * velocity ** (np.arange(len(gains)) * 0.3),
    )
    if p.get("pickup", 0):
        # Position-sensitive pickup: quadratic asymmetry creates the characteristic bark.
        drive = p["pickup"] * (0.35 + velocity)
        x = np.tanh(x * drive) / max(drive, 1) + 0.18 * (x * x)
    if p.get("buzz", 0):
        t = np.arange(n) / RATE
        x += p["buzz"] * filtered(rng.normal(0, 1, n), 3800) * np.exp(-t / 0.055)
    return x


def make(recipe, note=60, velocity=0.9, seed=1, seconds=None):
    p = recipe["params"]
    duration = seconds or recipe.get("seconds", 2.8 if recipe["pitched"] else 1.2)
    n = round(duration * RATE)
    rng = np.random.Generator(np.random.PCG64(seed))
    model = recipe["model"]
    f = hz(note) * 2 ** (p.get("tune_cents", 0) / 1200)
    if model in ("string", "hammer", "bar", "piano", "tine"):
        x = globals()[model](p, n, rng, velocity, f)
    else:
        x = globals()[model](p, n, rng, velocity)
    x = filtered(x, 25 if recipe["category"] == "Bass" else 32, "highpass")
    if p.get("lowpass"):
        x = filtered(x, p["lowpass"])
    if p.get("drive"):
        peak = max(np.max(np.abs(x)), 1e-12)
        x = np.tanh(x / peak * p["drive"])
    # Normalization belongs to bank compilation, not the physical response itself.
    x *= np.minimum(1, np.arange(n) / 16)
    fade = min(round(0.045 * RATE), n // 4)
    x[-fade:] *= np.linspace(1, 0, fade) ** 2
    if not np.all(np.isfinite(x)) or np.max(np.abs(x)) < 1e-9:
        raise ValueError(f"Invalid material response: {recipe['id']}")
    return x


def treatment(x, name):
    """Explicit offline derivatives for resampled instruments."""
    if name == "tight":
        t = np.arange(len(x)) / RATE
        return x * np.exp(-t / 0.13)
    if name == "dust":
        x = filtered(x, 3800)
        return np.round(x * 2048) / 2048
    if name == "reverse":
        # Reverse a 320 ms chop so its arrival is audible inside short house notes.
        length = min(len(x), round(0.32 * RATE))
        out = np.zeros_like(x)
        out[:length] = x[:length][::-1]
        fade = min(240, length // 2)
        out[:fade] *= np.linspace(0, 1, fade)
        out[length - fade : length] *= np.linspace(1, 0, fade)
        return out
    if name == "dub":
        x = filtered(x, 2600)
        return x + 0.32 * shift(x, 17705) + 0.12 * shift(x, 35410)
    if name == "tremolo":
        t = np.arange(len(x)) / RATE
        return x * (0.78 + 0.22 * np.cos(2 * np.pi * 4.1 * t))
    if name == "room":
        return x + 0.18 * shift(x, 1104) - 0.13 * shift(x, 1776) + 0.1 * shift(x, 2736)
    return x


def diffuse(n, rng, low, high, decay, tilt=0, rise=0.0002, damping=0.4):
    """Overlapping noise bands model unresolved dense resonances independently."""
    edges = np.geomspace(max(40, low), min(19500, high), 13)
    t = np.arange(n) / RATE
    x = np.zeros(n)
    for lo, hi in itertools.pairwise(edges):
        center = np.sqrt(lo * hi)
        noise = filtered(rng.normal(0, 1, n), [lo, hi], "bandpass")
        # Unit-variance band scaling makes tilt an amplitude-per-band control.
        noise *= np.sqrt(RATE / (2 * (hi - lo)))
        tau = decay / (center / 1000) ** damping
        envelope = (1 - np.exp(-t / max(rise, 0.00005))) * np.exp(-t / max(tau, 0.0002))
        x += noise * envelope * (center / 2500) ** tilt / np.sqrt(12)
    return x


def piano(p, n, rng, velocity, frequency):
    partials = np.arange(1, 41, dtype=float)
    stiffness = p["stiffness"] * (frequency / 260) ** 0.4
    modes = frequency * partials * np.sqrt(1 + stiffness * partials**2)
    # Harmonic body at 24 kHz, polyphase interpolation to 48 kHz; broadband
    # hammer radiation is generated separately at the full output rate.
    body_frames = (n + 1) // 2
    force = contact(body_frames, p["contact"] / (2 * (0.5 + velocity)), rng, velocity)
    gains = np.abs(np.sin(np.pi * partials * p["strike"])) / partials ** p["rolloff"]
    gains *= np.exp(-partials * (1 - velocity) * 0.07)
    # Smooth radiation controls, tied to absolute frequency rather than harmonic index.
    gains *= np.interp(
        np.log(modes), np.log([150, 600, 1800, 5000, 16000]), p["radiation"]
    )
    prompt = p["decay"] / (1 + partials ** p["decay_power"] * p["treble_loss"])
    x = np.zeros(body_frames)
    # Unequal courses and two loss rates create beating plus lingering aftersound.
    for offset, weight in [(-1, 0.22), (0, 0.5), (1, 0.28)]:
        freqs = modes * (1 + offset * p["detune"] * (1 + 0.07 * np.sin(partials)))
        x += resonators(force, freqs, prompt, gains * weight, sample_rate=RATE / 2)
        x += resonators(
            force,
            freqs * (1 + offset * 0.00007),
            prompt * p["aftersound_time"],
            gains * weight * p["aftersound"],
            sample_rate=RATE / 2,
        )
    x = signal.resample_poly(x, 2, 1)[:n]
    impact = diffuse(
        n, rng, 90, 12000, p["hammer_decay"], p["hammer_tilt"], 0.0001, 0.45
    )
    x += p["hammer_noise"] * impact
    return body(x, [95, 190, 380, 740, 1400], p["soundboard"])


def tine(p, n, rng, velocity, frequency):
    # Tine + box + several weak neighboring tines, with a separate nail contact.
    force = contact(n, p["tine_contact"], rng, velocity)
    ratios = np.asarray(p["tine_ratios"])
    gains = np.asarray(p["tine_gains"])
    x = resonators(
        force, frequency * ratios, p["tine_decay"] / ratios ** p["tine_damping"], gains
    )
    for ratio, gain in zip([0.76, 0.89, 1.12, 1.28], [0.3, 0.8, 1, 0.25]):
        x += resonators(
            force,
            [frequency * ratio],
            [p["sympathetic_decay"]],
            [gain * p["sympathetic"]],
        )
    nail = diffuse(n, rng, 1200, 16000, p["nail_decay"], p["nail_tilt"], 0.00007, 0.3)
    return body(x + nail * p["nail"], [180, 390, 750], p["box"])
