use super::Scalar;

pub(super) const MAX_RELEASE_SECONDS: f64 = 8.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct EnvelopeConfig {
    pub(super) attack: Scalar,
    pub(super) decay: Scalar,
    pub(super) sustain: Scalar,
    pub(super) release: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EnvelopeStage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct EnvelopeState {
    stage: EnvelopeStage,
    level: f64,
    last_gate: bool,
    release_step: f64,
}

impl EnvelopeState {
    pub(super) fn process(
        &mut self,
        gate: bool,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        sample_rate: f64,
    ) -> f64 {
        if gate && !self.last_gate {
            self.stage = EnvelopeStage::Attack;
        } else if !gate && self.last_gate && self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
            self.release_step = self.level / (release * sample_rate).max(1.0);
        }
        self.last_gate = gate;
        match self.stage {
            EnvelopeStage::Idle => self.level = 0.0,
            EnvelopeStage::Attack => {
                self.level += 1.0 / (attack * sample_rate).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                self.level -= (1.0 - sustain) / (decay * sample_rate).max(1.0);
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => self.level = sustain,
            EnvelopeStage::Release => {
                self.level -= self.release_step.max(0.000_000_001);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
            }
        }
        self.level
    }

    pub(super) fn idle(self) -> bool {
        self.stage == EnvelopeStage::Idle
    }
}

impl EnvelopeConfig {
    pub(super) fn process(
        self,
        state: &mut EnvelopeState,
        gate: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        state.process(
            gate >= 0.5,
            self.attack.value(controls).max(0.0),
            self.decay.value(controls).max(0.0),
            self.sustain.value(controls).clamp(0.0, 1.0),
            self.release.value(controls).clamp(0.0, MAX_RELEASE_SECONDS),
            sample_rate,
        ) as f32
    }
}
