use super::*;

/// Immutable processing graph compiled away from the callback.
#[derive(Debug, Clone)]
pub struct CompiledMixGraph {
    pub(super) program: Arc<CompiledMixProgram>,
    pub(super) inspection: MixGraphInspection,
    /// Profile enforced while compiling, or `None` for estimate-only compilation.
    pub(super) admission: Option<AdmissionProfile>,
}

#[derive(Debug, Clone)]
pub(super) struct ChannelDraft {
    pub(super) identity: String,
    pub(super) role: ChannelRole,
    pub(super) source_index: Option<usize>,
    pub(super) source_processors: Vec<CompiledProcessorInstance>,
    pub(super) inserts: Vec<CompiledProcessorInstance>,
    pub(super) level: MixScalar,
    pub(super) panner: Option<CompiledPanner>,
    pub(super) mute: MixScalar,
}

#[derive(Debug, Clone)]
pub(super) struct RouteDraft {
    pub(super) id: String,
    pub(super) role: RouteRole,
    pub(super) from: String,
    pub(super) owner: String,
    pub(super) destination: DraftDestination,
    pub(super) tap: TapPoint,
    pub(super) gain: Option<MixScalar>,
    pub(super) processor_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) enum DraftDestination {
    Bus(String),
    Auxiliary { slot: usize, display: String },
}

impl CompiledMixGraph {
    /// Compiles all authored processing scopes in a stable topological order.
    #[cfg(test)]
    pub(crate) fn compile(
        project: &Project,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
        let topology = AuthoredTopology::compile(project);
        Self::compile_with_topology(
            project,
            controls,
            &topology,
            Some(AdmissionProfile::Live),
            ProcessorAssets::Unloaded,
        )
    }

    pub(crate) fn compile_with_topology(
        project: &Project,
        controls: &CompiledControlProgram,
        topology: &AuthoredTopology,
        admission: Option<AdmissionProfile>,
        assets: ProcessorAssets<'_>,
    ) -> Result<Self, EngineError> {
        if let Some(finding) = topology.findings().first() {
            return Err(EngineError::Compile(routing_finding_message(&finding.kind)));
        }
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let processor_indexes = processor_telemetry_indices(project, topology)?;
        let mut drafts = BTreeMap::new();
        let mut processor_inspection = Vec::new();
        let mut authored_tracks = project.song.tracks.iter().collect::<Vec<_>>();
        authored_tracks.sort_by(|left, right| left.id.cmp(&right.id));
        for (source_index, track) in authored_tracks.into_iter().enumerate() {
            let instrument = instruments.get(track.instrument.as_str()).ok_or_else(|| {
                EngineError::Compile(format!(
                    "Track `{}` references missing instrument `{}`",
                    track.id, track.instrument
                ))
            })?;
            let identity = format!("track/{}", track.id);
            let source_processors = compile_processor_chain(
                &instrument.post_processors,
                &format!("instrument/{}@{}", instrument.id, track.id),
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
                assets,
            )?;
            let inserts = compile_processor_chain(
                &track.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
                assets,
            )?;
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Track,
                    source_index: Some(source_index),
                    source_processors,
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &track.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                },
            );
        }
        for group in &project.song.groups {
            let identity = format!("group/{}", group.id);
            let inserts = compile_processor_chain(
                &group.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
                assets,
            )?;
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Group,
                    source_index: None,
                    source_processors: Vec::new(),
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &group.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                },
            );
        }
        for channel in &project.song.returns {
            let identity = format!("return/{}", channel.id);
            let inserts = compile_processor_chain(
                &channel.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
                assets,
            )?;
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Return,
                    source_index: None,
                    source_processors: Vec::new(),
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &channel.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                },
            );
        }
        let master_inserts = if let Some(master) = &project.song.master {
            compile_processor_chain(
                &master.inserts,
                "master",
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
                assets,
            )?
        } else {
            Vec::new()
        };
        drafts.insert(
            "master".to_owned(),
            ChannelDraft {
                identity: "master".to_owned(),
                role: ChannelRole::Master,
                source_index: None,
                source_processors: Vec::new(),
                inserts: master_inserts,
                level: if project.song.master.is_some() {
                    MixScalar::from_target(controls, &ParameterTarget::Master)?
                } else {
                    MixScalar::Constant(0.0)
                },
                panner: None,
                mute: MixScalar::Constant(0.0),
            },
        );

        let mut route_drafts = Vec::with_capacity(topology.routes().len());
        let mut auxiliary_slots = 0_usize;
        for route in topology.routes() {
            let role = match route.role() {
                TopologyRouteRole::Main => RouteRole::Main,
                TopologyRouteRole::Send => RouteRole::Send,
                TopologyRouteRole::Sidechain => RouteRole::Sidechain,
            };
            let (destination, processor_index) = match route.destination() {
                TopologyDestination::Bus(identity) => {
                    (DraftDestination::Bus(identity.to_string()), None)
                }
                TopologyDestination::SidechainInput {
                    owner,
                    processor_id,
                    processor_index,
                    input,
                } => {
                    let slot = auxiliary_slots;
                    auxiliary_slots = auxiliary_slots.saturating_add(1);
                    let owner_name = owner.to_string();
                    let owner_draft = drafts.get_mut(&owner_name).ok_or_else(|| {
                        EngineError::Compile(format!("sidechain owner `{owner}` is missing"))
                    })?;
                    owner_draft.inserts[*processor_index]
                        .auxiliary_slots
                        .push(slot);
                    (
                        DraftDestination::Auxiliary {
                            slot,
                            display: format!("processor/{processor_id}/{input}"),
                        },
                        Some(*processor_index),
                    )
                }
            };
            let gain = match route.declaration() {
                RouteDeclaration::Send {
                    channel,
                    send_index,
                } => {
                    let send = authored_channel_strip(project, channel)
                        .and_then(|strip| strip.sends.get(send_index))
                        .ok_or_else(|| {
                            EngineError::Compile(format!(
                                "route `{}` lost its authored Send",
                                route.id()
                            ))
                        })?;
                    Some(MixScalar::from_target(
                        controls,
                        &ParameterTarget::Send {
                            send: send.id.clone(),
                        },
                    )?)
                }
                RouteDeclaration::Main(_) | RouteDeclaration::Sidechain(_) => None,
            };
            route_drafts.push(RouteDraft {
                id: route.id().to_owned(),
                role,
                from: route.source().to_string(),
                owner: route.owner().to_string(),
                destination,
                tap: route.tap(),
                gain,
                processor_index,
            });
        }

        let order = topology
            .order()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut channels = Vec::with_capacity(order.len());
        for identity in &order {
            let draft = drafts.remove(identity).ok_or_else(|| {
                EngineError::Compile(format!("compiled channel `{identity}` disappeared"))
            })?;
            channels.push(CompiledChannel {
                identity: draft.identity,
                role: draft.role,
                source_index: draft.source_index,
                source_processors: draft.source_processors,
                inserts: draft.inserts,
                level: draft.level,
                panner: draft.panner,
                mute: draft.mute,
                outgoing_routes: Vec::new(),
                source_compensation_frames: 0,
                pre_latency_frames: 0,
                post_latency_frames: 0,
            });
        }
        let indexes: BTreeMap<_, _> = channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.identity.clone(), index))
            .collect();
        route_drafts.sort_by(|left, right| left.id.cmp(&right.id));
        let mut routes = Vec::with_capacity(route_drafts.len());
        for draft in route_drafts {
            let from = *indexes.get(&draft.from).ok_or_else(|| {
                EngineError::Compile(format!("route `{}` has missing source", draft.id))
            })?;
            let owner = *indexes.get(&draft.owner).ok_or_else(|| {
                EngineError::Compile(format!("route `{}` has missing destination", draft.id))
            })?;
            let (destination, destination_display) = match draft.destination {
                DraftDestination::Bus(identity) => {
                    let destination = *indexes.get(&identity).ok_or_else(|| {
                        EngineError::Compile(format!("route `{}` has missing Bus", draft.id))
                    })?;
                    (RouteDestination::Bus(destination), identity)
                }
                DraftDestination::Auxiliary { slot, display } => {
                    (RouteDestination::Auxiliary(slot), display)
                }
            };
            let index = routes.len();
            channels[from].outgoing_routes.push(index);
            routes.push(CompiledRoute {
                id: draft.id,
                role: draft.role,
                from,
                owner,
                destination,
                destination_display,
                tap: draft.tap,
                gain: draft.gain,
                processor_index: draft.processor_index,
                compensation_frames: 0,
            });
        }
        compile_latency(&mut channels, &mut routes)?;
        let track_count = project.song.tracks.len();
        let mut track_channels = vec![0_usize; track_count];
        for (index, channel) in channels.iter().enumerate() {
            if let Some(source_index) = channel.source_index {
                track_channels[source_index] = index;
            }
        }
        let master_channel = *indexes
            .get("master")
            .ok_or_else(|| EngineError::Compile("missing compiled Master".to_owned()))?;
        let solo_masks = compile_solo_masks(&channels, &routes, master_channel);
        let program = Arc::new(CompiledMixProgram {
            channels,
            routes,
            track_channels,
            master_channel,
            auxiliary_slots,
            solo_masks,
        });
        let mut resources = RuntimeResourceEstimate::for_project(project, controls.target_count())?;
        resources.include_compiled_state(&program);
        if let Some(profile) = admission {
            enforce_admission(&resources, profile)?;
        }
        let inspection = compile_inspection(
            &program.channels,
            &program.routes,
            processor_inspection,
            program.master_channel,
            resources,
        );
        Ok(Self {
            program,
            inspection,
            admission,
        })
    }

    #[cfg(test)]
    pub(crate) fn resource_estimate_for_validation(
        project: &Project,
        topology: &AuthoredTopology,
    ) -> Result<RuntimeResourceEstimate, EngineError> {
        Self::resource_estimate_with_processor_assets(project, topology, ProcessorAssets::Unloaded)
    }

    pub(crate) fn resource_estimate_with_processor_assets(
        project: &Project,
        topology: &AuthoredTopology,
        assets: ProcessorAssets<'_>,
    ) -> Result<RuntimeResourceEstimate, EngineError> {
        let schedules = compile_schedules(project)?;
        let controls = CompiledControlProgram::compile(project)?;
        let mut graph = Self::compile_with_topology(project, &controls, topology, None, assets)?;
        let envelope_followers = super::super::follower::CompiledEnvelopeFollowerProgram::compile(
            project,
            &controls,
            &graph,
            project.sample_rate,
        )?;
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let schedule_program_bytes = schedules
            .iter()
            .map(super::super::schedule::TrackSchedule::allocation_bytes)
            .fold(0_u64, u64::saturating_add);
        let mut instrument_program_bytes = 0_u64;
        for schedule in &schedules {
            let instrument = instruments
                .get(schedule.instrument_id.as_str())
                .ok_or_else(|| {
                    EngineError::Compile(format!("missing instrument `{}`", schedule.instrument_id))
                })?;
            if instrument.kind == InstrumentKind::Graph {
                let voice_graph =
                    CompiledGraph::compile(instrument, project, project.sample_rate, &controls)?;
                let shared_graph = CompiledGraph::compile_shared(
                    instrument,
                    project,
                    project.sample_rate,
                    &controls,
                )?;
                instrument_program_bytes = instrument_program_bytes
                    .saturating_add(voice_graph.allocation_bytes())
                    .saturating_add(
                        shared_graph
                            .as_ref()
                            .map_or(0, CompiledGraph::allocation_bytes),
                    );
            }
        }
        let graph_track_count = schedules
            .iter()
            .filter(|schedule| {
                instruments
                    .get(schedule.instrument_id.as_str())
                    .is_some_and(|instrument| instrument.kind == InstrumentKind::Graph)
            })
            .count() as u64;
        let voicing_runtime_state_bytes = graph_track_count
            .saturating_mul(std::mem::size_of::<super::super::GraphVoicingRuntime>() as u64);
        let song_runtime_structure_bytes = (std::mem::size_of::<SongRuntime>() as u64)
            .saturating_add(project.id.len() as u64)
            .saturating_add(voicing_runtime_state_bytes)
            .saturating_add(
                (schedules.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<TrackRuntime>() as u64),
            )
            .saturating_add(controls.runtime_state_bytes())
            .saturating_add(controls.track_parameter_lock_runtime_state_bytes())
            .saturating_add(envelope_followers.runtime_state_bytes());
        graph.inspection.resources.include_song_runtime_state(
            controls
                .allocation_bytes()
                .saturating_add(envelope_followers.allocation_bytes()),
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        );
        let voicing_work_units_per_frame = schedules
            .iter()
            .filter_map(|schedule| instruments.get(schedule.instrument_id.as_str()))
            .filter(|instrument| {
                instrument.kind == InstrumentKind::Graph
                    && instrument.voicing.glide_mode != crate::model::GlideMode::Off
            })
            .map(|instrument| u64::from(instrument.polyphony))
            .fold(0_u64, u64::saturating_add);
        graph.inspection.resources.include_additional_callback_work(
            controls
                .instrument_work_units_per_frame()
                .saturating_add(envelope_followers.work_units_per_frame())
                .saturating_add(voicing_work_units_per_frame),
        );
        Ok(graph.inspection.resources)
    }

    fn enforce_admission(&self) -> Result<(), EngineError> {
        match self.admission {
            Some(profile) => enforce_admission(&self.inspection.resources, profile),
            None => Ok(()),
        }
    }

    pub(crate) fn include_additional_callback_work(
        &mut self,
        work_units_per_frame: u64,
    ) -> Result<(), EngineError> {
        self.inspection
            .resources
            .include_additional_callback_work(work_units_per_frame);
        self.enforce_admission()
    }

    pub(crate) fn include_song_runtime_state(
        &mut self,
        control_program_bytes: u64,
        schedule_program_bytes: u64,
        instrument_program_bytes: u64,
        song_runtime_structure_bytes: u64,
    ) -> Result<(), EngineError> {
        self.inspection.resources.include_song_runtime_state(
            control_program_bytes,
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        );
        self.enforce_admission()
    }

    /// Creates fully allocated mutable state for one playback/render runtime.
    #[must_use]
    pub(crate) fn runtime(&self) -> MixGraphRuntime {
        MixGraphRuntime {
            program: Arc::clone(&self.program),
            channels: self
                .program
                .channels
                .iter()
                .map(CompiledChannel::runtime)
                .collect(),
            route_delays: self
                .program
                .routes
                .iter()
                .map(|route| CompensationDelay::new(route.compensation_frames as usize))
                .collect(),
            route_gains: self
                .program
                .routes
                .iter()
                .map(|route| route.gain.map(GainRuntime::new))
                .collect(),
            bus_inputs: vec![[0.0, 0.0]; self.program.channels.len()],
            auxiliary_inputs: vec![[0.0, 0.0]; self.program.auxiliary_slots],
            meters: vec![ChannelMeter::default(); self.program.channels.len()],
            processor_meters: vec![ProcessorMeter::default(); self.inspection.processors.len()],
            channel_taps: [ChannelTapSamples::default(); MAX_MIX_CHANNELS],
            soloed: vec![false; self.program.channels.len()],
            cue: None,
            #[cfg(test)]
            reset_calls: 0,
        }
    }

    /// Returns the complete inspectable processing program and admission facts.
    #[must_use]
    pub fn inspection(&self) -> &MixGraphInspection {
        &self.inspection
    }
}

fn authored_channel_strip(
    project: &Project,
    declaration: ChannelDeclaration,
) -> Option<&crate::model::ChannelStrip> {
    match declaration {
        ChannelDeclaration::Track(index) => {
            project.song.tracks.get(index).map(|track| &track.channel)
        }
        ChannelDeclaration::Group(index) => {
            project.song.groups.get(index).map(|group| &group.channel)
        }
        ChannelDeclaration::Return(index) => project
            .song
            .returns
            .get(index)
            .map(|channel| &channel.channel),
        ChannelDeclaration::Master => None,
    }
}

fn routing_finding_message(finding: &RoutingFindingKind) -> String {
    match finding {
        RoutingFindingKind::InvalidChannelId { role, id, .. } => {
            format!("{} id `{id}` is invalid or duplicated", role.display_name())
        }
        RoutingFindingKind::InvalidProcessorId { id } => {
            format!("processor id `{id}` is invalid or duplicated")
        }
        RoutingFindingKind::UnresolvedMain {
            owner, destination, ..
        } => format!("{owner} routes to missing `{destination}`"),
        RoutingFindingKind::IllegalReturnMain { role, owner } => format!(
            "{} `{owner}` cannot use a Return as its main output",
            role.display_name()
        ),
        RoutingFindingKind::InvalidSendId { id } => {
            format!("Send id `{id}` is invalid or duplicated")
        }
        RoutingFindingKind::MissingSendReturn { send, target } => {
            format!("Send `{send}` targets missing Return `{target}`")
        }
        RoutingFindingKind::InvalidSidechainId { id } => {
            format!("sidechain route id `{id}` is invalid or duplicated")
        }
        RoutingFindingKind::MissingSidechainSource { sidechain, source } => {
            format!("sidechain `{sidechain}` references missing source `{source}`")
        }
        RoutingFindingKind::MissingSidechainProcessor {
            sidechain,
            processor,
        } => format!("sidechain `{sidechain}` references missing channel processor `{processor}`"),
        RoutingFindingKind::UndeclaredSidechainInput {
            sidechain,
            input,
            processor,
        } => format!(
            "sidechain `{sidechain}` targets undeclared input `{input}` on processor `{processor}`"
        ),
        RoutingFindingKind::DuplicateSidechainDestination { processor, .. } => {
            format!("processor `{processor}` has more than one sidechain route")
        }
        RoutingFindingKind::UnconnectedSidechainInput { processor, .. } => {
            format!("processor `{processor}` has an unconnected declared sidechain input")
        }
        RoutingFindingKind::Cycle => "compiled channel graph contains a route cycle".to_owned(),
    }
}

/// Rejects an estimate that exceeds any limit of `profile`, naming each failed limit.
pub(super) fn enforce_admission(
    resources: &RuntimeResourceEstimate,
    profile: AdmissionProfile,
) -> Result<(), EngineError> {
    let failures = resources.admission_failures(profile);
    if failures.is_empty() {
        return Ok(());
    }
    Err(EngineError::Compile(format!(
        "Project exceeds the {} budget: {}",
        profile.label(),
        describe_admission_failures(&failures)
    )))
}
