//! Thin graph adapters over the shared acoustic DSP; no second synthesis law.
use super::{EngineError, NodeInputs, Scalar, text};
use crate::{
    acoustics::{Excitation, ExcitationKind, Exciter, ModalBank, Profile},
    model::{GraphNode, Project, Unit},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) struct ExciterConfig {
    pub(super) identity: u64,
    kind: ExcitationKind,
    contact: Scalar,
    hardness: Scalar,
    rate: Scalar,
    duration: Scalar,
    attack: Scalar,
    level: Scalar,
}
#[derive(Debug, Clone)]
pub(super) struct ExciterState {
    pub(super) dsp: Exciter,
    gate: bool,
}
impl ExciterState {
    pub(super) fn new(rate: u32, seed: u64) -> Self {
        Self {
            dsp: Exciter::new(rate, seed),
            gate: false,
        }
    }
    pub(super) fn reset(&mut self, seed: u64) {
        self.dsp.reset(seed);
        self.gate = false;
    }
}
impl ExciterConfig {
    pub(super) fn compile(
        node: &GraphNode,
        identity: u64,
        number: impl Fn(&str, Unit, f64) -> Result<Scalar, EngineError>,
    ) -> Result<Self, EngineError> {
        let kind = match text(node, "kind", "impact") {
            "impact" => ExcitationKind::Impact,
            "collisions" => ExcitationKind::Collisions,
            "texture" => ExcitationKind::Texture,
            _ => return Err(EngineError::Compile("unsupported exciter kind".into())),
        };
        Ok(Self {
            identity,
            kind,
            contact: number("contact", Unit::Seconds, 0.0001)?,
            hardness: number("hardness", Unit::Ratio, 0.5)?,
            rate: number("rate", Unit::Hertz, 400.0)?,
            duration: number("duration", Unit::Seconds, 0.1)?,
            attack: number("attack", Unit::Seconds, 0.0)?,
            level: number("level", Unit::Ratio, 1.0)?,
        })
    }
    pub(super) fn compatible(&self, other: &Self) -> bool {
        self.kind == other.kind && self.identity == other.identity
    }
    pub(super) fn process(
        &self,
        state: &mut ExciterState,
        inputs: NodeInputs,
        controls: &[f64],
    ) -> f32 {
        let gate = inputs.gate.control() >= 0.5;
        if gate && !state.gate {
            let setting = Excitation {
                kind: self.kind,
                contact_seconds: self.contact.value(controls).clamp(0.00002, 0.02),
                hardness: self.hardness.value(controls).clamp(0.0, 1.0),
                rate_hz: self.rate.value(controls).clamp(1.0, 10000.0),
                duration_seconds: self.duration.value(controls).clamp(0.005, 2.0),
                attack_seconds: self.attack.value(controls).clamp(0.0, 1.0),
            };
            state.dsp.trigger(
                setting,
                if inputs.has_velocity {
                    f64::from(inputs.velocity.control())
                } else {
                    1.0
                },
            );
        }
        state.gate = gate;
        (state.dsp.process() * self.level.value(controls).clamp(0.0, 4.0)) as f32
    }
}

#[derive(Debug, Clone)]
pub(super) struct BankConfig {
    pub(super) profile: Arc<Profile>,
    decay: Scalar,
    brightness: Scalar,
    key_track: Scalar,
    rise: Scalar,
    level: Scalar,
}
#[derive(Debug, Clone)]
pub(super) struct BankState {
    pub(super) dsp: ModalBank,
    gate: bool,
}
impl BankState {
    pub(super) fn new(modes: usize, rate: u32) -> Self {
        Self {
            dsp: ModalBank::new(modes, rate),
            gate: false,
        }
    }
    pub(super) fn reset(&mut self) {
        self.dsp.reset();
        self.gate = false;
    }
}
impl BankConfig {
    pub(super) fn compile(
        node: &GraphNode,
        project: &Project,
        max_modes: usize,
        number: impl Fn(&str, Unit, f64) -> Result<Scalar, EngineError>,
    ) -> Result<Self, EngineError> {
        let id = text(node, "profile", "");
        let profile = project
            .acoustic_profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| EngineError::Compile(format!("missing acoustic profile `{id}`")))?;
        profile.body.validate().map_err(EngineError::Compile)?;
        if profile.body.modes.len() > max_modes {
            return Err(EngineError::Compile(
                "profile exceeds modal_bank max_modes".into(),
            ));
        }
        Ok(Self {
            profile: Arc::new(profile.body.clone()),
            decay: number("decay_scale", Unit::Ratio, 1.0)?,
            brightness: number("brightness", Unit::Ratio, 0.0)?,
            key_track: number("key_track", Unit::Ratio, 1.0)?,
            rise: number("rise", Unit::Seconds, 0.0)?,
            level: number("level", Unit::Ratio, 1.0)?,
        })
    }
    pub(super) fn compatible(&self, other: &Self) -> bool {
        self.profile == other.profile
    }
    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>()
            + std::mem::size_of::<Profile>()
            + self.profile.modes.capacity() * std::mem::size_of::<crate::acoustics::Mode>()
            + 2 * std::mem::size_of::<usize>()) as u64
    }
    pub(super) fn process(
        &self,
        state: &mut BankState,
        inputs: NodeInputs,
        controls: &[f64],
    ) -> f32 {
        let gate = inputs.gate.control() >= 0.5;
        if gate && !state.gate {
            let ratio = (f64::from(inputs.pitch.control()).max(1.0) / self.profile.root_hz)
                .powf(self.key_track.value(controls).clamp(0.0, 1.0));
            state.dsp.trigger_with_rise(
                &self.profile,
                ratio,
                self.decay.value(controls),
                self.brightness.value(controls),
                self.rise.value(controls).clamp(0.0, 1.0),
            );
        }
        state.gate = gate;
        (state.dsp.process(f64::from(inputs.audio.mono()))
            * self.level.value(controls).clamp(0.0, 4.0)) as f32
    }
}
