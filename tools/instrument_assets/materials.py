"""Procedural acoustic sample generation. No recorded audio inputs.

Excitation -> damped resonators / dispersive strings -> body -> optional treatment.
These are deliberately approximate material models, not calibrated instrument emulations.
Float64 computation; seeded PCG64; 48 kHz; no realtime or arbitrary-code DSL extension.
"""

import itertools
import json
from pathlib import Path

import numpy as np
from scipy import signal

RATE = 48000
VERSION = "crump-instrument-materials-v1"
PITCHED = (
    "string",
    "hammer",
    "bar",
    "piano",
    "tine",
    "sustained",
    "courses",
    "timpani",
    "organ",
    "bowed",
    "choir",
    "pizzicato",
    "machine_tuned",
    "tuned_drum",
    "modes_tuned",
    "layered_tuned",
    "plucked",
    "tonewheel",
    "prepared",
    "drone",
    "hurdy",
)


SHARED = Path(__file__).resolve().parent / "shared"


def load_recipe(path):
    """Read a recipe, resolving its shared family and named body curves.

    `"extends": "bowed"` starts from `shared/bowed.json`; the recipe's own keys
    and params win. A string `eq`, or a name in `eq_registers`, reads
    `shared/bodies/NAME.json`. Returns the recipe and the shared files read.
    """
    recipe = json.loads(Path(path).read_text())
    used = []

    def shared(name):
        used.append(SHARED / name)
        return json.loads(used[-1].read_text())

    if "extends" in recipe:
        family = shared(f"{recipe.pop('extends')}.json")
        params = family.pop("params", {}) | recipe.get("params", {})
        recipe = family | recipe | {"params": params}
    p = recipe["params"]
    if isinstance(p.get("eq"), str):
        p["eq"] = shared(f"bodies/{p['eq']}.json")
    if "eq_registers" in p:
        p["eq_registers"] = [
            [midi, shared(f"bodies/{body}.json") if isinstance(body, str) else body]
            for midi, body in p["eq_registers"]
        ]
    return recipe, used


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
    if model in PITCHED:
        x = globals()[model](p, n, rng, velocity, f)
    else:
        x = globals()[model](p, n, rng, velocity)
    if recipe.get("reference_eq"):
        # A smooth correction fitted against reference recordings
        # (references.py --fit); level-neutral and capped.
        x = equalize(x, recipe["reference_eq"])
    x = filtered(
        x,
        recipe.get("highpass", 25 if recipe["category"] == "Bass" else 32),
        "highpass",
    )
    if p.get("lowpass"):
        x = filtered(x, p["lowpass"])
    if p.get("drive"):
        peak = max(np.max(np.abs(x)), 1e-12)
        x = np.tanh(x / peak * p["drive"])
    # Normalization belongs to bank compilation, not the physical response itself.
    x *= np.minimum(1, np.arange(n) / 16)
    fade = min(round(0.045 * RATE), n // 4)
    # Stereo models return (2, frames); every step here works on the last axis.
    x[..., -fade:] *= np.linspace(1, 0, fade) ** 2
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
    if name == "leslie":
        # Rotary speaker: a horn's Doppler shift and level sweep at 6.25 Hz, ten
        # turns per sustain loop, under the slower bass drum.
        t = np.arange(len(x)) / RATE
        horn = 2 * np.pi * 6.25 * t
        drum = 2 * np.pi * 1.25 * t
        delay = RATE * (0.00045 * (1 + np.sin(horn)))
        moved = np.interp(np.arange(len(x)) - delay, np.arange(len(x)), x, left=0)
        upper = filtered(moved, 800, "highpass") * (1 + 0.3 * np.cos(horn))
        lower = filtered(x, 800) * (1 + 0.12 * np.cos(drum))
        # The level sweep must not push the normalized bank past full scale.
        out = upper + lower
        return out * np.max(np.abs(x)) / max(np.max(np.abs(out)), 1e-12)
    if name == "tape":
        # Tape replay: wow, flutter and a narrow, slightly saturated playback head.
        rng = np.random.Generator(np.random.PCG64(len(x)))
        t = np.arange(len(x)) / RATE
        cents = 5 * np.sin(2 * np.pi * 0.55 * t) + 2.5 * np.sin(2 * np.pi * 7.3 * t)
        cents += 2 * wander(len(x), rng, 3)
        x = glide(x, cents)
        x = filtered(filtered(x, 6500, order=4), 90, "highpass")
        peak = max(np.max(np.abs(x)), 1e-12)
        x = np.tanh(1.6 * x / peak) / np.tanh(1.6) * peak
        return x + peak * 0.0025 * filtered(rng.normal(0, 1, len(x)), [800, 8000], "bandpass")
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


# Orchestral models. Sustained tones are additive harmonic sources whose partials
# read a fixed body/formant response at their instantaneous frequency, so vibrato
# produces the per-partial amplitude motion of a real resonating body.


def smoothstep(x, low, high):
    u = np.clip((np.asarray(x, dtype=float) - low) / (high - low), 0, 1)
    return u * u * (3 - 2 * u)


def wander(n, rng, rate):
    """Seeded unit-variance random motion below `rate` Hz."""
    x = filtered(rng.normal(0, 1, n + RATE), rate)[RATE:]
    return x / max(np.std(x), 1e-12)


def resonance_sum(f, peaks, floor):
    """Magnitude envelope: a floor plus second-order resonance peaks [center, Q, gain]."""
    g = np.full_like(f, floor, dtype=float)
    for center, q, gain in peaks:
        g += gain / np.sqrt(1 + q * q * (f / center - center / f) ** 2)
    return g


def spectral_envelope(p, scale=1.0, body_seed=0):
    """Fixed-frequency response shared by every note of one instrument (or player)."""
    peaks = [[c * scale, q, a] for c, q, a in p.get("formants", [])]
    if p.get("body_modes"):
        rng = np.random.Generator(np.random.PCG64(p["body_seed"] + body_seed))
        low, high = p["body_range"]
        count = p["body_modes"]
        centers = np.geomspace(low, high, count) * rng.uniform(0.9, 1.1, count)
        peaks += [
            [c * scale, q, a]
            for c, q, a in zip(
                centers, rng.uniform(10, 28, count), rng.uniform(0.15, 1, count)
            )
        ]
    floor = p.get("floor", 0.2)
    high_cut, order = p.get("rolloff", [6000, 2])
    low_cut = p.get("low_cut", 0)

    def response(f):
        f = np.maximum(f, 1.0)
        g = resonance_sum(f, peaks, floor)
        g /= np.sqrt(1 + (f / high_cut) ** (2 * order))
        if low_cut:
            g *= (f / low_cut) ** 2 / (1 + (f / low_cut) ** 2)
        return g

    return response


def shaped_noise(n, rng, response, band, comb=0.0, frequency=None):
    """White noise filtered by a static response, band and optional harmonic comb."""
    spectrum = np.fft.rfft(rng.normal(0, 1, n))
    f = np.fft.rfftfreq(n, 1 / RATE)
    gain = (
        response(f)
        * smoothstep(f, band[0] * 0.7, band[0])
        * (1 - smoothstep(f, band[1], band[1] * 1.4))
    )
    if comb and frequency:
        gain *= (1 - comb) + comb * np.cos(np.pi * f / frequency) ** 8
    x = np.fft.irfft(spectrum * gain, n)
    return x / max(np.std(x), 1e-12)


def sustained_voice(p, n, rng, velocity, frequency, response, onset=0):
    t = np.maximum(np.arange(n) - onset, 0) / RATE
    attack = np.interp(velocity, [0.48, 0.92], p["attack"])
    envelope = (1 - np.exp(-t / attack)) ** 1.6 * (np.arange(n) >= onset)
    seconds = n / RATE
    taper = p.get("taper", 1.0)
    envelope *= (
        np.cos(0.5 * np.pi * smoothstep(t, seconds - taper - 0.05, seconds)) ** 2
    )
    # Slow bow/breath pressure changes keep the plateau from sounding frozen.
    # Brightness follows dynamic level more strongly than amplitude does. It reads
    # the smooth envelope so slow pressure changes cannot flicker high partials.
    loud = envelope * (0.2 + 0.8 * velocity)
    envelope *= 1 + p.get("swell", 0.04) * wander(n, rng, 0.9)
    amplitude = envelope * (0.55 + 0.45 * velocity)

    rate, soft_depth, hard_depth, delay, fade = p.get("vibrato", [5.5, 0, 0, 0.3, 0.4])
    depth = np.interp(velocity, [0.48, 0.92], [soft_depth, hard_depth])
    rate *= rng.uniform(0.94, 1.06)
    ramp = smoothstep(t, delay, delay + fade)
    vibrato_phase = (
        2 * np.pi * np.cumsum(rate * (1 + 0.04 * wander(n, rng, 0.7))) / RATE
    )
    vibrato = np.sin(vibrato_phase + rng.uniform(0, 2 * np.pi)) * ramp
    cents = depth * vibrato + p.get("drift", 2) * wander(n, rng, 1.3)
    # Players and singers never hold a perfect pitch: vibrato depth breathes and
    # each cycle is slightly irregular. Both are optional so older recipes keep
    # their exact output.
    if p.get("vibrato_wander"):
        cents += depth * vibrato * p["vibrato_wander"] * wander(n, rng, 0.6)
    if p.get("jitter"):
        cents += p["jitter"] * wander(n, rng, 25)
    scoop, scoop_time = p.get("scoop", [0, 0.03])
    cents += scoop * np.exp(-t / scoop_time) * (0.5 + velocity)
    f = frequency * 2 ** (cents / 1200)
    phase = 2 * np.pi * np.cumsum(f) / RATE

    slope = np.interp(velocity, [0.48, 0.92], p["slope"])
    gamma = p.get("gamma", 0.0)
    even_gain, even_cross = p.get("even", [1, 0])
    shimmer = p.get("shimmer", 0.03)
    motions = p.get("motions", 5)
    motion_tilt = p.get("motion_tilt", 0)
    motion = [wander(n, rng, p.get("shimmer_rate", 9)) for _ in range(motions)]
    ranks = p.get("ranks", [[1, 1]])
    am = 1 + p.get("am", 0) * vibrato
    x = np.zeros(n)
    k = 1
    while k * frequency < min(17000, RATE * 0.42):
        weight = sum(g * (k // r) ** -slope for r, g in ranks if k % r == 0)
        if weight:
            fk = k * f
            gain = weight * response(fk)
            if k % 2 == 0 and even_cross:
                gain = gain * (
                    even_gain
                    + (1 - even_gain) * smoothstep(fk, even_cross, even_cross * 2)
                )
            if gamma:
                gain = gain * np.maximum(loud, 1e-6) ** (gamma * min(k - 1, 16))
            # Upper partials fluctuate more, each group independently.
            gain = gain * (
                1
                + shimmer * k**motion_tilt * motion[k % motions] * (1 if k % 2 else -1)
            )
            x += gain * np.sin(k * phase + rng.uniform(0, 2 * np.pi))
        k += 1
    x *= amplitude * am

    level, burst, burst_decay = p.get("noise", [0, 0, 0.03])
    if level or burst:
        noise = shaped_noise(
            n,
            rng,
            response,
            p.get("noise_band", [300, 9000]),
            p.get("noise_comb", 0),
            frequency,
        )
        if p.get("pulse"):
            # Turbulence is strongest at one point in each vibration cycle (glottal
            # closure, reed or jet), so breath noise pulses at the played pitch.
            cycle = ((1 + np.cos(phase)) / 2) ** 4 / 0.2734375
            noise = noise * ((1 - p["pulse"]) + p["pulse"] * cycle)
        sustain_noise = level * amplitude
        # Bow scratch, tonguing or chiff. It swells in with the tone; an abrupt
        # broadband burst reads as a drum hit.
        onset_noise = (
            burst * velocity * (1 - np.exp(-t / 0.012)) * np.exp(-t / burst_decay)
        )
        x += np.std(x) * noise * (sustain_noise + onset_noise)
    return x


def sustained(p, n, rng, velocity, frequency):
    """Bowed, blown, sung or piped tone; `voices` > 1 builds a section."""
    voices = p.get("voices", 1)
    x = np.zeros(n)
    for voice in range(voices):
        if voices > 1:
            scale = rng.uniform(
                1 - p.get("formant_spread", 0), 1 + p.get("formant_spread", 0)
            )
            detune = 2 ** (rng.uniform(-1, 1) * p.get("detune", 0) / 1200)
            onset = int(rng.uniform(0, p.get("spread", 0)) * RATE)
        else:
            scale, detune, onset = 1.0, 1.0, 0
        if p.get("register_shift"):
            # Higher notes are sung by higher voices with higher formants.
            scale *= 1 + p["register_shift"] * smoothstep(
                np.log2(frequency / 110), 0, 2
            )
        response = spectral_envelope(p, scale, voice)
        x += sustained_voice(p, n, rng, velocity, frequency * detune, response, onset)
    return x / np.sqrt(voices)


def equalize(x, points):
    """Minimum-phase EQ through `[hz, db]` points on a log-frequency axis.

    Minimum phase keeps sharp body resonances causal: they ring after a note
    starts, as a real body does, instead of before it.
    """
    if not points:
        return x
    if x.ndim > 1:
        return np.vstack([equalize(channel, points) for channel in x])
    hz, db = np.asarray(points, dtype=float).T
    size = 1 << 15
    f = np.maximum(np.fft.rfftfreq(size, 1 / RATE), hz[0])
    log_magnitude = np.interp(np.log(f), np.log(hz), db) * np.log(10) / 20
    # Real cepstrum folding turns a magnitude response into its minimum-phase filter.
    cepstrum = np.fft.irfft(log_magnitude, size)
    fold = np.zeros(size)
    fold[0], fold[size // 2] = cepstrum[0], cepstrum[size // 2]
    fold[1 : size // 2] = 2 * cepstrum[1 : size // 2]
    response = np.fft.irfft(np.exp(np.fft.rfft(fold)), size)
    return signal.fftconvolve(x, response)[: len(x)]


def bow_loop(delay, bow, slope, beta, pole):
    """Bowed string waveguide (after the STK Bowed model).

    Nut-side and bridge-side delay lines meet at the bow. A friction table maps
    the bow/string velocity difference to how much the string sticks, giving
    Helmholtz stick-slip motion with its natural rasp and cycle irregularity.
    `delay`, `bow` and `slope` are per-sample sequences; the output is the bridge
    velocity, which drives the body.
    """
    size = int(max(delay)) + 4
    neck, bridge = [0.0] * size, [0.0] * size
    gain = (1 - pole) * 0.95
    out = [0.0] * len(bow)
    lowpass = neck_out = bridge_out = 0.0
    write = 0
    for i, (total, velocity, steepness) in enumerate(zip(delay, bow, slope)):
        lowpass = gain * bridge_out + pole * lowpass
        bridge_reflection = -lowpass
        nut_reflection = -neck_out
        difference = velocity - (bridge_reflection + nut_reflection)
        friction = (abs((difference + 0.001) * steepness) + 0.75) ** -4
        transmitted = difference * (min(1, friction))
        neck[write] = bridge_reflection + transmitted
        bridge[write] = nut_reflection + transmitted
        position = write - total * (1 - beta)
        index = int(position // 1)
        fraction = position - index
        neck_out = (
            neck[index % size] * (1 - fraction) + neck[(index + 1) % size] * fraction
        )
        position = write - total * beta
        index = int(position // 1)
        fraction = position - index
        bridge_out = (
            bridge[index % size] * (1 - fraction)
            + bridge[(index + 1) % size] * fraction
        )
        out[i] = bridge_out
        write = (write + 1) % size
    return np.asarray(out)


def fundamental(x, expected):
    """Autocorrelation pitch estimate near `expected` Hz, with parabolic refinement."""
    x = x - np.mean(x)
    spectrum = np.fft.rfft(x, 2 * len(x))
    correlation = np.fft.irfft(spectrum * np.conj(spectrum))[: len(x)]
    lag = RATE / expected
    low, high = int(lag * 0.8), min(int(lag * 1.25) + 2, len(x) - 2)
    peak = low + int(np.argmax(correlation[low:high]))
    a, b, c = correlation[peak - 1 : peak + 2]
    shift = 0.5 * (a - c) / (a - 2 * b + c) if a - 2 * b + c else 0
    return RATE / (peak + shift)


def body_modes(p, seed):
    """A dense, jagged body: fixed for one instrument so vibrato moves partials through it."""
    rng = np.random.Generator(np.random.PCG64(p["body_seed"] + seed))
    low, high = p["body_range"]
    count = p["body_modes"]
    freqs = np.geomspace(low, high, count) * rng.uniform(0.92, 1.08, count)
    q = rng.uniform(*p.get("body_q", [18, 45]), count)
    gains = rng.uniform(0.1, 1, count) * (freqs / 1000) ** p.get("body_tilt", 0)
    for center, quality, gain in p.get("formants", []):
        freqs, q, gains = (
            np.append(freqs, center),
            np.append(q, quality),
            np.append(gains, gain),
        )
    return freqs, q / (np.pi * freqs), gains


def bowed_player(p, n, rng, velocity, frequency, seed, onset=0):
    t = np.maximum(np.arange(n) - onset, 0) / RATE
    seconds = n / RATE
    attack = np.interp(velocity, [0.48, 0.92], p["attack"])
    envelope = (1 - np.exp(-t / attack)) ** 1.5 * (np.arange(n) >= onset)
    # Slowing the bow under pressure turns raucous, so the end of the sample
    # fades the output while the bow keeps moving.
    taper = (
        np.cos(0.5 * np.pi * smoothstep(t, seconds - p["taper"] - 0.05, seconds)) ** 2
    )
    # Bow speed sets loudness; pressure sets how raspy and bright the tone is.
    speed = np.interp(velocity, [0.48, 0.92], p["bow_speed"])
    pressure = np.interp(velocity, [0.48, 0.92], p["bow_pressure"])
    bow = speed * envelope * (1 + p.get("bow_wander", 0.04) * wander(n, rng, 2.0))
    bow *= 1 + p.get("rosin", 0.02) * filtered(rng.normal(0, 1, n), 2000)
    # Players build pressure together with speed. Full pressure on a slow bow
    # drags the string into loud irregular slips: a crunch before the tone.
    slope = 5 - 4 * pressure * np.sqrt(envelope)
    if p.get("tremolo"):
        # Bowed tremolo: rapid strokes that reverse the bow. Each player keeps
        # their own slightly unsteady stroke rate.
        strokes = np.cumsum(
            p["tremolo"] * rng.uniform(0.92, 1.08) * (1 + 0.1 * wander(n, rng, 1.5))
        )
        stroke = np.sin(np.pi * strokes / RATE + rng.uniform(0, np.pi))
        bow = bow * np.sign(stroke) * np.abs(stroke) ** 0.6

    rate, soft_depth, hard_depth, delay, fade = p["vibrato"]
    depth = np.interp(velocity, [0.48, 0.92], [soft_depth, hard_depth])
    rate *= rng.uniform(0.94, 1.06)
    ramp = smoothstep(t, delay, delay + fade)
    phase = 2 * np.pi * np.cumsum(rate * (1 + 0.05 * wander(n, rng, 0.7))) / RATE
    vibrato = np.sin(phase + rng.uniform(0, 2 * np.pi)) * ramp
    cents = depth * vibrato * (1 + p.get("vibrato_wander", 0.25) * wander(n, rng, 0.6))
    cents += p.get("drift", 2) * wander(n, rng, 1.3)

    # Loop filters lengthen the period; bow a short steady trial and correct.
    beta, pole = p.get("beta", 0.127), p.get("pole", 0.66)
    base = RATE / frequency - 4
    trial = int(0.5 * RATE)
    steady = (speed * smoothstep(np.arange(trial) / RATE, 0, 0.05)).tolist()
    pressing = [5 - 4 * pressure] * trial
    for _ in range(2):
        probe = bow_loop([base] * trial, steady, pressing, beta, pole)
        base *= fundamental(probe[trial // 2 :], frequency) / frequency
    if p.get("vibrato_after_string"):
        # Bow a steady string, then apply the pitch movement by resampling its
        # output (4x oversampled). Modulating the waveguide's own length
        # perturbs the stick-slip motion, which is audible high up the range.
        x = bow_loop([base] * n, bow.tolist(), slope.tolist(), beta, pole)
        dense = signal.resample_poly(x, 4, 1)
        read = np.cumsum(2 ** (cents / 1200)) - 2 ** (cents[0] / 1200)
        x = np.interp(np.minimum(read, n - 1) * 4, np.arange(len(dense)), dense)
    else:
        delay_line = base * 2 ** (-cents / 1200)
        x = bow_loop(delay_line.tolist(), bow.tolist(), slope.tolist(), beta, pole)
    freqs, decays, gains = body_modes(p, seed)
    x = x * p.get("direct", 0.3) + resonators(x, freqs, decays, gains)
    if p.get("bow_noise"):
        # Hair-on-string noise rides each slip, so it pulses at the played pitch.
        cycle = (
            (1 + np.cos(2 * np.pi * np.cumsum(frequency * 2 ** (cents / 1200)) / RATE))
            / 2
        ) ** 4
        hiss = filtered(rng.normal(0, 1, n), p.get("bow_noise_low", 1500), "highpass")
        x += (
            p["bow_noise"]
            * np.std(x)
            * hiss
            * (0.4 + cycle / 0.2734375 * 0.6)
            * envelope
        )
    # The string settles into Helmholtz motion during the first few periods;
    # a short fade keeps that settling from reading as a click.
    settle = (1 - np.exp(-t / (0.4 * attack))) ** 2 * (np.arange(n) >= onset)
    if p.get("tremolo"):
        # Each bow reversal thumps below the note; the body does not radiate it.
        x = filtered(x, 0.7 * frequency, "highpass")
    return taper * settle * x


def bowed(p, n, rng, velocity, frequency):
    """Bowed string or section: physical stick-slip strings through jagged bodies."""
    voices = p.get("voices", 1)
    width = p.get("stereo", 0)
    x = np.zeros((2, n)) if width else np.zeros(n)
    for voice in range(voices):
        detune = (
            2 ** (rng.uniform(-1, 1) * p.get("detune", 0) / 1200) if voices > 1 else 1
        )
        onset = int(rng.uniform(0, p.get("spread", 0)) * RATE) if voices > 1 else 0
        tone = bowed_player(p, n, rng, velocity, frequency * detune, voice, onset)
        tone = tone / max(np.std(tone), 1e-12)
        if p.get("player_variation"):
            # Each player's instrument has its own body: a smooth random
            # deviation of `player_variation` dB across the spectrum.
            deviation = rng.normal(0, p["player_variation"], 12)
            tone = equalize(
                tone, [[60 * 2 ** (0.75 * i), d] for i, d in enumerate(deviation)]
            )
        if width:
            # Equal-power placement across the stage.
            angle = (rng.uniform(-width, width) + 1) * np.pi / 4
            x += np.outer([np.cos(angle), np.sin(angle)], tone)
        else:
            x += tone
    high, order = p.get("rolloff", [6000, 4])
    # `eq` is a body response fitted to reference recordings. A section blends
    # the bodies of its instruments, `eq_registers` `[[midi, points], ...]`, by pitch.
    eq = p.get("eq")
    if p.get("eq_registers"):
        note = 69 + 12 * np.log2(frequency / 440)
        centers = [center for center, _ in p["eq_registers"]]
        weights = [np.interp(note, centers, row) for row in np.eye(len(centers))]
        hz = np.asarray(p["eq_registers"][0][1])[:, 0]
        db = sum(
            w * np.asarray(points)[:, 1]
            for w, (_, points) in zip(weights, p["eq_registers"])
        )
        eq = np.column_stack([hz, db]).tolist()
    return equalize(filtered(x / np.sqrt(voices), high, order=order), eq)


def glottal_singer(p, n, rng, velocity, frequency, formants, onset):
    """One singer: Rosenberg glottal pulses through cascaded vocal-tract formants."""
    t = np.maximum(np.arange(n) - onset, 0) / RATE
    seconds = n / RATE
    attack = np.interp(velocity, [0.48, 0.92], p["attack"])
    envelope = (1 - np.exp(-t / attack)) ** 1.6 * (np.arange(n) >= onset)
    envelope *= (
        np.cos(0.5 * np.pi * smoothstep(t, seconds - p["taper"] - 0.05, seconds)) ** 2
    )
    envelope *= 1 + p.get("swell", 0.05) * wander(n, rng, 0.8)

    rate, soft_depth, hard_depth, delay, fade = p["vibrato"]
    depth = np.interp(velocity, [0.48, 0.92], [soft_depth, hard_depth])
    depth *= rng.uniform(0.6, 1.3)
    rate *= rng.uniform(0.88, 1.12)
    ramp = smoothstep(t, delay * rng.uniform(0.7, 1.3), delay + fade)
    phase = 2 * np.pi * np.cumsum(rate * (1 + 0.06 * wander(n, rng, 0.7))) / RATE
    cents = depth * np.sin(phase + rng.uniform(0, 2 * np.pi)) * ramp
    cents *= 1 + p.get("vibrato_wander", 0.3) * wander(n, rng, 0.6)
    cents += p.get("drift", 5) * wander(n, rng, 0.9) + p.get("jitter", 6) * wander(
        n, rng, 30
    )
    scoop, scoop_time = p.get("scoop", [0, 0.05])
    cents += scoop * np.exp(-t / scoop_time)
    cycles = np.cumsum(frequency * 2 ** (cents / 1200)) / RATE
    position = cycles % 1

    # Louder singing closes the glottis for more of each cycle: a sharper,
    # brighter pulse. Softer singing is breathier.
    open_quotient = np.interp(velocity, [0.48, 0.92], p["open_quotient"])
    # A quick closing phase is what gives a voice its upper harmonics.
    rising, falling = 0.72 * open_quotient, 0.28 * open_quotient
    flow = np.where(
        position < rising,
        0.5 * (1 - np.cos(np.pi * position / rising)),
        np.where(
            position < open_quotient,
            np.cos(0.5 * np.pi * (position - rising) / falling),
            0.0,
        ),
    )
    shimmer = 1 + p.get("shimmer", 0.04) * rng.normal(0, 1, int(cycles[-1]) + 2)
    source = np.diff(flow * shimmer[cycles.astype(int)], prepend=0.0)
    breath = np.interp(velocity, [0.48, 0.92], p["breath"])
    turbulence = filtered(rng.normal(0, 1, n), [400, 7000], "bandpass")
    source += breath * np.std(source) * turbulence * (position < open_quotient)
    source *= envelope

    # Cascaded resonators with unity DC gain: relative formant levels follow
    # from their frequencies and bandwidths, as in a real vocal tract.
    voice = source
    for center, bandwidth in formants:
        radius = np.exp(-np.pi * bandwidth / RATE)
        cosine = np.cos(2 * np.pi * center / RATE)
        a = [1, -2 * radius * cosine, radius * radius]
        voice = signal.lfilter([sum(a)], a, voice)
    # Breath escaping past the tract adds the airy top a cascade alone lacks.
    air = filtered(rng.normal(0, 1, n), 3500, "highpass") * (position < open_quotient)
    return voice + p.get("air", 0.0) * np.std(voice) * air * envelope


def choir(p, n, rng, velocity, frequency):
    """A section of singers whose vowel formants follow their voice part."""
    note = 69 + 12 * np.log2(frequency / 440)
    parts = p["parts"]
    centers = np.array([part["center"] for part in parts], dtype=float)
    x = np.zeros(n)
    for _ in range(p["voices"]):
        # Each singer is the nearest part, or occasionally the next one, so
        # neighbouring parts blend across their shared notes.
        weights = np.exp(-(((centers - note) / p.get("part_spread", 5)) ** 2))
        part = parts[rng.choice(len(parts), p=weights / weights.sum())]
        scale = rng.uniform(1 - p["formant_spread"], 1 + p["formant_spread"])
        formants = [[f * scale, bw] for f, bw in part["formants"]]
        detune = 2 ** (rng.uniform(-1, 1) * p["detune"] / 1200)
        onset = int(rng.uniform(0, p["spread"]) * RATE)
        singer = glottal_singer(
            p, n, rng, velocity, frequency * detune, formants, onset
        )
        x += singer / max(np.std(singer), 1e-12)
    return x / np.sqrt(p["voices"])


def pizzicato(p, n, rng, velocity, frequency):
    """Plucked bowed-string instruments: short string ring through a full body."""
    note = 69 + 12 * np.log2(frequency / 440)
    # Lower strings ring longer; the left hand damps higher stopped notes sooner.
    ring = dict(p, decay=p["decay"] * (261.6 / frequency) ** 0.5, body=[], body_mix=0)
    treble = smoothstep(note, 48, 64)
    t = np.arange(n) / RATE
    x = np.zeros(n)
    for cents, delay, gain in p["players"]:
        tone = string(ring, n, rng, velocity, frequency * 2 ** (cents / 1200))
        thump = filtered(rng.normal(0, 1, n), [80, 1500], "bandpass") * np.exp(
            -t / 0.012
        )
        tone += (
            p["thump"] * velocity * thump * np.max(np.abs(tone)) / np.max(np.abs(thump))
        )
        bodies = []
        for name in ("low_body", "high_body"):
            freqs, decays, gains = body_modes(p[name], round(delay * 1000))
            bodies.append(resonators(tone, freqs, decays, gains))
        voiced = tone * p["direct"] + (1 - treble) * bodies[0] + treble * bodies[1]
        x += gain * (shift(voiced, round(delay * RATE)) if delay else voiced)
    return x


def courses(p, n, rng, velocity, frequency):
    """Several plucked strings sounding one note: harpsichord choirs or a pizzicato section."""
    x = np.zeros(n)
    for cents, delay, gain in p["courses"]:
        tone = string(p, n, rng, velocity, frequency * 2 ** (cents / 1200))
        x += gain * shift(tone, int(delay * RATE)) if delay else gain * tone
    return x


def timpani(p, n, rng, velocity, frequency):
    """Felt mallet on a tuned kettle: near-harmonic (n,1) modes and a quickly damped (0,1) thump."""
    ratios = np.array(
        p.get("ratios", [0.82, 1, 1.504, 1.98, 2.44, 2.9, 3.36, 3.8, 1.67, 2.23])
    )
    decays = np.array(
        p.get("decays", [0.09, 1, 0.62, 0.4, 0.27, 0.19, 0.14, 0.1, 0.12, 0.09])
    )
    gains = np.array(
        p.get("gains", [0.8, 1, 0.55, 0.34, 0.22, 0.13, 0.08, 0.05, 0.1, 0.07])
    )
    gains = gains * np.concatenate(
        [[1], velocity ** (np.arange(len(gains) - 1) * 0.35)]
    )
    contact_time = p.get("contact", 0.0026) / (0.6 + velocity * 0.7)
    force = contact(n, contact_time, rng, 0.3 + 0.5 * velocity)
    frequencies = frequency * ratios * rng.uniform(0.999, 1.001, len(ratios))
    x = resonators(force, frequencies, decays * p.get("decay", 2.4), gains)
    t = np.arange(n) / RATE
    # Hard strokes raise head tension momentarily; the pitch settles downward.
    read = np.cumsum(1 + p.get("bend", 0.006) * velocity * np.exp(-t / 0.05))
    x = np.interp(read, np.arange(n), x, right=0)
    x += (
        p.get("felt", 0.02)
        * filtered(rng.normal(0, 1, n), [200, 3000], "bandpass")
        * np.exp(-t / 0.006)
    )
    return body(x, [frequency * 0.5, 180, 420], 0.12)


def plate(p, n, rng, velocity):
    """Dense metal plate (tam-tam, suspended cymbal) with sharp, long modes.

    The strike excites the plate directly. A slower seeded force then feeds the
    upper modes, standing in for the nonlinear energy flow that makes the sound
    bloom. Because it drives resonances rather than noise bands, the bloom
    shimmers instead of hissing.
    """
    count = p["modes"]
    low, high = p["low"], p["high"]
    freqs = np.geomspace(low, high, count) * rng.uniform(0.98, 1.02, count)
    decays = (
        p["decay"]
        * rng.uniform(0.7, 1.3, count)
        / (freqs / low) ** p.get("damping", 0.3)
    )
    gains = rng.uniform(0.4, 1, count) * (freqs / low) ** p.get("tilt", -0.5)
    t = np.arange(n) / RATE
    # A felt mallet is a short, smooth push of random detail: modes start with
    # unrelated phases instead of summing into one coherent click.
    length = max(8, round(p["contact"] / (0.6 + 0.6 * velocity) * RATE))
    force = np.zeros(n)
    force[:length] = rng.normal(0, 1, length) * np.hanning(length)
    force = filtered(force, p["mallet"] * (0.5 + velocity))
    strike = resonators(force, freqs, decays, gains)
    # Neighbouring modes respond together while the mallet is in contact; a
    # short rise keeps that from cracking like a stick.
    strike *= (1 - np.exp(-t / p["strike_rise"])) ** 2
    strike /= max(np.sqrt(np.mean(strike[:RATE] ** 2)), 1e-12)
    rise = p["bloom_rise"] / (0.5 + velocity)
    flow = (
        rng.normal(0, 1, n)
        * (1 - np.exp(-t / rise)) ** 2
        * np.exp(-t / p["bloom_decay"])
    )
    upper = freqs > p["bloom_low"]
    bloom = resonators(flow, freqs[upper], decays[upper], gains[upper])
    bloom /= max(np.sqrt(np.mean(bloom[:RATE] ** 2)), 1e-12)
    return strike + p["bloom"] * (0.3 + velocity) * bloom


def ring(p, n, rng, velocity):
    """Sparse struck rod (triangle): mode pairs that beat slowly and ring long."""
    freqs = np.asarray(p["freqs"], dtype=float)
    split = p.get("split", 0.003)
    pairs = np.concatenate([freqs * (1 - split), freqs * (1 + split)])
    pairs *= rng.uniform(0.999, 1.001, len(pairs))
    gains = np.tile(p["gains"], 2) * 0.5
    x = resonators(
        contact(n, p["contact"], rng, 0.9), pairs, np.tile(p["decays"], 2), gains
    )
    t = np.arange(n) / RATE
    tick = filtered(rng.normal(0, 1, n), 6000, "highpass") * np.exp(-t / 0.0008)
    x += p.get("tick", 0.0) * velocity * tick * np.max(np.abs(x)) / np.max(np.abs(tick))
    return filtered(x, p.get("hipass", 500), "highpass")


def snare(p, n, rng, velocity):
    """Stick on a tight batter head with crisp wires against the snare head."""
    t = np.arange(n) / RATE
    ratios = np.array([1, 1.59, 2.14, 2.3, 2.65, 2.92])
    frequencies = p["freq"] * ratios * rng.uniform(0.995, 1.005, len(ratios))
    force = contact(n, p["contact"] / (0.6 + 0.6 * velocity), rng, 0.8)
    head = resonators(
        force, frequencies, p["decay"] / ratios**0.6, [1, 0.6, 0.4, 0.3, 0.2, 0.15]
    )
    head /= max(np.max(np.abs(head)), 1e-12)
    wire_decay = p["wire_decay"] * (0.75 + 0.5 * velocity)
    wires = filtered(rng.normal(0, 1, n), [p["wire_low"], p["wire_high"]], "bandpass")
    # The wires follow the snare head after the batter head drives it.
    wires *= (1 - np.exp(-t / 0.0012)) * np.exp(-t / wire_decay)
    wires /= max(np.max(np.abs(wires)), 1e-12)
    click = filtered(rng.normal(0, 1, n), 3500, "highpass") * np.exp(-t / 0.0012)
    click /= max(np.max(np.abs(click)), 1e-12)
    return (
        p["head"] * head
        + p["wires"] * (0.6 + 0.4 * velocity) * wires
        + p["click"] * velocity * click
    )


def organ(p, n, rng, velocity, frequency):
    """Pipe ranks speak separately, each with its own tuning, speed and tone."""
    response = spectral_envelope(p)
    x = np.zeros(n)
    for index, (multiple, gain, slope) in enumerate(p["stops"]):
        cents = rng.uniform(-1, 1) * p.get("stop_detune", 0)
        rank = dict(
            p,
            slope=[slope, slope],
            # Small high pipes speak faster than large low ones.
            attack=[a * multiple**-0.35 for a in p["attack"]],
            noise=p["noise"] if index == 0 else [0, 0, 0.03],
        )
        pitch = frequency * multiple * 2 ** (cents / 1200)
        x += gain * sustained_voice(rank, n, rng, velocity, pitch, response)
    return x


def clack(p, n, rng, velocity):
    """Castanets: two hollow shells meeting within a few milliseconds."""
    force = contact(n, p.get("contact", 0.00025), rng, 0.9)
    force += shift(
        force, int(rng.uniform(*p.get("gap", [0.0008, 0.0025])) * RATE)
    ) * rng.uniform(0.35, 0.8)
    freqs = np.array(p.get("freqs", [2150, 3350, 4800, 6400])) * rng.uniform(
        0.98, 1.02, 4
    )
    x = resonators(
        force,
        freqs,
        p.get("decays", [0.012, 0.008, 0.005, 0.0035]),
        p.get("gains", [1, 0.7, 0.4, 0.2]),
    )
    t = np.arange(n) / RATE
    x += (
        p.get("noise", 0.03)
        * velocity
        * filtered(rng.normal(0, 1, n), [1500, 12000], "bandpass")
        * np.exp(-t / 0.0012)
    )
    return filtered(x, p.get("hipass", 700), "highpass")


# Expansion models: drum machines, world and band instruments, vintage keys.
# Each is new; the models above keep their exact output.


def contour(n, points):
    """Piecewise-linear curve through `[seconds, value]` points, held past the ends."""
    times, values = np.asarray(points, dtype=float).T
    return np.interp(np.arange(n) / RATE, times, values)


def glide(x, cents):
    """Repitch a rendered tone along a curve in cents by moving its read position."""
    read = np.cumsum(2 ** (cents / 1200)) - 2 ** (cents[0] / 1200)
    return np.interp(read, np.arange(x.shape[-1]), x, right=0)


def normalized(x):
    return x / max(np.max(np.abs(x)), 1e-12)


def square_wave(n, frequency, rng):
    """Band-limited square: odd harmonics below 20 kHz, random starting phase."""
    t = np.arange(n) / RATE
    phase = rng.uniform(0, 2 * np.pi)
    x = np.zeros(n)
    k = 1
    while k * frequency < 20000:
        x += np.sin(k * (2 * np.pi * frequency * t + phase)) / k
        k += 2
    return x


def machine(p, n, rng, velocity, frequency=None):
    """Analog drum-machine voice: a swept sine, extra tones, click, filtered
    noise and clusters of square oscillators, each with its own decay."""
    t = np.arange(n) / RATE
    base = frequency or p.get("freq", 50)
    x = np.zeros(n)
    if p.get("tone", 0):
        sweep = p.get("sweep", 0) * (0.6 + 0.4 * velocity)
        f = base * (1 + sweep * np.exp(-t / p.get("sweep_time", 0.03)))
        phase = 2 * np.pi * np.cumsum(f) / RATE
        hold, decay = p.get("hold", 0), p.get("decay", 0.4)
        envelope = np.exp(-np.maximum(t - hold, 0) / decay)
        tone = np.sin(phase) * envelope
        if p.get("tone_drive"):
            tone = np.tanh(tone * p["tone_drive"]) / np.tanh(p["tone_drive"])
        x += p["tone"] * tone
        for ratio, gain, partial_decay in p.get("partials", []):
            x += gain * np.sin(ratio * phase) * np.exp(-t / partial_decay)
    if p.get("squares"):
        cluster = sum(square_wave(n, f, rng) for f in p["squares"])
        cluster = filtered(cluster, p["square_band"], "bandpass", order=p.get("square_order", 2))
        fast, slow, mix = p.get("square_decay", [0.05, 0.3, 0.3])
        envelope = (1 - mix) * np.exp(-t / fast) + mix * np.exp(-t / slow)
        x += p.get("square_level", 1) * normalized(cluster) * envelope
    if p.get("noise", 0):
        noise = filtered(rng.normal(0, 1, n), p.get("noise_band", [1000, 10000]), "bandpass")
        envelope = (1 - np.exp(-t / 0.0005)) * np.exp(-t / p.get("noise_decay", 0.15))
        x += p["noise"] * (0.6 + 0.4 * velocity) * normalized(noise) * envelope
    if p.get("click", 0):
        click = filtered(rng.normal(0, 1, n), p.get("click_band", [1500, 12000]), "bandpass")
        x += p["click"] * velocity * normalized(click) * np.exp(-t / p.get("click_decay", 0.0012))
    return x


def machine_tuned(p, n, rng, velocity, frequency):
    return machine(p, n, rng, velocity, frequency)


def drum(p, n, rng, velocity, frequency=None):
    """Membrane, tuned to the key when pitched, with an optional pitch contour
    in cents (a talking drum squeezed after the stroke)."""
    x = membrane(dict(p, freq=frequency or p["freq"]), n, rng, velocity)
    if p.get("contour"):
        x = glide(x, contour(n, p["contour"]) * (0.5 + 0.5 * velocity))
    return x


def tuned_drum(p, n, rng, velocity, frequency):
    return drum(p, n, rng, velocity, frequency)


def modes(p, n, rng, velocity, frequency=None):
    """Struck modal body with explicit mode ratios: bars, bells, pans, wood.

    `split` doubles each mode into a slowly beating pair. Low notes ring longer
    by `decay_tracking`. A short `tick` adds the mallet or plectrum contact.
    """
    base = frequency or p["freq"]
    ratios = np.asarray(p["ratios"], dtype=float)
    gains = np.asarray(p["gains"], dtype=float)
    gains = gains * velocity ** (np.arange(len(gains)) * p.get("velocity_tilt", 0.3))
    if "decays" in p:
        decays = np.asarray(p["decays"], dtype=float) * p.get("decay", 1)
    else:
        decays = p.get("decay", 1) / ratios ** p.get("damping", 0.75)
    if frequency and p.get("decay_tracking"):
        decays = decays * (261.63 / frequency) ** p["decay_tracking"]
    freqs = base * ratios
    if p.get("split"):
        # Unequal halves beat without cancelling completely, as a real bowl does.
        freqs = np.concatenate([freqs * (1 - p["split"]), freqs * (1 + p["split"])])
        gains, decays = np.concatenate([gains * 0.65, gains * 0.35]), np.tile(decays, 2)
    freqs = freqs * rng.uniform(1 - p.get("scatter", 0), 1 + p.get("scatter", 0), len(freqs))
    contact_time = p.get("contact", 0.001) / (0.6 + 0.6 * velocity)
    x = resonators(contact(n, contact_time, rng, p.get("hardness", velocity)), freqs, decays, gains)
    t = np.arange(n) / RATE
    if p.get("tick", 0):
        tick = filtered(rng.normal(0, 1, n), p.get("tick_band", [2000, 12000]), "bandpass")
        x += p["tick"] * velocity * np.max(np.abs(x)) * normalized(tick) * np.exp(-t / p.get("tick_decay", 0.002))
    if p.get("contour"):
        x = glide(x, contour(n, p["contour"]))
    return x


def modes_tuned(p, n, rng, velocity, frequency):
    return modes(p, n, rng, velocity, frequency)


def layered(p, n, rng, velocity, frequency=None):
    """Sum of other models, each peak-normalized, scaled and optionally delayed.

    A pitched layer follows the key times its `ratio`, plays a fixed MIDI
    `note`, or plays its own `freq` in an unpitched instrument. A `contour`
    bends the whole sum.
    """
    x = np.zeros(n)
    for layer in p["layers"]:
        model = globals()[layer["model"]]
        if layer["model"] in PITCHED:
            # `note` is a fixed pitch, such as a drone that ignores the key.
            if "note" in layer:
                pitch = hz(layer["note"])
            else:
                pitch = frequency * layer.get("ratio", 1) if frequency else layer["freq"]
            y = model(layer["params"], n, rng, velocity, pitch)
        else:
            y = model(layer["params"], n, rng, velocity)
        y = normalized(y) * layer.get("gain", 1)
        x += shift(y, round(layer["delay"] * RATE)) if layer.get("delay") else y
    if p.get("contour"):
        x = glide(x, contour(n, p["contour"]))
    return x


def layered_tuned(p, n, rng, velocity, frequency):
    return layered(p, n, rng, velocity, frequency)


def buzz(x, p, n):
    """Curved-bridge buzz (jawari, sawari): the string grazing its bridge folds
    energy into a bright band that sinks while the string rings."""
    t = np.arange(n) / RATE
    envelope = np.sqrt(np.maximum(filtered(x * x, 30), 0)) + 1e-9 * np.max(np.abs(x))
    clipped = np.clip(p.get("buzz_drive", 3) * x / (1.414 * envelope), -1, 1)
    high, low = p.get("buzz_bands", [[2500, 9000], [900, 3500]])
    sink = np.exp(-t / p.get("buzz_sink", 0.6))
    band = filtered(clipped, high, "bandpass") * sink + filtered(clipped, low, "bandpass") * (1 - sink)
    return band * envelope * 1.414


def plucked(p, n, rng, velocity, frequency):
    """Plucked strings: courses through the string loop, optional bridge buzz,
    sympathetic strings, plectrum or skin thump, an amplifier and a pitch contour."""
    x = np.zeros(n)
    for cents, delay, gain in p.get("courses", [[0, 0, 1]]):
        tone = string(p, n, rng, velocity, frequency * 2 ** (cents / 1200))
        x += gain * (shift(tone, round(delay * RATE)) if delay else tone)
    t = np.arange(n) / RATE
    peak = np.max(np.abs(x))
    if p.get("buzz", 0):
        x += p["buzz"] * buzz(x, p, n)
    if p.get("sympathetic", 0):
        # Unplucked strings tuned to fixed pitches ring when a partial matches.
        freqs = [hz(m) * k for m in p["sympathetic_notes"] for k in (1, 2, 3)]
        ringing = resonators(x, freqs, [p.get("sympathetic_decay", 1.5)] * len(freqs), [1] * len(freqs))
        x += p["sympathetic"] * peak * normalized(ringing)
    if p.get("thump", 0):
        thump = filtered(rng.normal(0, 1, n), p.get("thump_band", [150, 2500]), "bandpass")
        x += p["thump"] * velocity * peak * normalized(thump) * np.exp(-t / p.get("thump_decay", 0.01))
    if p.get("amp"):
        # Overdriven amplifier, then the speaker cabinet's narrow band.
        drive, cabinet = p["amp"]
        x = np.tanh(drive * normalized(x))
        x = filtered(filtered(x, cabinet, order=4), 90, "highpass")
    if p.get("contour"):
        x = glide(x, contour(n, p["contour"]))
    return x


def tonewheel(p, n, rng, velocity, frequency):
    """Drawbar organ: near-sine ranks, a decaying percussion harmonic and key click."""
    x = organ(p, n, rng, velocity, frequency)
    t = np.arange(n) / RATE
    level = np.std(x[n // 4 :]) * 1.414
    ratio, gain, decay = p.get("percussion", [3, 0, 0.3])
    if gain:
        x += gain * level * np.sin(2 * np.pi * frequency * ratio * t) * np.exp(-t / decay)
    if p.get("click", 0):
        click = filtered(rng.normal(0, 1, n), [800, 9000], "bandpass")
        x += p["click"] * level * normalized(click) * np.exp(-t / 0.004)
    return x


def prepared(p, n, rng, velocity, frequency):
    """Prepared piano: a damped piano string with a bolt's inharmonic modes and
    a rattle that buzzes while the string is loud."""
    x = piano(p, n, rng, velocity, frequency)
    t = np.arange(n) / RATE
    peak = np.max(np.abs(x))
    bolt_rng = np.random.Generator(np.random.PCG64(p["bolt_seed"] + round(frequency)))
    ratios = np.asarray(p["bolt_ratios"]) * bolt_rng.uniform(0.97, 1.03, len(p["bolt_ratios"]))
    force = contact(n, 0.0006, rng, 0.9)
    bolt = resonators(force, frequency * ratios, [p["bolt_decay"]] * len(ratios), [1] * len(ratios))
    x += p["bolt"] * peak * normalized(bolt)
    envelope = np.sqrt(np.maximum(filtered(x * x, 20), 0))
    rattle = filtered(rng.normal(0, 1, n), [1500, 7000], "bandpass") * (rng.random(n) < 0.3)
    x += p["rattle"] * normalized(rattle) * envelope
    return x


def drone(p, n, rng, velocity, frequency):
    """Lip-buzzed drone pipe (didgeridoo): a buzzy source whose vocal-tract
    shape the player moves between two vowels."""
    source = sustained_voice(p, n, rng, velocity, frequency, lambda f: np.ones_like(f))
    spectrum = np.fft.rfft(source)
    f = np.maximum(np.fft.rfftfreq(n, 1 / RATE), 1.0)
    floor = p.get("floor", 0.05)
    shapes = [
        np.fft.irfft(spectrum * resonance_sum(f, p[name], floor), n)
        for name in ("shape_a", "shape_b")
    ]
    t = np.arange(n) / RATE
    # A whole number of vowel cycles per sustain loop keeps the loop seamless.
    mix = 0.5 + 0.5 * np.sin(2 * np.pi * p.get("wah_rate", 1.25) * t + rng.uniform(0, 2 * np.pi))
    mix = mix * smoothstep(t, 0.15, 0.6)
    return shapes[0] * (1 - mix) + shapes[1] * mix


def hurdy(p, n, rng, velocity, frequency):
    """Hurdy-gurdy: a wheel-bowed melody string over drone strings at fixed
    pitches `[[midi, gain], ...]`, and the buzzing trompette bridge."""
    x = bowed(p, n, rng, velocity, frequency)
    for drone_note, gain in p.get("drones", []):
        x += gain * bowed(dict(p, vibrato=[5, 0, 0, 1, 1]), n, rng, velocity, hz(drone_note))
    if p.get("buzz", 0):
        x += p["buzz"] * buzz(x, p, n)
    return x


def friction(p, n, rng, velocity):
    """Friction drum (cuíca): a rubbed stick drives the head; the squeak glides
    with the thumb's pressure on the skin."""
    t = np.arange(n) / RATE
    cents = contour(n, p["contour"])
    f = p["freq"] * 2 ** (cents / 1200)
    phase = 2 * np.pi * np.cumsum(f) / RATE
    envelope = contour(n, p["envelope"]) * (1 + 0.15 * wander(n, rng, 30))
    x = sum(np.sin(k * phase) / k ** p.get("slope", 1.5) for k in range(1, 9) if k * f[0] < 16000)
    x = resonators(x * envelope, p.get("skin", [450, 1100, 2300]), [0.004] * 3, [1, 0.6, 0.3]) + 0.4 * x * envelope
    rub = filtered(rng.normal(0, 1, n), [600, 5000], "bandpass") * envelope
    return x + p.get("rub", 0.1) * np.std(x) * normalized(rub) * 3


def scrape(p, n, rng, velocity):
    """Scraped gourd (guiro): a stick crossing ridges sends a train of impacts
    into the hollow body. Each `[start, seconds, ridges per second]` is a stroke."""
    force = np.zeros(n)
    t = np.arange(n) / RATE
    for start, seconds, rate in p["strokes"]:
        times = start + np.cumsum(rng.uniform(0.85, 1.15, int(seconds * rate) + 1) / rate)
        for at in times[times < start + seconds]:
            index = int(at * RATE)
            if index < n:
                shape = np.sin(np.pi * min(1, (at - start) / seconds)) ** 0.5
                force[index] += shape * rng.uniform(0.6, 1.0)
    force = filtered(force, 9000)
    x = resonators(force, p["body"], [p.get("ring", 0.006)] * len(p["body"]), [1] * len(p["body"]))
    gesture = filtered(np.abs(force) > 0, 40)
    x += p.get("rasp", 0.3) * np.max(np.abs(x)) * normalized(filtered(rng.normal(0, 1, n), [1500, 9000], "bandpass")) * gesture
    return filtered(x, p.get("hipass", 300), "highpass")


def chimes(p, n, rng, velocity):
    """Mark tree: a hand sweeps a row of small rods, each a beating mode pair."""
    count = p["count"]
    freqs = np.geomspace(p["low"], p["high"], count)
    if p.get("direction", "down") == "down":
        freqs = freqs[::-1]
    # The hand slows a little along the row.
    onsets = p["sweep"] * (np.arange(count) / count) ** 1.25
    x = np.zeros(n)
    for f, at in zip(freqs, onsets):
        force = np.zeros(n)
        index = int((at + rng.uniform(0, 0.006)) * RATE)
        force[index : index + 6] = np.hanning(6) * rng.uniform(0.6, 1) * (0.5 + 0.5 * velocity)
        modes_hz = np.array([f * 0.998, f * 1.002, f * 2.76, f * 5.4])
        x += resonators(force, modes_hz, p["decay"] * np.array([1, 1, 0.45, 0.2]), [0.5, 0.5, 0.2, 0.06])
    return filtered(x, 1500, "highpass")
