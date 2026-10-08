//! Shared acoustic excitation and damped-body DSP for native graphs and offline fitting.
//!
//! Profiles describe resonances, not recorded waveforms or time-frequency frames.
//! All storage is prepared before processing; sample processing never allocates.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Maximum resonances in one body.
pub const MAX_MODES: usize = 256;
/// Version of the profile's units and modal response law.
pub const PROFILE_VERSION: u32 = 1;

/// One damped resonance, optionally with a second, slower decay component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    /// Resonance at the profile root, in hertz.
    pub frequency_hz: f64,
    /// Amplitude e-folding time, in seconds (not T60).
    pub decay_seconds: f64,
    /// Impulse-response amplitude.
    pub gain: f64,
    /// Aftersound amplitude e-folding time, in seconds.
    pub after_decay_seconds: f64,
    /// Aftersound impulse-response amplitude.
    pub after_gain: f64,
}

/// Versioned, portable numerical acoustic-body description.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Explicit response-law version.
    pub version: u32,
    /// Reference pitch for key tracking, in hertz.
    pub root_hz: f64,
    /// Ordered resonances; order is part of deterministic identity.
    pub modes: Vec<Mode>,
}

impl Profile {
    /// Reject unsupported versions, unstable parameters, and excess work before allocation.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != PROFILE_VERSION {
            return Err("unsupported acoustic profile version".into());
        }
        if !finite_range(self.root_hz, 8.0, 20000.0) || !(1..=MAX_MODES).contains(&self.modes.len())
        {
            return Err("profile requires a root in 8..20000 Hz and 1..256 modes".into());
        }
        for mode in &self.modes {
            if !finite_range(mode.frequency_hz, 18.0, 22000.0)
                || !finite_range(mode.decay_seconds, 0.0001, 12.0)
                || !finite_range(mode.after_decay_seconds, 0.0001, 12.0)
                || !finite_range(mode.gain, 0.0, 4.0)
                || !finite_range(mode.after_gain, 0.0, 4.0)
            {
                return Err("mode frequency, decay or gain is outside its finite domain".into());
            }
        }
        let gain: f64 = self.modes.iter().map(|m| m.gain + m.after_gain).sum();
        if !(0.0..=64.0).contains(&gain) || gain == 0.0 {
            return Err("total modal gain must be positive and at most 64".into());
        }
        Ok(())
    }
}

fn finite_range(value: f64, low: f64, high: f64) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}

/// Contact event law. Collisions drive the same body with a decaying gesture.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExcitationKind {
    /// One finite contact pulse.
    Impact,
    /// A deterministic sequence of finite contact pulses.
    Collisions,
    /// Centered random force with a smooth rise and exponential decay.
    Texture,
}

/// Physical-unit settings for one note's excitation, latched on trigger.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Excitation {
    /// Contact or collision gesture.
    pub kind: ExcitationKind,
    /// Contact duration at full velocity, in seconds.
    pub contact_seconds: f64,
    /// Pulse concentration in 0..1; greater values concentrate force near the center.
    pub hardness: f64,
    /// Initial collision rate, in hertz.
    pub rate_hz: f64,
    /// Collision gesture amplitude e-folding time, in seconds.
    pub duration_seconds: f64,
    /// Collision gesture rise time constant. Zero preserves the immediate strike.
    #[serde(default)]
    pub attack_seconds: f64,
}

impl Excitation {
    /// Validate all physical controls, including controls inactive for this event kind.
    pub fn validate(&self) -> Result<(), String> {
        if !finite_range(self.contact_seconds, 0.00002, 0.02)
            || !finite_range(self.hardness, 0.0, 1.0)
            || !finite_range(self.rate_hz, 1.0, 10000.0)
            || !finite_range(self.duration_seconds, 0.005, 2.0)
            || !finite_range(self.attack_seconds, 0.0, 1.0)
        {
            return Err("excitation settings outside their finite physical domains".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Pulse {
    age: u32,
    frames: u32,
    gain: f64,
}

/// Fixed-state contact generator. Sixteen concurrent contacts bound collision work.
#[derive(Debug, Clone)]
pub struct Exciter {
    pulses: [Pulse; 16],
    seed: u64,
    age: u64,
    velocity: f64,
    setting: Excitation,
    sample_rate: f64,
    running: bool,
    gesture_peak: f64,
}

impl Exciter {
    /// Prepare fixed storage. Call `trigger` before rendering a gesture.
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        Self {
            pulses: [Pulse::default(); 16],
            seed: seed.max(1),
            age: 0,
            velocity: 0.0,
            setting: Excitation {
                kind: ExcitationKind::Impact,
                contact_seconds: 0.0001,
                hardness: 0.5,
                rate_hz: 100.0,
                duration_seconds: 0.1,
                attack_seconds: 0.0,
            },
            sample_rate: f64::from(sample_rate.max(1)),
            running: false,
            gesture_peak: 1.0,
        }
    }
    /// Clear contacts and restore a deterministic note seed.
    pub fn reset(&mut self, seed: u64) {
        self.pulses.fill(Pulse::default());
        self.seed = seed.max(1);
        self.age = 0;
        self.running = false;
    }
    /// Whether an existing gesture can still emit contacts, including quiet gaps.
    pub fn is_active(&self) -> bool {
        self.running
    }
    /// Begin a contact gesture. Settings must have passed `validate`.
    pub fn trigger(&mut self, setting: Excitation, velocity: f64) {
        self.setting = setting;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.age = 0;
        self.pulses.fill(Pulse::default());
        self.running = true;
        self.gesture_peak = if setting.attack_seconds > 0.0
            && (2.0 * setting.duration_seconds / setting.attack_seconds).is_finite()
        {
            let power = if setting.kind == ExcitationKind::Texture {
                2
            } else {
                1
            };
            let peak_time = setting.attack_seconds
                * (1.0 + f64::from(power) * setting.duration_seconds / setting.attack_seconds).ln();
            (1.0 - (-peak_time / setting.attack_seconds).exp()).powi(power)
                * (-peak_time / setting.duration_seconds).exp()
        } else {
            // As the rise approaches zero its peak approaches one. This also
            // keeps valid subnormal rise times from overflowing the ratio.
            1.0
        };
        if setting.kind == ExcitationKind::Impact
            || (setting.kind == ExcitationKind::Collisions && setting.attack_seconds == 0.0)
        {
            self.start_pulse(self.velocity);
        }
    }
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / ((1_u64 << 53) as f64)
    }
    fn start_pulse(&mut self, gain: f64) {
        let frames = (self.setting.contact_seconds * self.sample_rate / (0.6 + 0.4 * self.velocity))
            .round()
            .max(2.0) as u32;
        // At overload replace the quietest contact; never allocate or extend the pool.
        let index = self
            .pulses
            .iter()
            .position(|p| p.age >= p.frames)
            .unwrap_or_else(|| {
                self.pulses
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.gain.total_cmp(&b.1.gain))
                    .map_or(0, |(i, _)| i)
            });
        self.pulses[index] = Pulse {
            age: 0,
            frames,
            gain,
        };
    }
    /// Generate the next mono force sample. No waveform or reference input exists.
    pub fn process(&mut self) -> f64 {
        if !self.running {
            return 0.0;
        }
        let time = self.age as f64 / self.sample_rate;
        if self.setting.kind == ExcitationKind::Texture {
            let mut envelope = (-time / self.setting.duration_seconds).exp();
            if self.setting.attack_seconds > 0.0 {
                envelope *=
                    (-(-time / self.setting.attack_seconds).exp_m1()).powi(2) / self.gesture_peak;
            }
            let out = (2.0 * self.random() - 1.0) * self.velocity * envelope;
            self.age += 1;
            self.running =
                time < (self.setting.duration_seconds + self.setting.attack_seconds) * 8.0;
            return out;
        }
        if self.setting.kind == ExcitationKind::Collisions
            && time < (self.setting.duration_seconds + self.setting.attack_seconds) * 8.0
        {
            let mut envelope = (-time / self.setting.duration_seconds).exp();
            if self.setting.attack_seconds > 0.0 {
                envelope *= -(-time / self.setting.attack_seconds).exp_m1() / self.gesture_peak;
            }
            if self.random() < self.setting.rate_hz / self.sample_rate * envelope {
                let gain = self.velocity * envelope * (0.35 + 0.65 * self.random());
                self.start_pulse(gain);
            }
        }
        let mut out = 0.0;
        for p in &mut self.pulses {
            if p.age < p.frames {
                let position = (p.age as f64 + 0.5) / p.frames as f64;
                // Discrete midpoint means: sin^4 has mean 1/4 for a two-frame
                // pulse, 3/8 otherwise. Normalize both ends of the hardness blend.
                let s = (PI * position).sin().powi(2);
                let fourth_scale = if p.frames == 2 { 4.0 } else { 8.0 / 3.0 };
                let force = (1.0 - self.setting.hardness) * s * 2.0
                    + self.setting.hardness * s * s * fourth_scale;
                out += p.gain * force / p.frames as f64;
                p.age += 1;
            }
        }
        self.age += 1;
        self.running = self.pulses.iter().any(|p| p.age < p.frames)
            || (self.setting.kind == ExcitationKind::Collisions
                && time < (self.setting.duration_seconds + self.setting.attack_seconds) * 8.0);
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Resonance {
    a1: f64,
    a2: f64,
    b: f64,
    y1: f64,
    y2: f64,
}
impl Resonance {
    fn configure(&mut self, f: f64, tau: f64, gain: f64, rate: f64) {
        let w = 2.0 * PI * f / rate;
        let r = (-1.0 / (rate * tau)).exp();
        *self = Self {
            a1: 2.0 * r * w.cos(),
            a2: -r * r,
            b: gain * w.sin(),
            y1: 0.0,
            y2: 0.0,
        };
    }
    fn process(&mut self, input: f64) -> f64 {
        let y = self.b * input + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = if y.abs() < 1e-30 { 0.0 } else { y };
        y
    }
}

/// Prepared two-stage modal bank. Coefficients latch on note onset.
#[derive(Debug, Clone)]
pub struct ModalBank {
    modes: Vec<[Resonance; 2]>,
    sample_rate: f64,
    rise_pole: f64,
    rise_state: f64,
}
impl ModalBank {
    /// Allocate fixed state off the audio callback.
    pub fn new(mode_count: usize, sample_rate: u32) -> Self {
        Self {
            modes: vec![[Resonance::default(); 2]; mode_count.min(MAX_MODES)],
            sample_rate: f64::from(sample_rate.max(1)),
            rise_pole: 0.0,
            rise_state: 0.0,
        }
    }
    /// Clear a released or replaced voice without allocation.
    pub fn reset(&mut self) {
        self.modes.fill([Resonance::default(); 2]);
        self.rise_pole = 0.0;
        self.rise_state = 0.0;
    }
    /// Configure a validated profile at a pitch ratio; decay and tilt are bounded macros.
    pub fn trigger(
        &mut self,
        profile: &Profile,
        pitch_ratio: f64,
        decay_scale: f64,
        brightness: f64,
    ) {
        self.trigger_with_rise(profile, pitch_ratio, decay_scale, brightness, 0.0);
    }
    /// Start a body with an optional smooth note-relative emergence envelope.
    /// This shapes radiation amplitude, not the contact force or modal poles.
    /// Zero rise preserves the original response exactly; no added latency.
    pub fn trigger_with_rise(
        &mut self,
        profile: &Profile,
        pitch_ratio: f64,
        decay_scale: f64,
        brightness: f64,
        rise_seconds: f64,
    ) {
        self.reset();
        if rise_seconds > 0.0 {
            self.rise_pole = (-1.0 / (self.sample_rate * rise_seconds.clamp(0.000001, 1.0))).exp();
            self.rise_state = 1.0;
        }
        let ratio = pitch_ratio.clamp(0.03125, 32.0);
        let decay = decay_scale.clamp(0.1, 4.0);
        let tilt = brightness.clamp(-2.0, 2.0);
        for (state, mode) in self.modes.iter_mut().zip(&profile.modes) {
            let f = mode.frequency_hz * ratio;
            if f < 18.0 || f >= self.sample_rate * 0.43 {
                continue;
            }
            let gain = (f / 1000.0).powf(tilt).min(16.0);
            state[0].configure(
                f,
                (mode.decay_seconds * decay).clamp(0.0001, 12.0),
                mode.gain * gain,
                self.sample_rate,
            );
            state[1].configure(
                f,
                (mode.after_decay_seconds * decay).clamp(0.0001, 12.0),
                mode.after_gain * gain,
                self.sample_rate,
            );
        }
    }
    /// Process one force sample through the body; both decay components share excitation.
    pub fn process(&mut self, input: f64) -> f64 {
        let output: f64 = self
            .modes
            .iter_mut()
            .map(|s| s[0].process(input) + s[1].process(input))
            .sum();
        if self.rise_state > 0.0 {
            let amplitude = (1.0 - self.rise_state).powi(2);
            self.rise_state *= self.rise_pole;
            if self.rise_state < 1e-15 {
                self.rise_state = 0.0;
            }
            output * amplitude
        } else {
            output
        }
    }
    /// Heap storage charged to runtime admission, including prepared coefficient state.
    pub fn allocation_bytes(&self) -> u64 {
        (self.modes.capacity() * std::mem::size_of::<[Resonance; 2]>()) as u64
    }
    /// Conservative state storage for an authored mode ceiling.
    pub fn state_bytes(mode_count: usize) -> u64 {
        (std::mem::size_of::<Self>() + mode_count * std::mem::size_of::<[Resonance; 2]>()) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile {
        Profile {
            version: 1,
            root_hz: 440.0,
            modes: vec![Mode {
                frequency_hz: 440.0,
                decay_seconds: 0.2,
                gain: 1.0,
                after_decay_seconds: 1.0,
                after_gain: 0.1,
            }],
        }
    }
    #[test]
    fn body_rise_delays_energy_without_delaying_or_changing_the_resonance() {
        let mut p = profile();
        p.modes[0].decay_seconds = 0.02;
        p.modes[0].after_gain = 0.0;
        let mut immediate = ModalBank::new(1, 48000);
        let mut emerging = ModalBank::new(1, 48000);
        immediate.trigger(&p, 1.0, 1.0, 0.0);
        emerging.trigger_with_rise(&p, 1.0, 1.0, 0.0, 0.004);
        let mut first = 0.0;
        let mut body = 0.0;
        for i in 0..9600 {
            let force = if i == 0 { 1.0 } else { 0.0 };
            let raw = immediate.process(force);
            let output = emerging.process(force);
            let expected = raw * (1.0 - (-(i as f64) / 48000.0 / 0.004).exp()).powi(2);
            assert!((output - expected).abs() < 1e-12);
            if i < 96 {
                first += output * output;
            }
            if (384..480).contains(&i) {
                body += output * output;
            }
        }
        assert!(body > first * 8.0);
        emerging.reset();
        assert_eq!(emerging.process(0.0), 0.0);
    }
    #[test]
    fn texture_force_is_centered_repeatable_bounded_and_rises_before_decaying() {
        let setting = Excitation {
            kind: ExcitationKind::Texture,
            contact_seconds: 0.0001,
            hardness: 0.5,
            rate_hz: 400.0,
            duration_seconds: 0.025,
            attack_seconds: 0.004,
        };
        let mut a = Exciter::new(48000, 17);
        let mut b = Exciter::new(48000, 17);
        a.trigger(setting, 0.8);
        b.trigger(setting, 0.8);
        assert!(a.is_active());
        let mut bins = [0.0; 200];
        let mut sum = 0.0;
        let allocations = allocation_counter::measure(|| {
            for i in 0..9600 {
                let x = a.process();
                assert_eq!(x, b.process());
                assert!(x.is_finite() && x.abs() <= 0.8);
                bins[i / 48] += x * x;
                sum += x;
            }
            a.reset(17);
            a.trigger(setting, 0.8);
        });
        assert_eq!(allocations.count_total, 0);
        let peak = bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert!((7..18).contains(&peak), "peak at {peak} ms");
        assert!(bins[0] < bins[peak] * 0.01);
        assert!(bins[150] < bins[peak] * 0.001);
        assert!(sum.abs() / 9600.0 < 0.004);
        assert_eq!(a.process(), 0.0);
        for attack_seconds in [0.0, f64::MIN_POSITIVE, 1.0] {
            a.trigger(
                Excitation {
                    attack_seconds,
                    ..setting
                },
                1.0,
            );
            for _ in 0..1000 {
                assert!(a.process().is_finite());
            }
        }
    }
    #[test]
    fn rejects_invalid_profiles() {
        let mut p = profile();
        assert!(p.validate().is_ok());
        p.modes[0].gain = f64::NAN;
        assert!(p.validate().is_err());
        p = profile();
        p.version = 2;
        assert!(p.validate().is_err());
    }

    #[test]
    fn contact_force_integral_tracks_velocity_across_durations_and_hardness() {
        for contact_seconds in [0.00002, 0.0001, 0.003, 0.02] {
            for hardness in [0.0, 0.5, 1.0] {
                let mut e = Exciter::new(48000, 91);
                e.trigger(
                    Excitation {
                        kind: ExcitationKind::Impact,
                        contact_seconds,
                        hardness,
                        rate_hz: 400.0,
                        duration_seconds: 0.1,
                        attack_seconds: 0.0,
                    },
                    0.7,
                );
                let total: f64 = (0..2000).map(|_| e.process()).sum();
                assert!((total - 0.7).abs() < 1e-12);
            }
        }
    }
    #[test]
    fn decay_and_frequency_follow_the_profile() {
        let p = profile();
        let mut bank = ModalBank::new(1, 48000);
        bank.trigger(&p, 1.0, 1.0, 0.0);
        for i in 0..48000 {
            let t = (i + 1) as f64 / 48000.0;
            let actual = bank.process(if i == 0 { 1.0 } else { 0.0 });
            let expected = (2.0 * PI * 440.0 * t).sin()
                * ((-i as f64 / (48000.0 * 0.2)).exp() + 0.1 * (-i as f64 / 48000.0).exp());
            assert!((actual - expected).abs() < 1e-9);
        }
    }
    #[test]
    fn contact_and_collisions_are_repeatable_allocation_free_and_prefix_stable() {
        let setting = Excitation {
            kind: ExcitationKind::Collisions,
            contact_seconds: 0.0002,
            hardness: 0.4,
            rate_hz: 450.0,
            duration_seconds: 0.1,
            attack_seconds: 0.08,
        };
        let p = profile();
        let mut a = Exciter::new(48000, 71);
        let mut b = Exciter::new(48000, 71);
        let mut bank = ModalBank::new(1, 48000);
        a.trigger(setting, 0.8);
        b.trigger(setting, 0.8);
        bank.trigger(&p, 1.0, 1.0, 0.0);
        let allocations = allocation_counter::measure(|| {
            for _ in 0..96000 {
                let x = a.process();
                assert_eq!(x, b.process());
                assert!(bank.process(x).is_finite());
            }
            a.reset(71);
            a.trigger(setting, 0.8);
            bank.trigger(&p, 2.0, 0.8, 0.5);
        });
        assert_eq!(allocations.count_total, 0);
    }

    #[test]
    fn collision_attack_rises_without_a_forced_initial_hit() {
        let mut e = Exciter::new(48000, 71029);
        let setting = Excitation {
            kind: ExcitationKind::Collisions,
            contact_seconds: 0.00002,
            hardness: 0.4,
            rate_hz: 6000.0,
            duration_seconds: 0.12,
            attack_seconds: 0.10,
        };
        assert!(setting.validate().is_ok());
        e.trigger(setting, 0.8);
        assert_eq!(e.process(), 0.0);
        let mut energy = [0.0; 100];
        for bin in &mut energy {
            for _ in 0..480 {
                *bin += e.process().powi(2);
            }
        }
        let peak = energy.iter().copied().fold(0.0_f64, f64::max);
        assert!(energy[0] < peak * 0.05);
        assert!(energy[6..12].iter().any(|v| *v > peak * 0.7));
        assert!(energy[60..].iter().sum::<f64>() < peak * 0.01);
        let legacy: Excitation = serde_json::from_str(
            r#"{"kind":"collisions","contact_seconds":0.00002,"hardness":0.4,"rate_hz":6000.0,"duration_seconds":0.12}"#,
        ).unwrap();
        assert_eq!(legacy.attack_seconds, 0.0);
        e.trigger(legacy, 0.8);
        assert!(e.process() > 0.0);
        e.trigger(
            Excitation {
                attack_seconds: f64::from_bits(1),
                ..setting
            },
            0.8,
        );
        assert!((0..48000).all(|_| e.process().is_finite()));
    }
}
