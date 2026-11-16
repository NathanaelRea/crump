use super::Scalar;

pub(super) const MAX_RELEASE_SECONDS: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnvelopeRetrigger {
    Always,
    Legato,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EnvelopeConfig {
    pub(super) attack: Scalar,
    pub(super) hold: Scalar,
    pub(super) decay: Scalar,
    pub(super) sustain: Scalar,
    pub(super) release: Scalar,
    pub(super) attack_curve: Scalar,
    pub(super) decay_curve: Scalar,
    pub(super) release_curve: Scalar,
    pub(super) retrigger: EnvelopeRetrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EnvelopeStage {
    #[default]
    Idle,
    Attack,
    Hold,
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
    segment_origin: f64,
    segment_position: f64,
    hold_frames_remaining: u64,
    note_retrigger_pending: bool,
}

impl EnvelopeState {
    /// Records a selected-note change while the voice gate remains high.
    pub(super) fn note_retrigger(&mut self) {
        self.note_retrigger_pending = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        gate: bool,
        attack: f64,
        hold: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        attack_curve: f64,
        decay_curve: f64,
        release_curve: f64,
        retrigger: EnvelopeRetrigger,
        sample_rate: f64,
    ) -> f64 {
        if std::mem::take(&mut self.note_retrigger_pending)
            && gate
            && retrigger == EnvelopeRetrigger::Always
        {
            self.level = 0.0;
            self.stage = EnvelopeStage::Attack;
            self.segment_origin = 0.0;
            self.segment_position = 0.0;
            self.hold_frames_remaining = 0;
        }
        if gate && !self.last_gate {
            if retrigger == EnvelopeRetrigger::Always {
                self.level = 0.0;
            }
            self.stage = EnvelopeStage::Attack;
            self.segment_origin = self.level;
            self.segment_position = 0.0;
        } else if !gate && self.last_gate && self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
            self.release_step = self.level / (release * sample_rate).max(1.0);
            self.segment_origin = self.level;
            self.segment_position = 0.0;
        }
        self.last_gate = gate;
        match self.stage {
            EnvelopeStage::Idle => self.level = 0.0,
            EnvelopeStage::Attack => {
                if attack_curve == 0.0 {
                    self.level += 1.0 / (attack * sample_rate).max(1.0);
                } else {
                    self.segment_position =
                        (self.segment_position + 1.0 / (attack * sample_rate).max(1.0)).min(1.0);
                    self.level = self.segment_origin
                        + (1.0 - self.segment_origin) * curve(self.segment_position, attack_curve);
                }
                if self.level >= 1.0 || self.segment_position >= 1.0 {
                    self.level = 1.0;
                    self.hold_frames_remaining =
                        (hold.max(0.0) * sample_rate).round().max(0.0) as u64;
                    if self.hold_frames_remaining == 0 {
                        self.enter_decay();
                    } else {
                        self.stage = EnvelopeStage::Hold;
                    }
                }
            }
            EnvelopeStage::Hold => {
                self.level = 1.0;
                self.hold_frames_remaining = self.hold_frames_remaining.saturating_sub(1);
                if self.hold_frames_remaining == 0 {
                    self.enter_decay();
                }
            }
            EnvelopeStage::Decay => {
                if decay_curve == 0.0 {
                    self.level -= (1.0 - sustain) / (decay * sample_rate).max(1.0);
                } else {
                    self.segment_position =
                        (self.segment_position + 1.0 / (decay * sample_rate).max(1.0)).min(1.0);
                    self.level = 1.0 - (1.0 - sustain) * curve(self.segment_position, decay_curve);
                }
                if self.level <= sustain || self.segment_position >= 1.0 {
                    self.level = sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => self.level = sustain,
            EnvelopeStage::Release => {
                if release_curve == 0.0 {
                    self.level -= self.release_step.max(0.000_000_001);
                } else {
                    self.segment_position =
                        (self.segment_position + 1.0 / (release * sample_rate).max(1.0)).min(1.0);
                    self.level =
                        self.segment_origin * (1.0 - curve(self.segment_position, release_curve));
                }
                if self.level <= 0.0 || self.segment_position >= 1.0 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
            }
        }
        self.level
    }

    fn enter_decay(&mut self) {
        self.stage = EnvelopeStage::Decay;
        self.segment_origin = 1.0;
        self.segment_position = 0.0;
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
            self.hold.value(controls).max(0.0),
            self.decay.value(controls).max(0.0),
            self.sustain.value(controls).clamp(0.0, 1.0),
            self.release.value(controls).clamp(0.0, MAX_RELEASE_SECONDS),
            self.attack_curve.value(controls).clamp(-1.0, 1.0),
            self.decay_curve.value(controls).clamp(-1.0, 1.0),
            self.release_curve.value(controls).clamp(-1.0, 1.0),
            self.retrigger,
            sample_rate,
        ) as f32
    }
}

fn curve(position: f64, amount: f64) -> f64 {
    position
        .clamp(0.0, 1.0)
        .powf(2.0_f64.powf(-amount.clamp(-1.0, 1.0) * 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ahdsr_hold_curves_and_zero_duration_boundaries_are_exact() {
        let mut state = EnvelopeState::default();
        let values = (0..7)
            .map(|_| {
                state.process(
                    true,
                    0.0,
                    2.0,
                    2.0,
                    0.5,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    EnvelopeRetrigger::Legato,
                    1.0,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(values, [1.0, 1.0, 1.0, 0.75, 0.5, 0.5, 0.5]);

        let mut curved = EnvelopeState::default();
        let fast = curved.process(
            true,
            4.0,
            0.0,
            1.0,
            1.0,
            1.0,
            1.0,
            0.0,
            0.0,
            EnvelopeRetrigger::Legato,
            1.0,
        );
        assert!(fast > 0.25);
    }

    #[test]
    fn always_retrigger_restarts_while_legato_retrigger_retains_level() {
        let mut always = EnvelopeState::default();
        let mut legato = EnvelopeState::default();
        for state in [&mut always, &mut legato] {
            let _ = state.process(
                true,
                4.0,
                0.0,
                1.0,
                1.0,
                4.0,
                0.0,
                0.0,
                0.0,
                EnvelopeRetrigger::Legato,
                1.0,
            );
            let _ = state.process(
                false,
                4.0,
                0.0,
                1.0,
                1.0,
                4.0,
                0.0,
                0.0,
                0.0,
                EnvelopeRetrigger::Legato,
                1.0,
            );
        }
        let restarted = always.process(
            true,
            4.0,
            0.0,
            1.0,
            1.0,
            4.0,
            0.0,
            0.0,
            0.0,
            EnvelopeRetrigger::Always,
            1.0,
        );
        let retained = legato.process(
            true,
            4.0,
            0.0,
            1.0,
            1.0,
            4.0,
            0.0,
            0.0,
            0.0,
            EnvelopeRetrigger::Legato,
            1.0,
        );
        assert_eq!(restarted, 0.25);
        assert!(retained > restarted);
    }

    #[test]
    fn selected_note_retrigger_obeys_node_policy_while_gate_stays_high() {
        let mut always = EnvelopeState::default();
        let mut legato = EnvelopeState::default();
        for state in [&mut always, &mut legato] {
            assert_eq!(
                state.process(
                    true,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    EnvelopeRetrigger::Legato,
                    4.0,
                ),
                1.0
            );
            state.note_retrigger();
        }
        let restarted = always.process(
            true,
            1.0,
            0.0,
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            EnvelopeRetrigger::Always,
            4.0,
        );
        let retained = legato.process(
            true,
            1.0,
            0.0,
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            EnvelopeRetrigger::Legato,
            4.0,
        );
        assert_eq!(restarted, 0.25);
        assert_eq!(retained, 1.0);
    }
}
