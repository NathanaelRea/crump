//! Callback-safe effective Parameter evaluation shared by playback and rendering.

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::model::{
    AutomationInterpolation, ControlPolarity, LfoWaveform, ModulationOperator, ModulationRate,
    ModulationScope, ParameterTarget, Project, Unit,
};

use super::{EngineError, schedule::compile_parameter_lock_lanes};

/// Dense callback-time index resolved from a stable authored target during compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ControlIndex(pub(crate) usize);

/// Immutable compact control program compiled away from the audio callback.
#[derive(Debug, Clone)]
pub struct CompiledControlProgram {
    targets: Arc<Vec<CompiledTarget>>,
    indexes: Arc<BTreeMap<ParameterTarget, ControlIndex>>,
}

impl CompiledControlProgram {
    /// Compiles every currently supported Parameter owner into stable dense slots.
    pub fn compile(project: &Project) -> Result<Self, EngineError> {
        let lanes: BTreeMap<_, _> = project
            .song
            .automations
            .iter()
            .map(|lane| (&lane.target, lane))
            .collect();
        let parameter_lock_lanes = compile_parameter_lock_lanes(project)?;
        let authored = project
            .parameter_specs()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        if let Some(target) = parameter_lock_lanes
            .keys()
            .find(|target| !authored.contains_key(*target))
        {
            return Err(EngineError::Compile(format!(
                "Parameter lock targets missing Parameter `{target}`"
            )));
        }

        let mut targets = Vec::with_capacity(authored.len());
        let mut indexes = BTreeMap::new();
        for (target, spec) in authored {
            let lane = lanes
                .get(&target)
                .map(|lane| {
                    lane.points
                        .iter()
                        .map(|point| {
                            let frame = point
                                .at
                                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
                            let value = spec.unit.parse_value(&point.value).map_err(|error| {
                                EngineError::Compile(format!(
                                    "Automation lane `{}` point `{}`: {error}",
                                    lane.id, point.id
                                ))
                            })?;
                            Ok(CompiledPoint {
                                frame,
                                value,
                                interpolation: point.interpolation,
                            })
                        })
                        .collect::<Result<Vec<_>, EngineError>>()
                        .map(|points| CompiledLane { points })
                })
                .transpose()?;
            let parameter_locks = parameter_lock_lanes
                .get(&target)
                .map(|points| {
                    points
                        .iter()
                        .map(|point| {
                            let value = point
                                .value
                                .as_ref()
                                .map(|value| {
                                    spec.unit.parse_value(value).map_err(|error| {
                                        EngineError::Compile(format!(
                                            "Parameter lock for `{target}` at frame {}: {error}",
                                            point.frame
                                        ))
                                    })
                                })
                                .transpose()?;
                            Ok(CompiledParameterLockPoint {
                                frame: point.frame,
                                value,
                            })
                        })
                        .collect::<Result<Vec<_>, EngineError>>()
                        .map(|points| CompiledParameterLockLane { points })
                })
                .transpose()?;
            let index = ControlIndex(targets.len());
            indexes.insert(target.clone(), index);
            let mut routes = project
                .song
                .modulations
                .iter()
                .filter(|route| route.target == target)
                .map(|route| CompiledRoute::compile(route, spec.unit, project))
                .collect::<Result<Vec<_>, _>>()?;
            routes.sort_by(|left, right| left.id.cmp(&right.id));
            targets.push(CompiledTarget {
                target,
                unit: spec.unit,
                step_only: spec.unit == Unit::Boolean,
                minimum: spec.minimum,
                maximum: spec.maximum,
                base: spec.base,
                lane,
                parameter_locks,
                routes,
            });
        }
        Ok(Self {
            targets: Arc::new(targets),
            indexes: Arc::new(indexes),
        })
    }

    /// Resolves one stable authored target away from the callback.
    #[must_use]
    pub(crate) fn index(&self, target: &ParameterTarget) -> Option<ControlIndex> {
        self.indexes.get(target).copied()
    }

    pub(crate) fn per_voice_routes(&self, instrument: &str) -> Vec<CompiledPerVoiceRoute> {
        let mut routes = Vec::new();
        for (target_index, target) in self.targets.iter().enumerate() {
            for route in &target.routes {
                if matches!(&route.scope, ModulationScope::PerVoice(owner) if owner == instrument) {
                    routes.push(CompiledPerVoiceRoute {
                        target: ControlIndex(target_index),
                        minimum: target.minimum,
                        maximum: target.maximum,
                        route: route.clone(),
                    });
                }
            }
        }
        routes.sort_by(|left, right| left.route.id.cmp(&right.route.id));
        routes
    }

    pub(crate) fn target_count(&self) -> usize {
        self.targets.len()
    }

    /// Owned immutable allocations retained by a runtime through this program.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let word = std::mem::size_of::<usize>() as u64;
        let targets = (std::mem::size_of::<Vec<CompiledTarget>>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(
                (self.targets.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<CompiledTarget>() as u64),
            )
            .saturating_add(
                self.targets
                    .iter()
                    .map(CompiledTarget::heap_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        let indexes = (std::mem::size_of::<BTreeMap<ParameterTarget, ControlIndex>>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(
                (self.indexes.len() as u64).saturating_mul(
                    (std::mem::size_of::<(ParameterTarget, ControlIndex)>() as u64)
                        .saturating_add(word.saturating_mul(4)),
                ),
            )
            .saturating_add(
                self.indexes
                    .keys()
                    .map(parameter_target_heap_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        targets.saturating_add(indexes)
    }

    /// Mutable vector backing allocated for one callback-time evaluator.
    pub(crate) fn runtime_state_bytes(&self) -> u64 {
        let targets = self.targets.len() as u64;
        targets
            .saturating_mul(4)
            .saturating_mul(std::mem::size_of::<f64>() as u64)
            .saturating_add(targets.saturating_mul(std::mem::size_of::<RuntimeProvenance>() as u64))
            .saturating_add(packed_bool_allocation_bytes(self.targets.len()))
    }

    pub(crate) fn initial_value(&self, index: ControlIndex) -> Option<f64> {
        self.targets
            .get(index.0)
            .map(|target| target.base.clamp(target.minimum, target.maximum))
    }

    /// Creates preallocated mutable effective-value state.
    #[must_use]
    pub(crate) fn runtime(&self) -> ControlRuntime {
        let effective = self
            .targets
            .iter()
            .map(|target| target.base.clamp(target.minimum, target.maximum))
            .collect::<Vec<_>>();
        ControlRuntime {
            program: self.clone(),
            transition_origin: effective.clone(),
            unconstrained: effective.clone(),
            effective,
            modulation: vec![0.0; self.targets.len()],
            provenance: vec![RuntimeProvenance::default(); self.targets.len()],
            transition_changed: vec![false; self.targets.len()],
            transition_position: 0,
            transition_frames: 0,
        }
    }

    /// Evaluates inspectable provenance at one absolute sample frame off the callback.
    #[must_use]
    pub fn snapshot_at(&self, frame: u64) -> Vec<ControlValueSnapshot> {
        let mut runtime = self.runtime();
        runtime.evaluate_frame(frame);
        self.targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let provenance = runtime.provenance[index];
                ControlValueSnapshot {
                    target: target.target.clone(),
                    unit: target.unit,
                    frame,
                    base: target.base,
                    automation: provenance.automation,
                    parameter_lock: provenance.parameter_lock,
                    modulation: provenance.modulation,
                    modulation_routes: target
                        .routes
                        .iter()
                        .map(|route| route.snapshot(frame))
                        .collect(),
                    unconstrained: provenance.unconstrained,
                    effective: runtime.effective[index],
                    clamped: provenance.clamped,
                    held_non_finite: provenance.held_non_finite,
                }
            })
            .collect()
    }
}

fn packed_bool_allocation_bytes(capacity_bits: usize) -> u64 {
    (capacity_bits.div_ceil(usize::BITS as usize) as u64)
        .saturating_mul(std::mem::size_of::<usize>() as u64)
}

fn parameter_target_heap_bytes(target: &ParameterTarget) -> u64 {
    match target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => (instrument.capacity() as u64).saturating_add(parameter.capacity() as u64),
        ParameterTarget::Track { track, .. } => track.capacity() as u64,
        ParameterTarget::Group { group, .. } => group.capacity() as u64,
        ParameterTarget::Return { r#return, .. } => r#return.capacity() as u64,
        ParameterTarget::Send { send } => send.capacity() as u64,
        ParameterTarget::Processor {
            processor,
            parameter,
        } => (processor.capacity() as u64).saturating_add(parameter.capacity() as u64),
        ParameterTarget::Master => 0,
    }
}

#[derive(Debug, Clone)]
struct CompiledTarget {
    target: ParameterTarget,
    unit: Unit,
    step_only: bool,
    minimum: f64,
    maximum: f64,
    base: f64,
    lane: Option<CompiledLane>,
    parameter_locks: Option<CompiledParameterLockLane>,
    routes: Vec<CompiledRoute>,
}

#[derive(Debug, Clone, PartialEq)]
struct CompiledRoute {
    id: String,
    scope: ModulationScope,
    polarity: ControlPolarity,
    waveform: LfoWaveform,
    phase_increment: f64,
    depth: f64,
    operators: Vec<ModulationOperator>,
}

impl CompiledTarget {
    fn heap_bytes(&self) -> u64 {
        let lane = self.lane.as_ref().map_or(0, |lane| {
            (lane.points.capacity() as u64)
                .saturating_mul(std::mem::size_of::<CompiledPoint>() as u64)
        });
        let parameter_locks = self.parameter_locks.as_ref().map_or(0, |lane| {
            (lane.points.capacity() as u64)
                .saturating_mul(std::mem::size_of::<CompiledParameterLockPoint>() as u64)
        });
        let routes = (self.routes.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledRoute>() as u64)
            .saturating_add(
                self.routes
                    .iter()
                    .map(CompiledRoute::heap_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        parameter_target_heap_bytes(&self.target)
            .saturating_add(lane)
            .saturating_add(parameter_locks)
            .saturating_add(routes)
    }
}

impl CompiledRoute {
    fn heap_bytes(&self) -> u64 {
        let scope = match &self.scope {
            ModulationScope::Global => 0,
            ModulationScope::Instrument(owner) | ModulationScope::PerVoice(owner) => {
                owner.capacity() as u64
            }
        };
        (self.id.capacity() as u64)
            .saturating_add(scope)
            .saturating_add(
                (self.operators.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<ModulationOperator>() as u64),
            )
    }

    fn compile(
        route: &crate::model::ModulationRoute,
        unit: Unit,
        project: &Project,
    ) -> Result<Self, EngineError> {
        let phase_increment = match route.rate {
            ModulationRate::FreeHertz(rate) => rate / f64::from(project.sample_rate),
            ModulationRate::Synchronized(period) => {
                let frames = period.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                if frames == 0 {
                    return Err(EngineError::Compile(format!(
                        "Modulation route `{}` has a zero-frame synchronized period",
                        route.id
                    )));
                }
                1.0 / frames as f64
            }
        };
        let depth = unit.parse_value(&route.depth).map_err(|error| {
            EngineError::Compile(format!("Modulation route `{}` depth: {error}", route.id))
        })?;
        Ok(Self {
            id: route.id.clone(),
            scope: route.scope.clone(),
            polarity: route.polarity,
            waveform: route.waveform,
            phase_increment,
            depth,
            operators: route.operators.clone(),
        })
    }

    fn normalized_at(&self, frame: u64) -> f64 {
        let phase = (frame as f64 * self.phase_increment).fract();
        let bipolar = match self.waveform {
            LfoWaveform::Sine => (phase * std::f64::consts::TAU).sin(),
            LfoWaveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        };
        let mut value = match self.polarity {
            ControlPolarity::Unipolar => (bipolar + 1.0) * 0.5,
            ControlPolarity::Bipolar => bipolar,
        };
        for operator in &self.operators {
            value = match *operator {
                ModulationOperator::Add { value: operand }
                | ModulationOperator::Bias { value: operand } => value + operand,
                ModulationOperator::Multiply { value: operand }
                | ModulationOperator::Scale { value: operand } => value * operand,
                ModulationOperator::Clamp { minimum, maximum } => value.clamp(minimum, maximum),
            };
        }
        value
    }

    fn contribution_at(&self, frame: u64) -> Option<f64> {
        (!matches!(self.scope, ModulationScope::PerVoice(_)))
            .then(|| self.normalized_at(frame) * self.depth)
    }

    fn snapshot(&self, frame: u64) -> ModulationValueSnapshot {
        ModulationValueSnapshot {
            route: self.id.clone(),
            scope: self.scope.clone(),
            normalized: self
                .contribution_at(frame)
                .map(|_| self.normalized_at(frame)),
            depth: self.depth,
            contribution: self.contribution_at(frame),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CompiledLane {
    points: Vec<CompiledPoint>,
}

impl CompiledLane {
    fn value_at(&self, frame: u64) -> Option<f64> {
        let first = self.points.first()?;
        if frame <= first.frame {
            return Some(first.value);
        }
        let current_index = self
            .points
            .partition_point(|point| point.frame <= frame)
            .saturating_sub(1);
        let current = self.points.get(current_index)?;
        let Some(next) = self.points.get(current_index + 1) else {
            return Some(current.value);
        };
        let span = next.frame.saturating_sub(current.frame);
        if span == 0 {
            return Some(next.value);
        }
        let progress = frame.saturating_sub(current.frame) as f64 / span as f64;
        Some(match current.interpolation {
            AutomationInterpolation::Step => current.value,
            AutomationInterpolation::Linear => {
                current.value + (next.value - current.value) * progress
            }
            AutomationInterpolation::Power(exponent) => {
                current.value + (next.value - current.value) * progress.powf(exponent)
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CompiledParameterLockLane {
    points: Vec<CompiledParameterLockPoint>,
}

impl CompiledParameterLockLane {
    fn value_at(&self, frame: u64) -> Option<f64> {
        let count = self.points.partition_point(|point| point.frame <= frame);
        count
            .checked_sub(1)
            .and_then(|index| self.points.get(index))
            .and_then(|point| point.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CompiledParameterLockPoint {
    frame: u64,
    value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CompiledPoint {
    frame: u64,
    value: f64,
    interpolation: AutomationInterpolation,
}

/// Preallocated mutable control evaluator used once per authored sample frame.
#[derive(Debug, Clone)]
pub(crate) struct ControlRuntime {
    program: CompiledControlProgram,
    effective: Vec<f64>,
    unconstrained: Vec<f64>,
    modulation: Vec<f64>,
    provenance: Vec<RuntimeProvenance>,
    transition_origin: Vec<f64>,
    transition_changed: Vec<bool>,
    transition_position: u64,
    transition_frames: u64,
}

impl ControlRuntime {
    /// Evaluates base → optional Automation → optional Parameter lock → additive Modulation → constraint.
    pub(crate) fn evaluate_frame(&mut self, frame: u64) {
        let transition_progress = if self.transition_position < self.transition_frames {
            self.transition_position = self.transition_position.saturating_add(1);
            Some(self.transition_position as f64 / self.transition_frames as f64)
        } else {
            None
        };
        for (index, target) in self.program.targets.iter().enumerate() {
            let automation = target.lane.as_ref().and_then(|lane| lane.value_at(frame));
            let parameter_lock = target
                .parameter_locks
                .as_ref()
                .and_then(|lane| lane.value_at(frame));
            let absolute = parameter_lock.or(automation).unwrap_or(target.base);
            let mut modulation = self.modulation[index];
            for route in &target.routes {
                if let Some(contribution) = route.contribution_at(frame) {
                    modulation += contribution;
                }
            }
            let unconstrained = absolute + modulation;
            let previous = self.effective[index];
            let (constrained, held_non_finite) = if unconstrained.is_finite() {
                (unconstrained.clamp(target.minimum, target.maximum), false)
            } else {
                (previous, true)
            };
            let effective = if held_non_finite {
                previous
            } else if let Some(progress) = transition_progress
                && self.transition_changed[index]
            {
                self.transition_origin[index]
                    + (constrained - self.transition_origin[index]) * progress
            } else {
                constrained
            };
            self.unconstrained[index] = if unconstrained.is_finite() {
                unconstrained
            } else {
                previous
            };
            self.effective[index] = effective;
            self.provenance[index] = RuntimeProvenance {
                automation,
                parameter_lock,
                modulation,
                unconstrained,
                clamped: unconstrained.is_finite() && constrained != unconstrained,
                held_non_finite,
            };
            // Routes are expected to publish a fresh contribution for every sample.
            self.modulation[index] = 0.0;
        }
    }

    /// Adopts a behavior-changing compatible program and starts a bounded publication ramp.
    ///
    /// Only targets whose compiled base/domain/lane changed are ramped; unchanged Automation
    /// remains exact. This monitoring-only cross-revision guard is not authored smoothing and
    /// never participates in playback/render evaluation within one revision.
    /// This runs on the callback at a publication boundary, but only compares and copies
    /// fixed, preallocated arrays. Program ownership is swapped without allocation.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn begin_publication_handoff(
        &mut self,
        replacement: &mut Self,
        frames: u64,
    ) -> bool {
        let compatible = self.program.targets.len() == replacement.program.targets.len()
            && self
                .program
                .targets
                .iter()
                .zip(replacement.program.targets.iter())
                .all(|(current, replacement)| current.target == replacement.target);
        if !compatible {
            return false;
        }
        let mut changed = false;
        for (index, (current, replacement)) in self
            .program
            .targets
            .iter()
            .zip(replacement.program.targets.iter())
            .enumerate()
        {
            let lane_changed = current.lane != replacement.lane;
            let unautomated_base_changed = current.lane.is_none()
                && replacement.lane.is_none()
                && current.base.to_bits() != replacement.base.to_bits();
            let target_changed = current.unit != replacement.unit
                || current.minimum.to_bits() != replacement.minimum.to_bits()
                || current.maximum.to_bits() != replacement.maximum.to_bits()
                || lane_changed
                || current.routes != replacement.routes
                || unautomated_base_changed;
            self.transition_changed[index] = target_changed && !replacement.step_only;
            changed |= target_changed;
        }
        if !changed {
            return false;
        }
        self.transition_origin.copy_from_slice(&self.effective);
        self.transition_position = 0;
        self.transition_frames = frames.max(1);
        std::mem::swap(&mut self.program, &mut replacement.program);
        true
    }

    /// Returns one already evaluated physical value by dense index.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn value(&self, index: ControlIndex) -> f64 {
        self.effective[index.0]
    }

    /// Dense physical values for graph and channel processing.
    #[must_use]
    pub(crate) fn values(&self) -> &[f64] {
        &self.effective
    }

    /// Dense finite pre-constraint values used as the base for per-voice routes.
    #[must_use]
    pub(crate) fn unconstrained_values(&self) -> &[f64] {
        &self.unconstrained
    }

    /// Adds one future modulation contribution before the next evaluation.
    #[allow(dead_code)]
    pub(crate) fn add_modulation(&mut self, index: ControlIndex, contribution: f64) {
        self.modulation[index.0] += contribution;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledPerVoiceRoute {
    target: ControlIndex,
    minimum: f64,
    maximum: f64,
    route: CompiledRoute,
}

impl CompiledPerVoiceRoute {
    pub(crate) fn allocation_bytes(&self) -> u64 {
        self.route.heap_bytes()
    }

    pub(crate) fn target(&self) -> ControlIndex {
        self.target
    }

    pub(crate) fn contribution_at(&self, voice_frame: u64) -> f64 {
        self.route.normalized_at(voice_frame) * self.route.depth
    }

    pub(crate) fn constrain(&self, value: f64, previous: f64) -> f64 {
        if value.is_finite() {
            value.clamp(self.minimum, self.maximum)
        } else {
            previous
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RuntimeProvenance {
    automation: Option<f64>,
    parameter_lock: Option<f64>,
    modulation: f64,
    unconstrained: f64,
    clamped: bool,
    held_non_finite: bool,
}

/// Inspectable effective-value provenance at an exact engine frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlValueSnapshot {
    /// Stable owner and immutable key.
    pub target: ParameterTarget,
    /// Canonical physical unit.
    pub unit: Unit,
    /// Absolute sample frame represented by this value.
    pub frame: u64,
    /// Authored base value.
    pub base: f64,
    /// Absolute Automation value, when a lane exists.
    pub automation: Option<f64>,
    /// Pattern-scoped step override after Automation, when active.
    pub parameter_lock: Option<f64>,
    /// Aggregate additive physical-unit Modulation contribution.
    pub modulation: f64,
    /// Stable authored route provenance in deterministic route-ID order.
    pub modulation_routes: Vec<ModulationValueSnapshot>,
    /// Value before domain constraint.
    pub unconstrained: f64,
    /// Final finite physical value.
    pub effective: f64,
    /// Whether domain constraint changed this sample's finite value.
    pub clamped: bool,
    /// Whether a non-finite result caused the prior finite value to be held.
    pub held_non_finite: bool,
}

/// Inspectable contribution from one authored Modulation route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModulationValueSnapshot {
    /// Durable route identity.
    pub route: String,
    /// Authored execution scope.
    pub scope: ModulationScope,
    /// Route-local normalized value, or `None` for note-relative per-voice state.
    pub normalized: Option<f64>,
    /// Physical-unit route depth.
    pub depth: f64,
    /// Physical contribution, or `None` when no singular Song-time value exists.
    pub contribution: Option<f64>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{dsl::parse_project, validate::validate_project};

    use super::*;

    #[test]
    fn packed_transition_flags_are_admitted_in_machine_words() {
        let word = std::mem::size_of::<usize>() as u64;
        assert_eq!(packed_bool_allocation_bytes(0), 0);
        assert_eq!(packed_bool_allocation_bytes(1), word);
        assert_eq!(packed_bool_allocation_bytes(usize::BITS as usize), word);
        assert_eq!(
            packed_bool_allocation_bytes(usize::BITS as usize + 1),
            word * 2
        );
    }

    const SOURCE: &str = r#"crump 1
project "Control" {
  id control
  seed 1
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 4
      parameter tone "Tone" ratio 0 1 0.5 0.5 automatable
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
    }
    pattern beat 1bar {
      hit 0beat kick 1
    }
    track drums "Drums" instrument drums level -12db pan 0 {
      mute false
      clip beat pattern beat start 0bar duration 1bar mode once
    }
    automation mute-drums target track/drums/mute {
      point unmuted 0bar false step
      point muted 1beat true step
    }
    automation ride target track/drums/level {
      point start 0bar -24db linear
      point lift 2beat -6db power(2)
      point hold 3beat -9db step
    }
    modulation z-wide lfo global bipolar sine free 1hz target track/drums/balance depth 0.5 {
      scale 0.5
      bias 0.25
      clamp -1 1
    }
    modulation a-narrow lfo global unipolar triangle sync 1beat target track/drums/balance depth 0.25 {
      add -0.5
      multiply 2
    }
  }
}
"#;

    fn project() -> Project {
        let document = parse_project(Path::new("project.crump"), SOURCE);
        assert!(!document.diagnostics.has_errors());
        let project = document.project.expect("parsed Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
        project
    }

    #[test]
    fn compiled_targets_match_the_authoritative_parameter_inventory() {
        let project = project();
        let inventory = project.parameter_specs();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        assert_eq!(program.target_count(), inventory.len());
        for (target, _) in inventory {
            assert!(
                program.index(&target).is_some(),
                "compiled controls omitted {target}"
            );
        }
    }

    #[test]
    fn interpolation_is_exact_at_boundaries_and_holds_last_value() {
        let project = project();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let target = ParameterTarget::Track {
            track: "drums".to_owned(),
            parameter: crate::model::TrackParameter::Level,
        };
        let index = program.index(&target).expect("Track level target");
        let mut runtime = program.runtime();
        runtime.evaluate_frame(0);
        assert_eq!(runtime.value(index), -24.0);
        runtime.evaluate_frame(24_000);
        assert_eq!(runtime.value(index), -15.0);
        runtime.evaluate_frame(48_000);
        assert_eq!(runtime.value(index), -6.0);
        runtime.evaluate_frame(60_000);
        assert_eq!(runtime.value(index), -6.75);
        runtime.evaluate_frame(72_000);
        assert_eq!(runtime.value(index), -9.0);
        runtime.evaluate_frame(200_000);
        assert_eq!(runtime.value(index), -9.0);
    }

    #[test]
    fn boolean_mute_automation_is_exact_and_step_only() {
        let project = project();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let target = ParameterTarget::Track {
            track: "drums".to_owned(),
            parameter: crate::model::TrackParameter::Mute,
        };
        let index = program.index(&target).expect("Track mute target");
        let mut runtime = program.runtime();
        runtime.evaluate_frame(0);
        assert_eq!(runtime.value(index), 0.0);
        runtime.evaluate_frame(23_999);
        assert_eq!(runtime.value(index), 0.0);
        runtime.evaluate_frame(24_000);
        assert_eq!(runtime.value(index), 1.0);

        let invalid = SOURCE.replacen(
            "point unmuted 0bar false step",
            "point unmuted 0bar false linear",
            1,
        );
        let parsed = parse_project(Path::new("project.crump"), &invalid);
        let diagnostics = validate_project(parsed.project.as_ref().expect("Project"));
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV132")
        );
    }

    #[test]
    fn automated_base_only_publication_does_not_smear_the_lane() {
        let current = project();
        let mut replacement = current.clone();
        replacement.song.tracks[0].channel.level_db = -3.0;
        let current = CompiledControlProgram::compile(&current).expect("compile current controls");
        let index = current
            .index(&ParameterTarget::Track {
                track: "drums".to_owned(),
                parameter: crate::model::TrackParameter::Level,
            })
            .expect("Track level control");
        let replacement =
            CompiledControlProgram::compile(&replacement).expect("compile replacement controls");
        let mut current = current.runtime();
        let mut replacement = replacement.runtime();
        assert!(!current.begin_publication_handoff(&mut replacement, 480));
        current.evaluate_frame(24_000);
        assert_eq!(current.value(index), -15.0);
    }

    #[test]
    fn snapshots_expose_effective_value_provenance() {
        let project = project();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let snapshot = program
            .snapshot_at(24_000)
            .into_iter()
            .find(|value| value.target.to_string() == "track/drums/level")
            .expect("level snapshot");
        assert_eq!(snapshot.base, -12.0);
        assert_eq!(snapshot.automation, Some(-15.0));
        assert_eq!(snapshot.effective, -15.0);
        assert!(!snapshot.clamped);
    }

    #[test]
    fn pattern_parameter_locks_override_automation_and_reset_at_clip_end() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "      parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "      parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
            )
            .replace(
                "      note 0beat C4 1beat 0.8",
                "      lock closed at 0beat parameter tone value 1000hz\n      lock open at 2beat parameter tone value 9000hz\n      note 0beat C4 1beat 0.8",
            )
            .replace(
                "    track lead \"Lead\" instrument lead level -9db pan -0.1 {",
                "    automation lead-tone target instrument/lead/tone {\n      point start 0bar 5000hz step\n    }\n    track lead \"Lead\" instrument lead level -9db pan -0.1 {",
            );
        let document = parse_project(Path::new("project.crump"), &source);
        let parsed = document.project.expect("Project with Parameter locks");
        let diagnostics = validate_project(&parsed);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
        let program = CompiledControlProgram::compile(&parsed).expect("control program");
        let target = ParameterTarget::Instrument {
            instrument: "lead".to_owned(),
            parameter: "tone".to_owned(),
        };
        let value_at = |frame| {
            program
                .snapshot_at(frame)
                .into_iter()
                .find(|snapshot| snapshot.target == target)
                .expect("tone snapshot")
        };

        let closed = value_at(0);
        assert_eq!(closed.automation, Some(5_000.0));
        assert_eq!(closed.parameter_lock, Some(1_000.0));
        assert_eq!(closed.effective, 1_000.0);
        let open = value_at(48_000);
        assert_eq!(open.parameter_lock, Some(9_000.0));
        assert_eq!(open.effective, 9_000.0);
        let reset = value_at(96_000);
        assert_eq!(reset.parameter_lock, None);
        assert_eq!(reset.effective, 5_000.0);
    }

    #[test]
    fn routes_use_normalized_arithmetic_and_stable_id_order() {
        let project = project();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let snapshot = program
            .snapshot_at(12_000)
            .into_iter()
            .find(|value| value.target.to_string() == "track/drums/balance")
            .expect("balance snapshot");
        assert_eq!(
            snapshot
                .modulation_routes
                .iter()
                .map(|route| route.route.as_str())
                .collect::<Vec<_>>(),
            vec!["a-narrow", "z-wide"]
        );
        assert!((snapshot.modulation - 0.625).abs() < 1.0e-12);
        assert!((snapshot.effective - 0.625).abs() < 1.0e-12);
    }
}
