use super::*;

pub(super) fn processor_telemetry_indices(
    project: &Project,
    topology: &AuthoredTopology,
) -> Result<BTreeMap<(String, String), usize>, EngineError> {
    let mut identities = BTreeSet::new();
    for processor in topology.processors() {
        match processor.owner() {
            ProcessorOwner::Channel { channel, .. } => {
                identities.insert((channel.to_string(), processor.id().to_owned()));
            }
            ProcessorOwner::Instrument { instrument_id, .. } => {
                for track in project
                    .song
                    .tracks
                    .iter()
                    .filter(|track| track.instrument == *instrument_id)
                {
                    identities.insert((
                        format!("instrument/{instrument_id}@{}", track.id),
                        processor.id().to_owned(),
                    ));
                }
            }
        }
    }
    Ok(identities
        .into_iter()
        .enumerate()
        .map(|(index, identity)| (identity, index))
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_processor_chain(
    processors: &[crate::model::Processor],
    owner: &str,
    controls: &CompiledControlProgram,
    sample_rate: u32,
    tempo_bpm: f64,
    processor_indexes: &BTreeMap<(String, String), usize>,
    inspection: &mut Vec<CompiledProcessorInspection>,
    assets: ProcessorAssets<'_>,
) -> Result<Vec<CompiledProcessorInstance>, EngineError> {
    processors
        .iter()
        .map(|processor| {
            let program =
                ProcessorProgram::compile(processor, controls, sample_rate, tempo_bpm, assets)?;
            let tail_frames_upper_bound = program.tail_frames_upper_bound();
            let tail_policy = if program.uses_project_render_tail() {
                ProcessorTailPolicy::ProjectRenderTail
            } else if tail_frames_upper_bound.is_some() {
                ProcessorTailPolicy::BoundedFrames
            } else {
                ProcessorTailPolicy::None
            };
            let telemetry_index = *processor_indexes
                .get(&(owner.to_owned(), processor.id.clone()))
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "processor `{}` has no stable telemetry index for `{owner}`",
                        processor.id
                    ))
                })?;
            inspection.push(CompiledProcessorInspection {
                index: telemetry_index,
                id: processor.id.clone(),
                kind: processor.kind,
                owner: owner.to_owned(),
                authored_bypass: processor.bypass,
                state_policy: ProcessorStatePolicy::Continue,
                settings: resolved_processor_settings(processor, tempo_bpm)?,
                sidechain_inputs: processor.sidechain_inputs.clone(),
                authored_latency_frames: program.authored_latency_frames(),
                intrinsic_latency_frames: program.intrinsic_latency_frames(),
                output_latency_frames: program.output_latency_frames(),
                nonlinear_substeps: program.nonlinear_substeps(),
                oversampling_factor: program.oversampling_factor(),
                oversampling_filter_taps: program.oversampling_filter_taps(),
                true_peak_safety_margin_db: program.true_peak_safety_margin_db(),
                tail_policy,
                tail_frames_upper_bound,
                state_bytes: program.state_bytes(),
                work_units_per_frame: program.work_units(),
            });
            Ok(CompiledProcessorInstance {
                program,
                auxiliary_slots: Vec::new(),
                telemetry_index,
            })
        })
        .collect()
}

pub(super) fn compile_latency(
    channels: &mut [CompiledChannel],
    routes: &mut [CompiledRoute],
) -> Result<(), EngineError> {
    for channel_index in 0..channels.len() {
        let mut input_latency = 0_u32;
        for route in routes.iter().filter(|route| {
            route.owner == channel_index && matches!(route.destination, RouteDestination::Bus(_))
        }) {
            input_latency = input_latency.max(route_source_latency(channels, route));
        }
        for route in routes.iter().filter(|route| {
            route.owner == channel_index
                && matches!(route.destination, RouteDestination::Auxiliary(_))
        }) {
            let processor_index = route.processor_index.ok_or_else(|| {
                EngineError::Compile(format!("sidechain route `{}` has no processor", route.id))
            })?;
            let offset = channels[channel_index].processor_input_offset(processor_index);
            input_latency =
                input_latency.max(route_source_latency(channels, route).saturating_sub(offset));
        }
        if channels[channel_index].source_index.is_some() {
            channels[channel_index].source_compensation_frames = input_latency;
        }
        for route in routes
            .iter_mut()
            .filter(|route| route.owner == channel_index)
        {
            let arrival = match route.tap {
                TapPoint::PreFader => channels[route.from].pre_latency_frames,
                TapPoint::PostFader => channels[route.from].post_latency_frames,
            };
            let target = if let Some(processor_index) = route.processor_index {
                input_latency
                    .saturating_add(channels[channel_index].processor_input_offset(processor_index))
            } else {
                input_latency
            };
            route.compensation_frames = target.checked_sub(arrival).ok_or_else(|| {
                EngineError::Compile(format!(
                    "latency convergence for route `{}` could not be represented",
                    route.id
                ))
            })?;
        }
        let output_latency =
            input_latency.saturating_add(channels[channel_index].total_processor_latency());
        channels[channel_index].pre_latency_frames = output_latency;
        channels[channel_index].post_latency_frames = output_latency;
    }
    Ok(())
}

pub(super) fn route_source_latency(channels: &[CompiledChannel], route: &CompiledRoute) -> u32 {
    match route.tap {
        TapPoint::PreFader => channels[route.from].pre_latency_frames,
        TapPoint::PostFader => channels[route.from].post_latency_frames,
    }
}

pub(super) fn compile_solo_masks(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    master: usize,
) -> Vec<Vec<bool>> {
    let mut masks = Vec::with_capacity(channels.len());
    for selected in 0..channels.len() {
        let mut mask = vec![false; channels.len()];
        mask[selected] = true;
        // A Group solo includes descendants on main-output paths, but never
        // walks backward through Master and accidentally selects all channels.
        let mut changed = true;
        while changed {
            changed = false;
            for route in routes.iter().filter(|route| route.role == RouteRole::Main) {
                if route.owner != master && mask[route.owner] && !mask[route.from] {
                    mask[route.from] = true;
                    changed = true;
                }
            }
        }
        // Preserve every selected channel's main ancestry and Return branches.
        changed = true;
        while changed {
            changed = false;
            for route in routes
                .iter()
                .filter(|route| route.role != RouteRole::Sidechain)
            {
                if mask[route.from] && !mask[route.owner] {
                    mask[route.owner] = true;
                    changed = true;
                }
            }
        }
        mask[master] = true;
        masks.push(mask);
    }
    masks
}

pub(super) fn chains_compatible(
    left: &[CompiledProcessorInstance],
    right: &[CompiledProcessorInstance],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.program.topology_compatible(&right.program)
                && left.auxiliary_slots == right.auxiliary_slots
                && left.telemetry_index == right.telemetry_index
        })
}

pub(super) fn db_gain(decibels: f64) -> f32 {
    10.0_f64.powf(decibels.clamp(-192.0, 96.0) / 20.0) as f32
}
