//! Typed channel-tap Envelope Followers with explicit one-sample control latency.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    model::{EnvelopeDetector, ModulationOperator, ParameterTarget, Project, TapPoint, Unit},
    validate::MAX_MODULATION_ROUTES,
};

use super::{
    EngineError,
    control::{CompiledControlProgram, ControlIndex, ControlRuntime},
    mix_graph::{CompiledMixGraph, MixGraphRuntime},
};

/// Fixed maximum telemetry entries, aligned to validated route capacity.
pub const MAX_ENVELOPE_FOLLOWERS: usize = MAX_MODULATION_ROUTES;

/// Static inspectable contract for one compiled Envelope Follower route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvelopeFollowerInspection {
    /// Dense telemetry index.
    pub index: usize,
    /// Durable route identity.
    pub id: String,
    /// Stable source channel identity.
    pub source: String,
    /// Exact source tap.
    pub tap: TapPoint,
    /// Linked-stereo detector law.
    pub detector: EnvelopeDetector,
    /// Detector attack in seconds.
    pub attack_seconds: f64,
    /// Detector release in seconds.
    pub release_seconds: f64,
    /// Input level mapping to normalized zero.
    pub floor_db: f64,
    /// Input level mapping to normalized one.
    pub ceiling_db: f64,
    /// Stable physical destination.
    pub target: ParameterTarget,
    /// Destination physical unit.
    pub unit: Unit,
    /// Physical contribution at normalized one.
    pub depth: f64,
    /// Route-local normalized arithmetic.
    pub operators: Vec<ModulationOperator>,
    /// Audio from frame N contributes to controls at frame N+1.
    pub control_latency_frames: u32,
}

/// Low-cost runtime value aligned to [`EnvelopeFollowerInspection::index`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct EnvelopeFollowerMeter {
    /// Smoothed normalized detector output after route-local operators.
    pub normalized: f32,
    /// Physical contribution scheduled for the following sample.
    pub contribution: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct CompiledEnvelopeFollowerRoute {
    inspection: EnvelopeFollowerInspection,
    source_index: usize,
    target: ControlIndex,
}

/// Immutable follower route program compiled away from the callback.
#[derive(Debug, Clone, Default)]
pub(crate) struct CompiledEnvelopeFollowerProgram {
    routes: Vec<CompiledEnvelopeFollowerRoute>,
    sample_rate: u32,
}

impl CompiledEnvelopeFollowerProgram {
    pub(crate) fn compile(
        project: &Project,
        controls: &CompiledControlProgram,
        mix_graph: &CompiledMixGraph,
        sample_rate: u32,
    ) -> Result<Self, EngineError> {
        let channels = &mix_graph.inspection().channels;
        let mut authored = project.song.envelope_followers.iter().collect::<Vec<_>>();
        authored.sort_by(|left, right| left.id.cmp(&right.id));
        let mut routes = Vec::with_capacity(authored.len());
        for (index, route) in authored.into_iter().enumerate() {
            let source = route.source.to_string();
            let source_index = channels
                .iter()
                .find(|channel| channel.identity == source)
                .map(|channel| channel.index)
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "Envelope Follower `{}` source `{source}` does not exist",
                        route.id
                    ))
                })?;
            let target = controls.index(&route.target).ok_or_else(|| {
                EngineError::Compile(format!(
                    "Envelope Follower `{}` target `{}` is unavailable",
                    route.id, route.target
                ))
            })?;
            let spec = project.parameter_spec(&route.target).ok_or_else(|| {
                EngineError::Compile(format!(
                    "Envelope Follower `{}` target `{}` has no Parameter contract",
                    route.id, route.target
                ))
            })?;
            let depth = spec.unit.parse_value(&route.depth).map_err(|error| {
                EngineError::Compile(format!(
                    "Envelope Follower `{}` depth is invalid: {error}",
                    route.id
                ))
            })?;
            routes.push(CompiledEnvelopeFollowerRoute {
                inspection: EnvelopeFollowerInspection {
                    index,
                    id: route.id.clone(),
                    source,
                    tap: route.tap,
                    detector: route.detector,
                    attack_seconds: route.attack_seconds,
                    release_seconds: route.release_seconds,
                    floor_db: route.floor_db,
                    ceiling_db: route.ceiling_db,
                    target: route.target.clone(),
                    unit: spec.unit,
                    depth,
                    operators: route.operators.clone(),
                    control_latency_frames: 1,
                },
                source_index,
                target,
            });
        }
        Ok(Self {
            routes,
            sample_rate,
        })
    }

    pub(crate) fn runtime(self: &Arc<Self>) -> EnvelopeFollowerRuntime {
        EnvelopeFollowerRuntime {
            program: self.clone(),
            states: vec![EnvelopeFollowerState::default(); self.routes.len()],
        }
    }

    pub(crate) fn inspection(&self) -> Vec<EnvelopeFollowerInspection> {
        self.routes
            .iter()
            .map(|route| route.inspection.clone())
            .collect()
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.routes.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledEnvelopeFollowerRoute>() as u64)
            .saturating_add(
                self.routes
                    .iter()
                    .map(|route| {
                        (route.inspection.id.capacity() as u64)
                            .saturating_add(route.inspection.source.capacity() as u64)
                            .saturating_add(parameter_target_heap_bytes(&route.inspection.target))
                            .saturating_add(
                                (route.inspection.operators.capacity() as u64).saturating_mul(
                                    std::mem::size_of::<ModulationOperator>() as u64,
                                ),
                            )
                    })
                    .fold(0_u64, u64::saturating_add),
            )
    }

    pub(crate) fn runtime_state_bytes(&self) -> u64 {
        (self.routes.capacity() as u64)
            .saturating_mul(std::mem::size_of::<EnvelopeFollowerState>() as u64)
    }

    pub(crate) fn work_units_per_frame(&self) -> u64 {
        (self.routes.len() as u64).saturating_mul(20)
    }
}

fn parameter_target_heap_bytes(target: &ParameterTarget) -> u64 {
    match target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => instrument.capacity() as u64 + parameter.capacity() as u64,
        ParameterTarget::Track { track, .. } => track.capacity() as u64,
        ParameterTarget::Group { group, .. } => group.capacity() as u64,
        ParameterTarget::Return { r#return, .. } => r#return.capacity() as u64,
        ParameterTarget::Send { send } => send.capacity() as u64,
        ParameterTarget::Processor {
            processor,
            parameter,
        } => processor.capacity() as u64 + parameter.capacity() as u64,
        ParameterTarget::Master => 0,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct EnvelopeFollowerState {
    envelope: f64,
}

/// Preallocated mutable follower state.
pub(crate) struct EnvelopeFollowerRuntime {
    program: Arc<CompiledEnvelopeFollowerProgram>,
    states: Vec<EnvelopeFollowerState>,
}

impl EnvelopeFollowerRuntime {
    pub(crate) fn route_count(&self) -> usize {
        self.program.routes.len()
    }

    pub(crate) fn process(
        &mut self,
        mix: &MixGraphRuntime,
        controls: &mut ControlRuntime,
        meters: &mut [EnvelopeFollowerMeter; MAX_ENVELOPE_FOLLOWERS],
    ) -> usize {
        for (index, (route, state)) in self
            .program
            .routes
            .iter()
            .zip(self.states.iter_mut())
            .enumerate()
        {
            let sample = mix
                .channel_tap(route.source_index, route.inspection.tap)
                .unwrap_or([0.0; 2]);
            let detector = match route.inspection.detector {
                EnvelopeDetector::Peak => f64::from(sample[0].abs().max(sample[1].abs())),
                EnvelopeDetector::Rms => {
                    ((f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2)) * 0.5).sqrt()
                }
            };
            let seconds = if detector > state.envelope {
                route.inspection.attack_seconds
            } else {
                route.inspection.release_seconds
            };
            let coefficient =
                (-1.0 / (seconds.max(1.0e-6) * f64::from(self.program.sample_rate))).exp();
            state.envelope = if detector.is_finite() {
                detector + coefficient * (state.envelope - detector)
            } else {
                state.envelope
            };
            let level_db = 20.0 * state.envelope.max(1.0e-12).log10();
            let mut normalized = ((level_db - route.inspection.floor_db)
                / (route.inspection.ceiling_db - route.inspection.floor_db))
                .clamp(0.0, 1.0);
            for operator in &route.inspection.operators {
                normalized = match *operator {
                    ModulationOperator::Add { value } | ModulationOperator::Bias { value } => {
                        normalized + value
                    }
                    ModulationOperator::Multiply { value }
                    | ModulationOperator::Scale { value } => normalized * value,
                    ModulationOperator::Clamp { minimum, maximum } => {
                        normalized.clamp(minimum, maximum)
                    }
                };
            }
            if !normalized.is_finite() {
                normalized = 0.0;
            }
            let contribution = normalized * route.inspection.depth;
            controls.add_modulation(route.target, contribution);
            meters[index] = EnvelopeFollowerMeter {
                normalized: normalized as f32,
                contribution: contribution as f32,
            };
        }
        self.program.routes.len()
    }

    pub(crate) fn reset(&mut self) {
        self.states.fill(EnvelopeFollowerState::default());
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn adopt_compatible_state(&mut self, current: &mut Self) -> bool {
        if self.program.routes != current.program.routes
            || self.states.len() != current.states.len()
        {
            return false;
        }
        self.states.copy_from_slice(&current.states);
        true
    }

    #[cfg(test)]
    pub(crate) fn storage_fingerprint(&self) -> u64 {
        self.states.as_ptr() as usize as u64 ^ (self.states.capacity() as u64).rotate_left(31)
    }
}
