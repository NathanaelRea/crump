//! Deterministic stock synthesized percussion with no waveform assets.

use std::f64::consts::TAU;

use crate::model::{DrumVoice, Instrument};

use super::schedule::ClipEnvelope;

/// Preallocated stock-percussion runtime.
#[derive(Debug, Clone)]
pub(crate) struct DrumSynth {
    voices: Vec<DrumState>,
    sample_rate: f64,
}

impl DrumSynth {
    pub(crate) fn voice_state_bytes(polyphony: u16) -> u64 {
        u64::from(polyphony).saturating_mul(std::mem::size_of::<DrumState>() as u64)
    }

    pub(crate) fn new(instrument: &Instrument, sample_rate: u32) -> Self {
        let voices = (0..instrument.polyphony)
            .map(|_| DrumState::default())
            .collect();
        Self {
            voices,
            sample_rate: f64::from(sample_rate),
        }
    }

    pub(crate) fn trigger(
        &mut self,
        voice: DrumVoice,
        velocity: f32,
        seed: u64,
        clip_envelope: ClipEnvelope,
    ) {
        let slot = self
            .voices
            .iter()
            .position(|state| !state.active)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, state)| state.age_frames)
                    .map_or(0, |(index, _)| index)
            });
        self.voices[slot] = DrumState {
            active: true,
            voice,
            velocity: velocity.clamp(0.0, 1.0),
            age_frames: 0,
            phase: seed_phase(seed),
            rng: nonzero_seed(seed),
            lowpass: 0.0,
            clip_envelope,
        };
    }

    pub(crate) fn process(&mut self, tone: f64, punch: f64) -> [f32; 2] {
        let tone = tone.clamp(0.0, 1.0);
        let punch = punch.clamp(0.0, 1.0);
        let mut mono = 0.0_f64;
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let time = voice.age_frames as f64 / self.sample_rate;
            let sample = match voice.voice {
                DrumVoice::Kick => kick(voice, time, self.sample_rate, punch),
                DrumVoice::Snare => snare(voice, time, self.sample_rate, tone),
                DrumVoice::Hat => hat(voice, time, tone),
                DrumVoice::Clap => clap(voice, time, tone),
            };
            mono += sample
                * f64::from(voice.velocity)
                * f64::from(voice.clip_envelope.gain_at(voice.age_frames));
            voice.age_frames = voice.age_frames.saturating_add(1);
            let maximum = match voice.voice {
                DrumVoice::Kick => 1.2,
                DrumVoice::Snare => 0.8,
                DrumVoice::Hat => 0.35,
                DrumVoice::Clap => 0.7,
            };
            if time >= maximum {
                voice.active = false;
            }
        }
        let sample = (mono * 0.75).tanh() as f32;
        [sample * 0.707_106_77, sample * 0.707_106_77]
    }

    pub(crate) fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }

    #[cfg(test)]
    pub(crate) fn allocated_voice_state_bytes(&self) -> u64 {
        (self.voices.capacity() as u64).saturating_mul(std::mem::size_of::<DrumState>() as u64)
    }
}

#[derive(Debug, Clone, Copy)]
struct DrumState {
    active: bool,
    voice: DrumVoice,
    velocity: f32,
    age_frames: u64,
    phase: f64,
    rng: u64,
    lowpass: f64,
    clip_envelope: ClipEnvelope,
}

impl Default for DrumState {
    fn default() -> Self {
        Self {
            active: false,
            voice: DrumVoice::Kick,
            velocity: 0.0,
            age_frames: 0,
            phase: 0.0,
            rng: 1,
            lowpass: 0.0,
            clip_envelope: ClipEnvelope::TRANSPARENT,
        }
    }
}

fn kick(state: &mut DrumState, time: f64, sample_rate: f64, punch: f64) -> f64 {
    let frequency = 43.0 + (95.0 + punch * 55.0) * (-time * 28.0).exp();
    state.phase = (state.phase + frequency / sample_rate).fract();
    let body = (state.phase * TAU).sin();
    let envelope = (-time * (7.0 + (1.0 - punch) * 5.0)).exp();
    let click = if time < 0.006 {
        next_noise(state) * (1.0 - time / 0.006) * 0.12
    } else {
        0.0
    };
    body * envelope + click
}

fn snare(state: &mut DrumState, time: f64, sample_rate: f64, tone: f64) -> f64 {
    let noise = next_noise(state);
    let cutoff = 0.03 + tone * 0.22;
    state.lowpass += cutoff * (noise - state.lowpass);
    let high = noise - state.lowpass;
    let noise_envelope = (-time * (13.0 + tone * 8.0)).exp();
    let frequency = 165.0 + tone * 65.0;
    state.phase = (state.phase + frequency / sample_rate).fract();
    let body = (state.phase * TAU).sin() * (-time * 20.0).exp();
    high * noise_envelope * 0.8 + body * 0.35
}

fn hat(state: &mut DrumState, time: f64, tone: f64) -> f64 {
    let noise = next_noise(state);
    let cutoff = 0.08 + tone * 0.28;
    state.lowpass += cutoff * (noise - state.lowpass);
    let metallic = noise - state.lowpass;
    metallic * (-time * (35.0 + tone * 35.0)).exp() * 0.75
}

fn clap(state: &mut DrumState, time: f64, tone: f64) -> f64 {
    let noise = next_noise(state);
    state.lowpass += (0.08 + tone * 0.18) * (noise - state.lowpass);
    let high = noise - state.lowpass;
    let burst = [0.0, 0.018, 0.036]
        .into_iter()
        .map(|start| {
            let local = time - start;
            if (0.0..0.035).contains(&local) {
                (-local * 70.0).exp()
            } else {
                0.0
            }
        })
        .sum::<f64>();
    let tail = if time >= 0.04 {
        (-(time - 0.04) * 14.0).exp() * 0.5
    } else {
        0.0
    };
    high * (burst + tail) * 0.65
}

fn next_noise(state: &mut DrumState) -> f64 {
    state.rng = xorshift(state.rng);
    let normalized = (state.rng >> 11) as f64 / (1_u64 << 53) as f64;
    normalized * 2.0 - 1.0
}

const fn xorshift(mut value: u64) -> u64 {
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    value
}

const fn nonzero_seed(seed: u64) -> u64 {
    if seed == 0 {
        0x517c_c1b7_2722_0a95
    } else {
        seed
    }
}

fn seed_phase(seed: u64) -> f64 {
    (nonzero_seed(seed) >> 11) as f64 / (1_u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::dsl::parse_project;

    use super::*;

    #[test]
    fn stock_drums_are_deterministic_and_finite() {
        let source = r#"crump 1
project "Drum" {
  id drum
  seed 1
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.6
      parameter punch "Punch" ratio 0 1 0.5 0.7
      faceplate {
        control tone "Tone" "Brightness" maps tone
        control punch "Punch" "Kick impact" maps punch
      }
    }
    pattern beat 1bar {
      hit 0beat kick 1
    }
    track drums "Drums" instrument drums level 0db pan 0 {
      clip once pattern beat start 0bar duration 1bar mode once
    }
  }
}
"#;
        let document = parse_project(Path::new("project.crump"), source);
        let instrument = &document.project.unwrap().song.instruments[0];
        let mut first = DrumSynth::new(instrument, 48_000);
        let mut second = DrumSynth::new(instrument, 48_000);
        first.trigger(DrumVoice::Snare, 0.8, 4, ClipEnvelope::TRANSPARENT);
        second.trigger(DrumVoice::Snare, 0.8, 4, ClipEnvelope::TRANSPARENT);
        for _ in 0..1_000 {
            let a = first.process(0.55, 0.65);
            let b = second.process(0.55, 0.65);
            assert_eq!(a, b);
            assert!(a.into_iter().all(f32::is_finite));
        }
    }
}
