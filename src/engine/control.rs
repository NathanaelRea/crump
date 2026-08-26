//! Callback-safe effective Parameter evaluation shared by playback and rendering.

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::model::{
    AutomationInterpolation, ControlPolarity, GraphDomain, Instrument, InstrumentModulationRate,
    InstrumentModulationScope, InstrumentModulationSourceKind, LfoDirection, LfoMode, LfoRetrigger,
    LfoWaveform, ModulationCurve, ModulationOperator, ModulationRate, ModulationScope,
    ParameterTarget, Project, Unit,
};

use super::{
    EngineError,
    schedule::{compile_parameter_lock_lanes, compile_track_parameter_lock_lanes},
};

/// Dense callback-time index resolved from a stable authored target during compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ControlIndex(pub(crate) usize);

/// Immutable compact control program compiled away from the audio callback.
#[derive(Debug, Clone)]
pub struct CompiledControlProgram {
    targets: Arc<Vec<CompiledTarget>>,
    indexes: Arc<BTreeMap<ParameterTarget, ControlIndex>>,
    instrument_routes: Arc<Vec<CompiledInstrumentRoute>>,
    instrument_local_routes: Arc<Vec<CompiledInstrumentRoute>>,
    track_parameter_locks: Arc<BTreeMap<String, Arc<Vec<CompiledIndexedParameterLockLane>>>>,
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
        let track_parameter_lock_lanes = compile_track_parameter_lock_lanes(project)?;
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
        let mut instruments = project.song.instruments.iter().collect::<Vec<_>>();
        instruments.sort_by(|left, right| left.id.cmp(&right.id));
        let instrument_routes: Vec<CompiledInstrumentRoute> = instruments
            .iter()
            .map(|instrument| {
                compile_instrument_routes(
                    instrument,
                    project,
                    &indexes,
                    &[InstrumentModulationScope::Global],
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let instrument_local_routes: Vec<CompiledInstrumentRoute> = instruments
            .iter()
            .map(|instrument| {
                compile_instrument_routes(
                    instrument,
                    project,
                    &indexes,
                    &[
                        InstrumentModulationScope::Instrument,
                        InstrumentModulationScope::PerVoice,
                    ],
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let locked_targets = track_parameter_lock_lanes
            .keys()
            .filter_map(|(_, target)| indexes.get(target).copied())
            .collect::<std::collections::BTreeSet<_>>();
        if instrument_routes.iter().any(|route| {
            std::iter::once(&route.source)
                .chain(route.auxiliary.as_ref())
                .any(|source| {
                    matches!(
                        source.kind,
                        CompiledInstrumentSourceKind::Parameter { parameter }
                            if locked_targets.contains(&parameter)
                    )
                })
        }) {
            return Err(EngineError::Compile(
                "a Song-global Parameter modulation source cannot read a Track-owned Parameter lock"
                    .to_owned(),
            ));
        }
        let mut compiled_track_locks =
            BTreeMap::<String, Vec<CompiledIndexedParameterLockLane>>::new();
        for ((track, target), points) in track_parameter_lock_lanes {
            let index = indexes.get(&target).copied().ok_or_else(|| {
                EngineError::Compile(format!(
                    "Parameter lock targets missing Parameter `{target}`"
                ))
            })?;
            let unit = targets[index.0].unit;
            let points = points
                .into_iter()
                .map(|point| {
                    let value = point
                        .value
                        .as_ref()
                        .map(|value| {
                            unit.parse_value(value).map_err(|error| {
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
                .collect::<Result<Vec<_>, EngineError>>()?;
            compiled_track_locks
                .entry(track)
                .or_default()
                .push(CompiledIndexedParameterLockLane {
                    target: index,
                    lane: CompiledParameterLockLane { points },
                });
        }
        let track_parameter_locks = compiled_track_locks
            .into_iter()
            .map(|(track, mut lanes)| {
                lanes.sort_by_key(|lane| lane.target);
                (track, Arc::new(lanes))
            })
            .collect();
        Ok(Self {
            targets: Arc::new(targets),
            indexes: Arc::new(indexes),
            instrument_routes: Arc::new(instrument_routes),
            instrument_local_routes: Arc::new(instrument_local_routes),
            track_parameter_locks: Arc::new(track_parameter_locks),
        })
    }

    /// Resolves one stable authored target away from the callback.
    #[must_use]
    pub(crate) fn index(&self, target: &ParameterTarget) -> Option<ControlIndex> {
        self.indexes.get(target).copied()
    }

    pub(crate) fn graph_routes(
        &self,
        instrument: &Instrument,
        project: &Project,
        domain: GraphDomain,
    ) -> Result<Vec<CompiledPerVoiceRoute>, EngineError> {
        let scopes: &[InstrumentModulationScope] = match domain {
            GraphDomain::Voice => &[
                InstrumentModulationScope::Global,
                InstrumentModulationScope::Instrument,
                InstrumentModulationScope::PerVoice,
            ],
            GraphDomain::Shared => &[
                InstrumentModulationScope::Global,
                InstrumentModulationScope::Instrument,
            ],
        };
        let mut routes = compile_instrument_routes(instrument, project, &self.indexes, scopes)?
            .into_iter()
            .map(CompiledPerVoiceRoute::Instrument)
            .collect::<Vec<_>>();
        for (target_index, target) in self.targets.iter().enumerate() {
            let has_song_tier = target.routes.iter().any(|route| {
                !matches!(&route.scope, ModulationScope::PerVoice(owner) if owner == &instrument.id)
            }) || project
                .song
                .envelope_followers
                .iter()
                .any(|route| route.target == target.target);
            if has_song_tier
                && matches!(
                    &target.target,
                    ParameterTarget::Instrument { instrument: owner, .. } if owner == &instrument.id
                )
            {
                routes.push(CompiledPerVoiceRoute::SongTier {
                    target: ControlIndex(target_index),
                    minimum: target.minimum,
                    maximum: target.maximum,
                });
            }
        }
        if domain == GraphDomain::Voice {
            let mut song_routes = Vec::new();
            for (target_index, target) in self.targets.iter().enumerate() {
                for route in &target.routes {
                    if matches!(&route.scope, ModulationScope::PerVoice(owner) if owner == &instrument.id)
                    {
                        song_routes.push(CompiledPerVoiceRoute::Song {
                            target: ControlIndex(target_index),
                            minimum: target.minimum,
                            maximum: target.maximum,
                            route: route.clone(),
                        });
                    }
                }
            }
            song_routes.sort_by(|left, right| left.id().cmp(right.id()));
            routes.extend(song_routes);
        }
        Ok(routes)
    }

    pub(crate) fn target_count(&self) -> usize {
        self.targets.len()
    }

    pub(crate) fn track_parameter_lock_runtime(
        &self,
        track_id: &str,
    ) -> Option<TrackParameterLockRuntime> {
        self.track_parameter_locks
            .get(track_id)
            .map(|lanes| TrackParameterLockRuntime {
                lanes: lanes.clone(),
                effective: vec![0.0; self.targets.len()],
                pre_instrument_unconstrained: vec![0.0; self.targets.len()],
                transition_origin: vec![0.0; self.targets.len()],
                transition_pre_instrument_origin: vec![0.0; self.targets.len()],
                transition_changed: vec![false; self.targets.len()],
                transition_lanes: None,
                transition_active: false,
                transition_position: 0,
                transition_frames: 0,
            })
    }

    pub(crate) fn track_parameter_lock_runtime_state_bytes(&self) -> u64 {
        let arrays = (self.targets.len() as u64)
            .saturating_mul(4)
            .saturating_mul(std::mem::size_of::<f64>() as u64)
            .saturating_add(packed_bool_allocation_bytes(self.targets.len()));
        (self.track_parameter_locks.len() as u64).saturating_mul(arrays)
    }

    pub(crate) fn global_instrument_route_index(
        &self,
        route: &CompiledInstrumentRoute,
    ) -> Option<usize> {
        (route.source.scope == InstrumentModulationScope::Global).then(|| {
            self.instrument_routes
                .iter()
                .position(|candidate| candidate == route)
                .expect("graph global route must share the compiled Control program")
        })
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
        let instrument_routes = (std::mem::size_of::<Vec<CompiledInstrumentRoute>>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(
                (self.instrument_routes.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<CompiledInstrumentRoute>() as u64),
            )
            .saturating_add(
                self.instrument_routes
                    .iter()
                    .map(CompiledInstrumentRoute::heap_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        let instrument_local_routes = (std::mem::size_of::<Vec<CompiledInstrumentRoute>>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(
                (self.instrument_local_routes.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<CompiledInstrumentRoute>() as u64),
            )
            .saturating_add(
                self.instrument_local_routes
                    .iter()
                    .map(CompiledInstrumentRoute::heap_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        let track_parameter_locks = (std::mem::size_of::<
            BTreeMap<String, Arc<Vec<CompiledIndexedParameterLockLane>>>,
        >() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(
                (self.track_parameter_locks.len() as u64).saturating_mul(
                    (std::mem::size_of::<(String, Arc<Vec<CompiledIndexedParameterLockLane>>)>()
                        as u64)
                        .saturating_add(word.saturating_mul(4)),
                ),
            )
            .saturating_add(self.track_parameter_locks.iter().fold(
                0_u64,
                |bytes, (track, lanes)| {
                    lanes.iter().fold(
                        bytes
                            .saturating_add(track.capacity() as u64)
                            .saturating_add(
                                std::mem::size_of::<Vec<CompiledIndexedParameterLockLane>>() as u64,
                            )
                            .saturating_add(word.saturating_mul(2))
                            .saturating_add((lanes.capacity() as u64).saturating_mul(
                                std::mem::size_of::<CompiledIndexedParameterLockLane>() as u64,
                            )),
                        |bytes, lane| {
                            bytes.saturating_add(
                                (lane.lane.points.capacity() as u64).saturating_mul(
                                    std::mem::size_of::<CompiledParameterLockPoint>() as u64,
                                ),
                            )
                        },
                    )
                },
            ));
        targets
            .saturating_add(indexes)
            .saturating_add(instrument_routes)
            .saturating_add(instrument_local_routes)
            .saturating_add(track_parameter_locks)
    }

    /// Mutable vector backing allocated for one callback-time evaluator.
    pub(crate) fn runtime_state_bytes(&self) -> u64 {
        let targets = self.targets.len() as u64;
        targets
            .saturating_mul(7)
            .saturating_mul(std::mem::size_of::<f64>() as u64)
            .saturating_add(targets.saturating_mul(std::mem::size_of::<RuntimeProvenance>() as u64))
            .saturating_add(
                (self.instrument_routes.len() as u64)
                    .saturating_mul(std::mem::size_of::<RuntimeInstrumentRouteValue>() as u64),
            )
            .saturating_add(packed_bool_allocation_bytes(self.targets.len()))
    }

    /// Deterministic scalar work added by Instrument/global sources and routes each frame.
    pub(crate) fn instrument_work_units_per_frame(&self) -> u64 {
        self.instrument_routes.iter().fold(0_u64, |work, route| {
            work.saturating_add(2)
                .saturating_add(route.operators.len() as u64)
                .saturating_add(u64::from(route.auxiliary.is_some()))
        })
    }

    pub(crate) fn initial_value(&self, index: ControlIndex) -> Option<f64> {
        self.targets
            .get(index.0)
            .map(|target| target.base.clamp(target.minimum, target.maximum))
    }

    /// Creates preallocated mutable effective-value state.
    #[must_use]
    pub(crate) fn runtime(&self) -> ControlRuntime {
        self.clone().into_runtime()
    }

    fn into_runtime(self) -> ControlRuntime {
        let target_count = self.targets.len();
        let route_count = self.instrument_routes.len();
        let effective = self
            .targets
            .iter()
            .map(|target| target.base.clamp(target.minimum, target.maximum))
            .collect::<Vec<_>>();
        ControlRuntime {
            program: self,
            transition_origin: effective.clone(),
            pre_instrument_unconstrained: effective.clone(),
            instrument_unconstrained: effective.clone(),
            song_modulation: vec![0.0; target_count],
            unconstrained: effective.clone(),
            effective,
            modulation: vec![0.0; target_count],
            provenance: vec![RuntimeProvenance::default(); target_count],
            instrument_route_values: vec![RuntimeInstrumentRouteValue::default(); route_count],
            transition_changed: vec![false; target_count],
            transition_position: 0,
            transition_frames: 0,
            apply_parameter_locks: false,
        }
    }

    /// Consumes this immutable program into a reusable off-callback inspector.
    #[must_use]
    pub fn into_inspector(self) -> ControlInspector {
        let mut instrument_targets = BTreeMap::<String, Vec<InspectorInstrumentTarget>>::new();
        for (target_index, target) in self.targets.iter().enumerate() {
            let ParameterTarget::Instrument { instrument, .. } = &target.target else {
                continue;
            };
            let index = ControlIndex(target_index);
            instrument_targets
                .entry(instrument.clone())
                .or_default()
                .push(InspectorInstrumentTarget {
                    target_index,
                    routes: {
                        let mut routes = self
                            .instrument_routes
                            .iter()
                            .enumerate()
                            .filter_map(|(route_index, route)| {
                                (route.target == index)
                                    .then_some(InspectorInstrumentRoute::Global(route_index))
                            })
                            .chain(self.instrument_local_routes.iter().enumerate().filter_map(
                                |(route_index, route)| {
                                    (route.target == index)
                                        .then_some(InspectorInstrumentRoute::Local(route_index))
                                },
                            ))
                            .collect::<Vec<_>>();
                        routes.sort_by(|left, right| {
                            let left = left.route(&self);
                            let right = right.route(&self);
                            left.order
                                .cmp(&right.order)
                                .then_with(|| left.id.cmp(&right.id))
                        });
                        routes
                    },
                });
        }
        let mut runtime = self.into_runtime();
        runtime.apply_parameter_locks = true;
        ControlInspector {
            runtime,
            instrument_targets,
        }
    }

    /// Evaluates inspectable provenance at one absolute sample frame off the callback.
    #[must_use]
    pub fn snapshot_at(&self, frame: u64) -> Vec<ControlValueSnapshot> {
        let mut runtime = self.runtime();
        runtime.apply_parameter_locks = true;
        runtime.evaluate_frame(frame);
        self.targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let provenance = runtime.provenance[index];
                let mut instrument_modulation_routes = self
                    .instrument_routes
                    .iter()
                    .enumerate()
                    .filter(|(_, route)| route.target == ControlIndex(index))
                    .map(|(route_index, route)| {
                        route.snapshot(Some(runtime.instrument_route_values[route_index]))
                    })
                    .chain(
                        self.instrument_local_routes
                            .iter()
                            .filter(|route| route.target == ControlIndex(index))
                            .map(|route| route.snapshot(None)),
                    )
                    .collect::<Vec<_>>();
                instrument_modulation_routes.sort_by(|left, right| {
                    left.order
                        .cmp(&right.order)
                        .then_with(|| left.route.cmp(&right.route))
                });
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
                    instrument_modulation_routes,
                    unconstrained: provenance.unconstrained,
                    effective: runtime.effective[index],
                    clamped: provenance.clamped,
                    held_non_finite: provenance.held_non_finite,
                }
            })
            .collect()
    }
}

/// Reusable off-callback evaluator for targeted control inspection.
#[derive(Debug, Clone)]
pub struct ControlInspector {
    runtime: ControlRuntime,
    instrument_targets: BTreeMap<String, Vec<InspectorInstrumentTarget>>,
}

#[derive(Debug, Clone)]
struct InspectorInstrumentTarget {
    target_index: usize,
    routes: Vec<InspectorInstrumentRoute>,
}

#[derive(Debug, Clone, Copy)]
enum InspectorInstrumentRoute {
    Global(usize),
    Local(usize),
}

impl InspectorInstrumentRoute {
    fn route<'a>(&self, program: &'a CompiledControlProgram) -> &'a CompiledInstrumentRoute {
        match *self {
            Self::Global(index) => &program.instrument_routes[index],
            Self::Local(index) => &program.instrument_local_routes[index],
        }
    }
}

impl ControlInspector {
    /// Evaluates one exact target without materializing every Project target.
    pub fn snapshot_target_at(
        &mut self,
        frame: u64,
        target: &ParameterTarget,
    ) -> Option<ControlValueSnapshot> {
        self.evaluate(frame);
        let index = self.runtime.program.indexes.get(target)?.0;
        Some(self.snapshot_index(frame, index))
    }

    /// Evaluates only Instrument-owned routes for the requested Instrument.
    pub fn snapshot_instrument_routes_at(
        &mut self,
        frame: u64,
        instrument: &str,
    ) -> Vec<(Unit, InstrumentModulationValueSnapshot)> {
        self.evaluate(frame);
        let program = &self.runtime.program;
        let Some(targets) = self.instrument_targets.get(instrument) else {
            return Vec::new();
        };
        let route_count = targets.iter().map(|target| target.routes.len()).sum();
        let mut snapshots = Vec::with_capacity(route_count);
        for target_routes in targets {
            let target = &program.targets[target_routes.target_index];
            snapshots.extend(target_routes.routes.iter().map(|route_index| {
                let route = route_index.route(program);
                let value = match *route_index {
                    InspectorInstrumentRoute::Global(index) => {
                        Some(self.runtime.instrument_route_values[index])
                    }
                    InspectorInstrumentRoute::Local(_) => None,
                };
                (target.unit, route.snapshot(value))
            }));
        }
        snapshots
    }

    fn evaluate(&mut self, frame: u64) {
        self.runtime
            .effective
            .copy_from_slice(&self.runtime.transition_origin);
        self.runtime.evaluate_frame(frame);
    }

    fn snapshot_index(&self, frame: u64, index: usize) -> ControlValueSnapshot {
        let program = &self.runtime.program;
        let target = &program.targets[index];
        let provenance = self.runtime.provenance[index];
        let mut instrument_modulation_routes = program
            .instrument_routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.target == ControlIndex(index))
            .map(|(route_index, route)| {
                route.snapshot(Some(self.runtime.instrument_route_values[route_index]))
            })
            .chain(
                program
                    .instrument_local_routes
                    .iter()
                    .filter(|route| route.target == ControlIndex(index))
                    .map(|route| route.snapshot(None)),
            )
            .collect::<Vec<_>>();
        instrument_modulation_routes.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.route.cmp(&right.route))
        });
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
            instrument_modulation_routes,
            unconstrained: provenance.unconstrained,
            effective: self.runtime.effective[index],
            clamped: provenance.clamped,
            held_non_finite: provenance.held_non_finite,
        }
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
pub(crate) struct CompiledRoute {
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
        let position = frame as f64 * self.phase_increment;
        let phase = position.fract();
        let cycle = position.floor().max(0.0) as u64;
        let bipolar = super::lfo::bipolar_sample(
            self.waveform,
            phase,
            cycle,
            stable_modulation_identity(&self.id),
        );
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
pub(crate) enum CompiledInstrumentSourceKind {
    GraphPort {
        node: String,
        port: String,
    },
    Lfo {
        waveform: LfoWaveform,
        phase_increment: f64,
        phase: f64,
        mode: LfoMode,
        direction: LfoDirection,
    },
    NoteVelocity,
    NoteKey {
        reference: u8,
        range_semitones: f64,
    },
    NoteGate,
    NoteAge {
        duration_frames: f64,
    },
    RandomHold {
        period_frames: u64,
        seed: u64,
    },
    Parameter {
        parameter: ControlIndex,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompiledInstrumentSource {
    id: String,
    scope: InstrumentModulationScope,
    polarity: ControlPolarity,
    kind: CompiledInstrumentSourceKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompiledInstrumentRoute {
    id: String,
    order: i32,
    target: ControlIndex,
    minimum: f64,
    maximum: f64,
    source: CompiledInstrumentSource,
    auxiliary: Option<CompiledInstrumentSource>,
    depth: f64,
    polarity: ControlPolarity,
    curve: ModulationCurve,
    bypass: bool,
    operators: Vec<ModulationOperator>,
}

impl CompiledInstrumentRoute {
    fn heap_bytes(&self) -> u64 {
        (self.id.capacity() + self.source.heap_bytes()) as u64
            + self
                .auxiliary
                .as_ref()
                .map_or(0, |source| source.heap_bytes() as u64)
            + self.operators.capacity() as u64 * std::mem::size_of::<ModulationOperator>() as u64
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) const fn target(&self) -> ControlIndex {
        self.target
    }

    pub(crate) const fn source(&self) -> &CompiledInstrumentSource {
        &self.source
    }

    pub(crate) fn auxiliary(&self) -> Option<&CompiledInstrumentSource> {
        self.auxiliary.as_ref()
    }

    pub(crate) fn topology_compatible(&self, replacement: &Self) -> bool {
        self.id == replacement.id
            && self.order == replacement.order
            && self.target == replacement.target
            && self.minimum.to_bits() == replacement.minimum.to_bits()
            && self.maximum.to_bits() == replacement.maximum.to_bits()
            && self.source == replacement.source
            && self.auxiliary == replacement.auxiliary
            && self.polarity == replacement.polarity
            && self.curve == replacement.curve
            && self.operators == replacement.operators
    }

    pub(crate) fn scalar_changed(&self, replacement: &Self) -> bool {
        self.depth.to_bits() != replacement.depth.to_bits() || self.bypass != replacement.bypass
    }

    pub(crate) fn parameter_dependencies(&self) -> impl Iterator<Item = ControlIndex> + '_ {
        std::iter::once(&self.source)
            .chain(self.auxiliary.as_ref())
            .filter_map(|source| match source.kind {
                CompiledInstrumentSourceKind::Parameter { parameter } => Some(parameter),
                _ => None,
            })
    }

    pub(crate) fn contribution(&self, primary: f64, auxiliary: Option<f64>) -> f64 {
        if self.bypass {
            return 0.0;
        }
        let mut value = remap_polarity(primary, self.source.polarity, self.polarity);
        value = match self.curve {
            ModulationCurve::Linear => value,
            ModulationCurve::Power(exponent) => match self.polarity {
                ControlPolarity::Unipolar => value.clamp(0.0, 1.0).powf(exponent),
                ControlPolarity::Bipolar => value.signum() * value.abs().min(1.0).powf(exponent),
            },
            ModulationCurve::Smoothstep => match self.polarity {
                ControlPolarity::Unipolar => {
                    let value = value.clamp(0.0, 1.0);
                    value * value * (3.0 - 2.0 * value)
                }
                ControlPolarity::Bipolar => {
                    let magnitude = value.abs().min(1.0);
                    value.signum() * magnitude * magnitude * (3.0 - 2.0 * magnitude)
                }
            },
        };
        if let Some(auxiliary) = auxiliary {
            value *= auxiliary;
        }
        for operator in &self.operators {
            value = apply_operator(value, *operator);
        }
        value * self.depth
    }

    fn snapshot(
        &self,
        value: Option<RuntimeInstrumentRouteValue>,
    ) -> InstrumentModulationValueSnapshot {
        InstrumentModulationValueSnapshot {
            route: self.id.clone(),
            source: self.source.id.clone(),
            scope: self.source.scope,
            order: self.order,
            bypass: self.bypass,
            normalized: value.map(|value| value.normalized),
            auxiliary: value.and_then(|value| value.auxiliary),
            depth: self.depth,
            contribution: value.map(|value| value.contribution),
        }
    }
}

pub(crate) struct InstrumentSourceContext<'a> {
    pub(crate) frame: u64,
    pub(crate) pitch: u8,
    pub(crate) velocity: f64,
    pub(crate) gate: bool,
    pub(crate) controls: &'a [f64],
    pub(crate) graph_control: Option<f64>,
    pub(crate) random_identity: u64,
}

impl CompiledInstrumentSource {
    fn heap_bytes(&self) -> usize {
        self.id.capacity()
            + match &self.kind {
                CompiledInstrumentSourceKind::GraphPort { node, port } => {
                    node.capacity() + port.capacity()
                }
                _ => 0,
            }
    }

    pub(crate) const fn kind(&self) -> &CompiledInstrumentSourceKind {
        &self.kind
    }

    pub(crate) const fn scope(&self) -> InstrumentModulationScope {
        self.scope
    }

    pub(crate) fn value(&self, context: InstrumentSourceContext<'_>) -> Option<f64> {
        let native = match self.kind {
            CompiledInstrumentSourceKind::GraphPort { .. } => context.graph_control?,
            CompiledInstrumentSourceKind::Lfo {
                waveform,
                phase_increment,
                phase,
                mode,
                direction,
            } => {
                let (phase, cycle) = super::lfo::position(
                    phase,
                    context.frame as f64 * phase_increment,
                    mode,
                    direction,
                );
                super::lfo::bipolar_sample(
                    waveform,
                    phase,
                    cycle,
                    mix_modulation_identity(
                        stable_modulation_identity(&self.id),
                        context.random_identity,
                    ),
                )
            }
            CompiledInstrumentSourceKind::NoteVelocity => context.velocity.clamp(0.0, 1.0),
            CompiledInstrumentSourceKind::NoteKey {
                reference,
                range_semitones,
            } => (f64::from(context.pitch) - f64::from(reference)) / range_semitones,
            CompiledInstrumentSourceKind::NoteGate => f64::from(context.gate),
            CompiledInstrumentSourceKind::NoteAge { duration_frames } => {
                (context.frame as f64 / duration_frames).clamp(0.0, 1.0)
            }
            CompiledInstrumentSourceKind::RandomHold {
                period_frames,
                seed,
            } => {
                deterministic_random(
                    seed ^ context.random_identity,
                    context.frame / period_frames,
                ) * 2.0
                    - 1.0
            }
            CompiledInstrumentSourceKind::Parameter { parameter } => context.controls[parameter.0],
        };
        Some(match self.polarity {
            ControlPolarity::Unipolar => match self.kind {
                CompiledInstrumentSourceKind::Lfo { .. }
                | CompiledInstrumentSourceKind::RandomHold { .. } => (native + 1.0) * 0.5,
                _ => native.clamp(0.0, 1.0),
            },
            ControlPolarity::Bipolar => match self.kind {
                CompiledInstrumentSourceKind::NoteVelocity
                | CompiledInstrumentSourceKind::NoteGate
                | CompiledInstrumentSourceKind::NoteAge { .. }
                | CompiledInstrumentSourceKind::Parameter { .. } => native * 2.0 - 1.0,
                _ => native.clamp(-1.0, 1.0),
            },
        })
    }
}

fn remap_polarity(value: f64, from: ControlPolarity, to: ControlPolarity) -> f64 {
    match (from, to) {
        (ControlPolarity::Unipolar, ControlPolarity::Bipolar) => value * 2.0 - 1.0,
        (ControlPolarity::Bipolar, ControlPolarity::Unipolar) => (value + 1.0) * 0.5,
        _ => value,
    }
}

fn apply_operator(value: f64, operator: ModulationOperator) -> f64 {
    match operator {
        ModulationOperator::Add { value: operand }
        | ModulationOperator::Bias { value: operand } => value + operand,
        ModulationOperator::Multiply { value: operand }
        | ModulationOperator::Scale { value: operand } => value * operand,
        ModulationOperator::Clamp { minimum, maximum } => value.clamp(minimum, maximum),
    }
}

fn deterministic_random(seed: u64, index: u64) -> f64 {
    let mut value = seed ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let bits = value ^ (value >> 31);
    (bits >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
}

fn compile_instrument_routes(
    instrument: &Instrument,
    project: &Project,
    indexes: &BTreeMap<ParameterTarget, ControlIndex>,
    scopes: &[InstrumentModulationScope],
) -> Result<Vec<CompiledInstrumentRoute>, EngineError> {
    let authored_sources = instrument
        .modulation_sources
        .iter()
        .map(|source| (source.id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    let mut routes = Vec::new();
    for route in &instrument.modulation_routes {
        let source = authored_sources.get(route.source.as_str()).ok_or_else(|| {
            EngineError::Compile(format!("Instrument route `{}` has no source", route.id))
        })?;
        if !scopes.contains(&source.scope) {
            continue;
        }
        let parameter = instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == route.target)
            .ok_or_else(|| {
                EngineError::Compile(format!("Instrument route `{}` has no target", route.id))
            })?;
        let target = indexes
            .get(&ParameterTarget::Instrument {
                instrument: instrument.id.clone(),
                parameter: route.target.clone(),
            })
            .copied()
            .ok_or_else(|| {
                EngineError::Compile(format!(
                    "Instrument route `{}` target is not compiled",
                    route.id
                ))
            })?;
        let source = compile_instrument_source(source, instrument, project, indexes)?;
        let auxiliary = route
            .auxiliary
            .as_ref()
            .map(|id| {
                authored_sources
                    .get(id.as_str())
                    .ok_or_else(|| {
                        EngineError::Compile(format!(
                            "Instrument route `{}` has no auxiliary source",
                            route.id
                        ))
                    })
                    .and_then(|source| {
                        compile_instrument_source(source, instrument, project, indexes)
                    })
            })
            .transpose()?;
        routes.push(CompiledInstrumentRoute {
            id: route.id.clone(),
            order: route.order,
            target,
            minimum: parameter.minimum,
            maximum: parameter.maximum,
            source,
            auxiliary,
            depth: parameter.unit.parse_value(&route.depth).map_err(|error| {
                EngineError::Compile(format!("Instrument route `{}` depth: {error}", route.id))
            })?,
            polarity: route.polarity,
            curve: route.curve,
            bypass: route.bypass,
            operators: route.operators.clone(),
        });
    }
    routes.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.id.cmp(&right.id))
    });
    stable_route_dag_order(routes)
}

fn stable_route_dag_order(
    routes: Vec<CompiledInstrumentRoute>,
) -> Result<Vec<CompiledInstrumentRoute>, EngineError> {
    let mut outgoing = vec![Vec::<usize>::new(); routes.len()];
    let mut incoming = vec![0_usize; routes.len()];
    let mut add_edge = |from: usize, to: usize| {
        if from != to && !outgoing[from].contains(&to) {
            outgoing[from].push(to);
            incoming[to] += 1;
        }
    };
    for (producer_index, producer) in routes.iter().enumerate() {
        for (consumer_index, consumer) in routes.iter().enumerate() {
            let consumes_target = matches!(consumer.source.kind, CompiledInstrumentSourceKind::Parameter { parameter } if parameter == producer.target)
                || consumer.auxiliary.as_ref().is_some_and(|source| {
                    matches!(source.kind, CompiledInstrumentSourceKind::Parameter { parameter } if parameter == producer.target)
                });
            if consumes_target {
                add_edge(producer_index, consumer_index);
            }
        }
    }
    for left in 0..routes.len() {
        for right in left + 1..routes.len() {
            if routes[left].target == routes[right].target {
                add_edge(left, right);
            }
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<std::collections::BTreeSet<_>>();
    let mut order = Vec::with_capacity(routes.len());
    while let Some(index) = ready.pop_first() {
        order.push(index);
        for destination in &outgoing[index] {
            incoming[*destination] -= 1;
            if incoming[*destination] == 0 {
                ready.insert(*destination);
            }
        }
    }
    if order.len() != routes.len() {
        return Err(EngineError::Compile(
            "Instrument macro/route dependency graph contains a cycle".to_owned(),
        ));
    }
    let mut routes = routes.into_iter().map(Some).collect::<Vec<_>>();
    Ok(order
        .into_iter()
        .map(|index| routes[index].take().expect("route index is unique"))
        .collect())
}

fn compile_instrument_source(
    source: &crate::model::InstrumentModulationSource,
    instrument: &Instrument,
    project: &Project,
    indexes: &BTreeMap<ParameterTarget, ControlIndex>,
) -> Result<CompiledInstrumentSource, EngineError> {
    let kind = match &source.kind {
        InstrumentModulationSourceKind::GraphPort { node, port } => {
            CompiledInstrumentSourceKind::GraphPort {
                node: node.clone(),
                port: port.clone(),
            }
        }
        InstrumentModulationSourceKind::Lfo {
            waveform,
            rate,
            phase,
            mode,
            direction,
            retrigger,
        } => {
            let expected = if source.scope == InstrumentModulationScope::PerVoice {
                LfoRetrigger::Note
            } else {
                LfoRetrigger::Transport
            };
            if *retrigger != expected {
                return Err(EngineError::Compile(format!(
                    "Instrument LFO `{}` retrigger is incompatible with its scope",
                    source.id
                )));
            }
            CompiledInstrumentSourceKind::Lfo {
                waveform: *waveform,
                phase_increment: modulation_phase_increment(rate, project)?,
                phase: *phase,
                mode: *mode,
                direction: *direction,
            }
        }
        InstrumentModulationSourceKind::NoteVelocity => CompiledInstrumentSourceKind::NoteVelocity,
        InstrumentModulationSourceKind::NoteKey {
            reference,
            range_semitones,
        } => CompiledInstrumentSourceKind::NoteKey {
            reference: *reference,
            range_semitones: *range_semitones,
        },
        InstrumentModulationSourceKind::NoteGate => CompiledInstrumentSourceKind::NoteGate,
        InstrumentModulationSourceKind::NoteAge { duration_seconds } => {
            CompiledInstrumentSourceKind::NoteAge {
                duration_frames: duration_seconds * f64::from(project.sample_rate),
            }
        }
        InstrumentModulationSourceKind::RandomHold { rate, seed } => {
            let increment = modulation_phase_increment(rate, project)?;
            CompiledInstrumentSourceKind::RandomHold {
                period_frames: (1.0 / increment).round().max(1.0) as u64,
                seed: *seed
                    ^ project.seed
                    ^ stable_modulation_identity(&instrument.id)
                    ^ stable_modulation_identity(&source.id),
            }
        }
        InstrumentModulationSourceKind::Parameter { parameter } => {
            CompiledInstrumentSourceKind::Parameter {
                parameter: indexes
                    .get(&ParameterTarget::Instrument {
                        instrument: instrument.id.clone(),
                        parameter: parameter.clone(),
                    })
                    .copied()
                    .ok_or_else(|| {
                        EngineError::Compile(format!("missing macro Parameter `{parameter}`"))
                    })?,
            }
        }
    };
    Ok(CompiledInstrumentSource {
        id: source.id.clone(),
        scope: source.scope,
        polarity: source.polarity,
        kind,
    })
}

fn modulation_phase_increment(
    rate: &InstrumentModulationRate,
    project: &Project,
) -> Result<f64, EngineError> {
    match rate {
        InstrumentModulationRate::FreeHertz(rate) => Ok(*rate / f64::from(project.sample_rate)),
        InstrumentModulationRate::Synchronized(literal) => {
            let period =
                crate::time::parse_musical_time(literal, project.song.meter).map_err(|error| {
                    EngineError::Compile(format!("invalid synchronized Modulation rate: {error}"))
                })?;
            let frames = period.to_frames(project.song.tempo_bpm, project.sample_rate)?;
            if frames == 0 {
                Err(EngineError::Compile(
                    "synchronized Modulation period resolves to zero frames".to_owned(),
                ))
            } else {
                Ok(1.0 / frames as f64)
            }
        }
    }
}

pub(crate) fn stable_modulation_identity(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

pub(crate) fn mix_modulation_identity(seed: u64, identity: u64) -> u64 {
    let mut value = seed ^ identity.rotate_left(29) ^ 0x9e37_79b9_7f4a_7c15;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
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
pub(crate) struct CompiledIndexedParameterLockLane {
    target: ControlIndex,
    lane: CompiledParameterLockLane,
}

/// Preallocated per-Track projection for authored Instrument Parameter locks.
#[derive(Debug, Clone)]
pub(crate) struct TrackParameterLockRuntime {
    lanes: Arc<Vec<CompiledIndexedParameterLockLane>>,
    effective: Vec<f64>,
    pre_instrument_unconstrained: Vec<f64>,
    transition_origin: Vec<f64>,
    transition_pre_instrument_origin: Vec<f64>,
    transition_changed: Vec<bool>,
    transition_lanes: Option<Arc<Vec<CompiledIndexedParameterLockLane>>>,
    transition_active: bool,
    transition_position: u64,
    transition_frames: u64,
}

impl TrackParameterLockRuntime {
    /// Overlays only this Track's active locks onto the Song-wide unlocked controls.
    pub(crate) fn project<'a>(
        &'a mut self,
        frame: u64,
        effective: &'a [f64],
        pre_instrument_unconstrained: &'a [f64],
    ) -> (&'a [f64], &'a [f64]) {
        self.effective.copy_from_slice(effective);
        self.pre_instrument_unconstrained
            .copy_from_slice(pre_instrument_unconstrained);
        let lanes = if self.transition_active {
            self.transition_lanes.as_deref()
        } else {
            Some(self.lanes.as_ref())
        };
        if let Some(lanes) = lanes {
            for lane in lanes {
                if let Some(value) = lane.lane.value_at(frame) {
                    self.effective[lane.target.0] = value;
                    self.pre_instrument_unconstrained[lane.target.0] = value;
                }
            }
        }
        if self.transition_active && self.transition_position < self.transition_frames {
            self.transition_position = self.transition_position.saturating_add(1);
            let progress = self.transition_position as f64 / self.transition_frames as f64;
            for (index, changed) in self.transition_changed.iter().copied().enumerate() {
                if changed {
                    self.effective[index] = self.transition_origin[index]
                        + (self.effective[index] - self.transition_origin[index]) * progress;
                    self.pre_instrument_unconstrained[index] = self
                        .transition_pre_instrument_origin[index]
                        + (self.pre_instrument_unconstrained[index]
                            - self.transition_pre_instrument_origin[index])
                            * progress;
                }
            }
        }
        (&self.effective, &self.pre_instrument_unconstrained)
    }

    pub(crate) fn lanes_handle(&self) -> Arc<Vec<CompiledIndexedParameterLockLane>> {
        self.lanes.clone()
    }

    pub(crate) fn begin_handoff(
        &mut self,
        replacement_lanes: Option<Arc<Vec<CompiledIndexedParameterLockLane>>>,
        fallback_effective: &[f64],
        fallback_pre_instrument: &[f64],
        initialize_from_fallback: bool,
        frames: u64,
    ) -> bool {
        let current_lanes = self.lanes.as_ref();
        if !initialize_from_fallback && replacement_lanes.as_deref() == Some(current_lanes) {
            return false;
        }
        if initialize_from_fallback {
            self.effective.copy_from_slice(fallback_effective);
            self.pre_instrument_unconstrained
                .copy_from_slice(fallback_pre_instrument);
        }
        self.transition_origin.copy_from_slice(&self.effective);
        self.transition_pre_instrument_origin
            .copy_from_slice(&self.pre_instrument_unconstrained);
        self.transition_changed.fill(false);
        for lane in current_lanes
            .iter()
            .chain(replacement_lanes.iter().flat_map(|lanes| lanes.iter()))
        {
            self.transition_changed[lane.target.0] = true;
        }
        self.transition_lanes = replacement_lanes;
        self.transition_active = true;
        self.transition_position = 0;
        self.transition_frames = frames.max(1);
        true
    }

    pub(crate) fn finish_handoff(&mut self) -> bool {
        if !self.transition_active {
            return true;
        }
        let Some(lanes) = self.transition_lanes.take() else {
            self.transition_active = false;
            return false;
        };
        self.lanes = lanes;
        self.transition_active = false;
        true
    }

    pub(crate) fn adopt_values_from(&mut self, current: &mut Self) {
        std::mem::swap(&mut self.effective, &mut current.effective);
        std::mem::swap(
            &mut self.pre_instrument_unconstrained,
            &mut current.pre_instrument_unconstrained,
        );
    }

    #[cfg(test)]
    pub(crate) fn effective_value(&self, index: ControlIndex) -> f64 {
        self.effective[index.0]
    }

    #[cfg(test)]
    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.effective.capacity()
            + self.pre_instrument_unconstrained.capacity()
            + self.transition_origin.capacity()
            + self.transition_pre_instrument_origin.capacity()) as u64
            * std::mem::size_of::<f64>() as u64
            + packed_bool_allocation_bytes(self.transition_changed.capacity())
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
    pre_instrument_unconstrained: Vec<f64>,
    instrument_unconstrained: Vec<f64>,
    song_modulation: Vec<f64>,
    unconstrained: Vec<f64>,
    modulation: Vec<f64>,
    provenance: Vec<RuntimeProvenance>,
    instrument_route_values: Vec<RuntimeInstrumentRouteValue>,
    transition_origin: Vec<f64>,
    transition_changed: Vec<bool>,
    transition_position: u64,
    transition_frames: u64,
    apply_parameter_locks: bool,
}

impl ControlRuntime {
    /// Evaluates base → Automation → lock → Instrument routes → Song routes → constraint.
    pub(crate) fn evaluate_frame(&mut self, frame: u64) {
        let transition_progress = if self.transition_position < self.transition_frames {
            self.transition_position = self.transition_position.saturating_add(1);
            Some(self.transition_position as f64 / self.transition_frames as f64)
        } else {
            None
        };

        // Establish every absolute value before evaluating the same-frame macro DAG.
        for (index, target) in self.program.targets.iter().enumerate() {
            let automation = target.lane.as_ref().and_then(|lane| lane.value_at(frame));
            let parameter_lock = self
                .apply_parameter_locks
                .then(|| {
                    target
                        .parameter_locks
                        .as_ref()
                        .and_then(|lane| lane.value_at(frame))
                })
                .flatten();
            let absolute = parameter_lock.or(automation).unwrap_or(target.base);
            self.unconstrained[index] = absolute;
            self.provenance[index] = RuntimeProvenance {
                automation,
                parameter_lock,
                ..RuntimeProvenance::default()
            };
        }
        self.pre_instrument_unconstrained
            .copy_from_slice(&self.unconstrained);

        // Instrument routes are compiled in stable order. Parameter sources read this frame's
        // already-folded Instrument value, so validated acyclic macro chains have no hidden delay.
        for (route_index, route) in self.program.instrument_routes.iter().enumerate() {
            let primary = route.source.value(InstrumentSourceContext {
                frame,
                pitch: 0,
                velocity: 0.0,
                gate: false,
                controls: &self.unconstrained,
                graph_control: None,
                random_identity: 0,
            });
            let auxiliary = route.auxiliary.as_ref().and_then(|source| {
                source.value(InstrumentSourceContext {
                    frame,
                    pitch: 0,
                    velocity: 0.0,
                    gate: false,
                    controls: &self.unconstrained,
                    graph_control: None,
                    random_identity: 0,
                })
            });
            if let Some(primary) = primary {
                let contribution = route.contribution(primary, auxiliary);
                self.unconstrained[route.target.0] += contribution;
                self.provenance[route.target.0].modulation += contribution;
                self.instrument_route_values[route_index] = RuntimeInstrumentRouteValue {
                    normalized: remap_polarity(primary, route.source.polarity, route.polarity),
                    auxiliary,
                    contribution,
                };
            }
        }

        self.instrument_unconstrained
            .copy_from_slice(&self.unconstrained);
        for (index, target) in self.program.targets.iter().enumerate() {
            let mut song_modulation = self.modulation[index];
            for route in &target.routes {
                if let Some(contribution) = route.contribution_at(frame) {
                    song_modulation += contribution;
                }
            }
            self.song_modulation[index] = song_modulation;
            self.unconstrained[index] += song_modulation;
            let unconstrained = self.unconstrained[index];
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
            self.provenance[index].modulation += song_modulation;
            self.provenance[index].unconstrained = unconstrained;
            self.provenance[index].clamped =
                unconstrained.is_finite() && constrained != unconstrained;
            self.provenance[index].held_non_finite = held_non_finite;
            // Audio-derived routes publish a fresh future contribution for every sample.
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
            && self.program.instrument_routes.len() == replacement.program.instrument_routes.len()
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
        for (index, (current, replacement_target)) in self
            .program
            .targets
            .iter()
            .zip(replacement.program.targets.iter())
            .enumerate()
        {
            let lane_changed = current.lane != replacement_target.lane;
            let unautomated_base_changed = current.lane.is_none()
                && replacement_target.lane.is_none()
                && current.base.to_bits() != replacement_target.base.to_bits();
            let instrument_routes_changed = self
                .program
                .instrument_routes
                .iter()
                .filter(|route| route.target == ControlIndex(index))
                .ne(replacement
                    .program
                    .instrument_routes
                    .iter()
                    .filter(|route| route.target == ControlIndex(index)));
            let track_parameter_locks_changed = self
                .program
                .track_parameter_locks
                .iter()
                .filter_map(|(track, lanes)| {
                    lanes
                        .iter()
                        .find(|lane| lane.target == ControlIndex(index))
                        .map(|lane| (track, lane))
                })
                .ne(replacement.program.track_parameter_locks.iter().filter_map(
                    |(track, lanes)| {
                        lanes
                            .iter()
                            .find(|lane| lane.target == ControlIndex(index))
                            .map(|lane| (track, lane))
                    },
                ));
            let target_changed = current.unit != replacement_target.unit
                || current.minimum.to_bits() != replacement_target.minimum.to_bits()
                || current.maximum.to_bits() != replacement_target.maximum.to_bits()
                || lane_changed
                || current.parameter_locks != replacement_target.parameter_locks
                || track_parameter_locks_changed
                || current.routes != replacement_target.routes
                || instrument_routes_changed
                || unautomated_base_changed;
            self.transition_changed[index] = target_changed && !replacement_target.step_only;
            changed |= target_changed;
        }
        loop {
            let mut propagated = false;
            for route in replacement.program.instrument_routes.iter() {
                let dependency_changed = std::iter::once(&route.source)
                    .chain(route.auxiliary.as_ref())
                    .filter_map(|source| match source.kind {
                        CompiledInstrumentSourceKind::Parameter { parameter } => Some(parameter),
                        _ => None,
                    })
                    .any(|parameter| self.transition_changed[parameter.0]);
                if dependency_changed
                    && !replacement.program.targets[route.target.0].step_only
                    && !self.transition_changed[route.target.0]
                {
                    self.transition_changed[route.target.0] = true;
                    changed = true;
                    propagated = true;
                }
            }
            if !propagated {
                break;
            }
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

    /// Dense values after base, Automation, and locks but before any Instrument route.
    #[must_use]
    pub(crate) fn pre_instrument_unconstrained_values(&self) -> &[f64] {
        &self.pre_instrument_unconstrained
    }

    /// Precomputed values for global Instrument routes in compiled order.
    #[must_use]
    pub(crate) fn instrument_route_values(&self) -> &[RuntimeInstrumentRouteValue] {
        &self.instrument_route_values
    }

    /// Dense aggregate Song-owned contributions applied after Instrument routes.
    #[must_use]
    pub(crate) fn song_modulation_values(&self) -> &[f64] {
        &self.song_modulation
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn transition_changed(&self) -> &[bool] {
        &self.transition_changed
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn track_parameter_locks_differ(&self, replacement: &Self, track_id: &str) -> bool {
        self.program.track_parameter_locks.get(track_id)
            != replacement.program.track_parameter_locks.get(track_id)
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn reset_transition_changes(&mut self) {
        self.transition_changed.fill(false);
    }

    /// Adds one future modulation contribution before the next evaluation.
    pub(crate) fn add_modulation(&mut self, index: ControlIndex, contribution: f64) {
        self.modulation[index.0] += contribution;
    }

    /// Clears pending Audio-derived contributions after an explicit transport discontinuity.
    pub(crate) fn clear_future_modulation(&mut self) {
        self.modulation.fill(0.0);
    }
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledPerVoiceRoute {
    Instrument(CompiledInstrumentRoute),
    SongTier {
        target: ControlIndex,
        minimum: f64,
        maximum: f64,
    },
    Song {
        target: ControlIndex,
        minimum: f64,
        maximum: f64,
        route: CompiledRoute,
    },
}

impl CompiledPerVoiceRoute {
    pub(crate) fn allocation_bytes(&self) -> u64 {
        match self {
            Self::Instrument(route) => route.heap_bytes(),
            Self::SongTier { .. } => 0,
            Self::Song { route, .. } => route.heap_bytes(),
        }
    }

    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Instrument(route) => route.id(),
            Self::SongTier { .. } => "@song-tier",
            Self::Song { route, .. } => &route.id,
        }
    }

    pub(crate) fn target(&self) -> ControlIndex {
        match self {
            Self::Instrument(route) => route.target(),
            Self::SongTier { target, .. } | Self::Song { target, .. } => *target,
        }
    }

    pub(crate) fn instrument(&self) -> Option<&CompiledInstrumentRoute> {
        match self {
            Self::Instrument(route) => Some(route),
            Self::SongTier { .. } | Self::Song { .. } => None,
        }
    }

    pub(crate) fn transition_target(&self, replacement: &Self) -> Result<Option<ControlIndex>, ()> {
        match (self, replacement) {
            (Self::Instrument(current), Self::Instrument(replacement))
                if current.topology_compatible(replacement) =>
            {
                Ok(current
                    .scalar_changed(replacement)
                    .then_some(current.target))
            }
            (
                Self::SongTier {
                    target: current_target,
                    minimum: current_minimum,
                    maximum: current_maximum,
                },
                Self::SongTier {
                    target: replacement_target,
                    minimum: replacement_minimum,
                    maximum: replacement_maximum,
                },
            ) if current_target == replacement_target
                && current_minimum.to_bits() == replacement_minimum.to_bits()
                && current_maximum.to_bits() == replacement_maximum.to_bits() =>
            {
                Ok(None)
            }
            (
                Self::Song {
                    target: current_target,
                    minimum: current_minimum,
                    maximum: current_maximum,
                    route: current_route,
                },
                Self::Song {
                    target: replacement_target,
                    minimum: replacement_minimum,
                    maximum: replacement_maximum,
                    route: replacement_route,
                },
            ) if current_target == replacement_target
                && current_minimum.to_bits() == replacement_minimum.to_bits()
                && current_maximum.to_bits() == replacement_maximum.to_bits()
                && current_route == replacement_route =>
            {
                Ok(None)
            }
            _ => Err(()),
        }
    }

    pub(crate) fn parameter_dependencies(&self) -> impl Iterator<Item = ControlIndex> + '_ {
        self.instrument()
            .into_iter()
            .flat_map(CompiledInstrumentRoute::parameter_dependencies)
    }

    pub(crate) fn contribution_at(&self, voice_frame: u64, song_modulation: &[f64]) -> f64 {
        match self {
            Self::Instrument(_) => 0.0,
            Self::SongTier { target, .. } => song_modulation[target.0],
            Self::Song { route, .. } => route.normalized_at(voice_frame) * route.depth,
        }
    }

    pub(crate) fn constrain(&self, value: f64, previous: f64) -> f64 {
        let (minimum, maximum) = match self {
            Self::Instrument(route) => (route.minimum, route.maximum),
            Self::SongTier {
                minimum, maximum, ..
            }
            | Self::Song {
                minimum, maximum, ..
            } => (*minimum, *maximum),
        };
        if value.is_finite() {
            value.clamp(minimum, maximum)
        } else {
            previous
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeInstrumentRouteValue {
    normalized: f64,
    auxiliary: Option<f64>,
    contribution: f64,
}

impl RuntimeInstrumentRouteValue {
    pub(crate) const fn contribution(self) -> f64 {
        self.contribution
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
    /// Stable Song-owned route provenance in deterministic route-ID order.
    pub modulation_routes: Vec<ModulationValueSnapshot>,
    /// Stable Instrument-owned route provenance in effective route order.
    pub instrument_modulation_routes: Vec<InstrumentModulationValueSnapshot>,
    /// Value before domain constraint.
    pub unconstrained: f64,
    /// Final finite physical value.
    pub effective: f64,
    /// Whether domain constraint changed this sample's finite value.
    pub clamped: bool,
    /// Whether a non-finite result caused the prior finite value to be held.
    pub held_non_finite: bool,
}

/// Inspectable contribution from one Instrument-owned Modulation route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstrumentModulationValueSnapshot {
    /// Durable Instrument-local route identity.
    pub route: String,
    /// Stable Instrument-local source identity.
    pub source: String,
    /// Authored source execution scope.
    pub scope: InstrumentModulationScope,
    /// Authored primary route order.
    pub order: i32,
    /// Durable route bypass value.
    pub bypass: bool,
    /// Route-mapped normalized primary value, or `None` for per-voice state.
    pub normalized: Option<f64>,
    /// Evaluated auxiliary value, when singular and present.
    pub auxiliary: Option<f64>,
    /// Physical-unit route depth.
    pub depth: f64,
    /// Physical contribution, or `None` when no singular Song-time value exists.
    pub contribution: Option<f64>,
}

/// Inspectable contribution from one Song-owned Modulation route.
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
    fn targeted_inspector_matches_fresh_whole_program_snapshots_across_seeks() {
        let project = project();
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let target = ParameterTarget::Track {
            track: "drums".to_owned(),
            parameter: crate::model::TrackParameter::Level,
        };
        let mut inspector = program.clone().into_inspector();
        for frame in [24_000, 0, 48_000, 12_000] {
            let expected = program
                .snapshot_at(frame)
                .into_iter()
                .find(|snapshot| snapshot.target == target)
                .expect("whole-program target");
            let actual = inspector
                .snapshot_target_at(frame, &target)
                .expect("targeted snapshot");
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn indexed_instrument_route_inspection_matches_whole_program_snapshot() {
        let source = include_str!("../../tests/fixtures/instrument_modulation.crump");
        let document = parse_project(Path::new("project.crump"), source);
        let project = document.project.expect("instrument modulation Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
        let program = CompiledControlProgram::compile(&project).expect("compile controls");
        let frame = 12_345;
        let expected = program
            .snapshot_at(frame)
            .into_iter()
            .filter_map(|snapshot| match snapshot.target {
                ParameterTarget::Instrument { instrument, .. } if instrument == "lead" => Some(
                    snapshot
                        .instrument_modulation_routes
                        .into_iter()
                        .map(move |route| (snapshot.unit, route)),
                ),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let actual = program
            .into_inspector()
            .snapshot_instrument_routes_at(frame, "lead");
        assert_eq!(actual, expected);
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

        let track_runtime = program
            .track_parameter_lock_runtime("lead")
            .expect("lead Track lock runtime");
        assert_eq!(
            track_runtime.allocation_bytes(),
            program.track_parameter_lock_runtime_state_bytes(),
            "admission must account for every preallocated Track lock-projection buffer"
        );
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
