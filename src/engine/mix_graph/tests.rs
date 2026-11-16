use std::path::Path;

use crate::{
    dsl::parse_project,
    model::{OutputTarget, Panner},
    validate::validate_project,
};

use super::admission::channel_and_master_processors;
use super::*;

fn project(track_order: &str) -> Project {
    let source = format!(
        "crump 1\nproject \"Mix\" {{\n  id mix\n  seed 1\n  sample_rate 48000\n  render_tail 0s\n  song {{\n    tempo 120\n    meter 4/4\n    end 2bar\n    instrument drums \"Drums\" stock_drums {{\n      polyphony 8\n      parameter tone \"Tone\" ratio 0 1 0.5 0.5\n      faceplate {{\n        control tone \"Tone\" \"Percussion brightness\" maps tone\n      }}\n    }}\n    pattern beat 1bar {{\n      hit 1.1 kick 1\n    }}\n{track_order}\n  }}\n}}\n"
    );
    let parsed = parse_project(Path::new("project.crump"), &source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    project
}

#[test]
fn packed_boolean_admission_rounds_to_machine_words() {
    let word = std::mem::size_of::<usize>() as u64;
    assert_eq!(vec_bool_allocation_bytes(0), 0);
    assert_eq!(vec_bool_allocation_bytes(1), word);
    assert_eq!(vec_bool_allocation_bytes(usize::BITS as usize), word);
    assert_eq!(
        vec_bool_allocation_bytes(usize::BITS as usize + 1),
        word * 2
    );
    assert_eq!(vec_bool_allocation_bytes(97), word * 2);
}

#[test]
fn runtime_program_is_a_direct_lowering_of_the_canonical_authored_topology() {
    let parsed = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../../tests/fixtures/production_mix.crump"),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let topology = AuthoredTopology::compile(&project);
    assert!(topology.is_valid(), "{:?}", topology.findings());

    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile_with_topology(&project, &controls, &topology, true)
        .expect("canonical topology lowers");
    let topology_channels = topology
        .order()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let runtime_channels = graph
        .inspection
        .channels
        .iter()
        .map(|channel| channel.identity.clone())
        .collect::<Vec<_>>();
    assert_eq!(runtime_channels, topology_channels);
    let topology_routes = topology
        .routes()
        .iter()
        .map(|route| route.id().to_owned())
        .collect::<Vec<_>>();
    let runtime_routes = graph
        .inspection
        .routes
        .iter()
        .map(|route| route.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(runtime_routes, topology_routes);
}

#[test]
fn stable_track_identity_controls_float_summing_order() {
    let first = project(
        "    track b \"B\" instrument drums level -1db pan 0 {\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }\n    track a \"A\" instrument drums level -2db pan -0.25 {\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }",
    );
    let second = project(
        "    track a \"A\" instrument drums level -2db pan -0.25 {\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }\n    track b \"B\" instrument drums level -1db pan 0 {\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }",
    );
    let first_controls = CompiledControlProgram::compile(&first).expect("controls");
    let second_controls = CompiledControlProgram::compile(&second).expect("controls");
    let first_mix = CompiledMixGraph::compile(&first, &first_controls).expect("mix");
    let second_mix = CompiledMixGraph::compile(&second, &second_controls).expect("mix");
    assert_eq!(first_mix.inspection.routes, second_mix.inspection.routes);
    assert_eq!(
        first_mix.inspection.signal_paths,
        second_mix.inspection.signal_paths
    );
}

#[test]
fn compilation_rejects_unconnected_duplicate_or_undeclared_sidechain_inputs() {
    let parsed = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../../tests/fixtures/production_mix.crump"),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");

    let mut disconnected = project.clone();
    disconnected.song.sidechains.clear();
    let controls = CompiledControlProgram::compile(&disconnected).expect("controls");
    let error = CompiledMixGraph::compile(&disconnected, &controls)
        .expect_err("declared sidechain without a route must not compile");
    assert!(error.to_string().contains("unconnected declared sidechain"));

    let mut duplicate = project.clone();
    let mut duplicate_route = duplicate.song.sidechains[0].clone();
    duplicate_route.id = "duplicate_detector".to_owned();
    duplicate.song.sidechains.push(duplicate_route);
    let controls = CompiledControlProgram::compile(&duplicate).expect("controls");
    let error = CompiledMixGraph::compile(&duplicate, &controls)
        .expect_err("two routes to one sidechain input must not compile");
    assert!(error.to_string().contains("more than one sidechain route"));

    let mut undeclared = project;
    undeclared.song.sidechains[0].input = "missing".to_owned();
    let controls = CompiledControlProgram::compile(&undeclared).expect("controls");
    let error = CompiledMixGraph::compile(&undeclared, &controls)
        .expect_err("route to an undeclared sidechain input must not compile");
    assert!(error.to_string().contains("targets undeclared input"));
}

#[test]
fn integrated_sidechain_fixture_creates_kick_driven_bass_movement() {
    let parsed = parse_project(
        Path::new("compressor_sidechain.crump"),
        include_str!("../../../tests/fixtures/compressor_sidechain.crump"),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let mut project = parsed.project.expect("Project");
    project
        .song
        .tracks
        .iter_mut()
        .find(|track| track.id == "kick")
        .expect("kick Track")
        .channel
        .level_db = -96.0;
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
    let control_runtime = controls.runtime();
    let control_values = control_runtime.values();
    assert!(graph.inspection.routes.iter().any(|route| {
        route.role == RouteRole::Sidechain
            && route.from == "track/kick"
            && route.to == "processor/bass_comp/detector"
            && route.tap == TapPoint::PreFader
    }));
    let compressor_index = graph
        .inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bass_comp")
        .expect("bass compressor")
        .index;
    let mut runtime = graph.runtime();
    let bass = [0.25, -0.25];
    let silence = [0.0, 0.0];
    let kick = [1.0, 1.0];
    let mut baseline = [0.0, 0.0];
    for _ in 0..480 {
        baseline = runtime.process(&[bass, silence], control_values, false);
    }
    let mut ducked = baseline;
    for _ in 0..480 {
        ducked = runtime.process(&[bass, kick], control_values, false);
    }
    assert!(ducked[0].abs() < baseline[0].abs() * 0.14);
    assert!(runtime.processor_meters[compressor_index].gain_reduction_db > 17.9);

    let mut recovering = ducked;
    for _ in 0..4_800 {
        recovering = runtime.process(&[bass, silence], control_values, false);
    }
    assert!(recovering[0].abs() > ducked[0].abs() * 1.9);
    assert!(recovering[0].abs() < baseline[0].abs() * 0.4);
    let mut recovered = recovering;
    for _ in 4_800..14_400 {
        recovered = runtime.process(&[bass, silence], control_values, false);
    }
    assert!((recovered[0] - baseline[0]).abs() < 1.0e-5);
    assert!(runtime.processor_meters[compressor_index].gain_reduction_db < 1.0e-4);
    assert!(runtime.processor_meters[compressor_index].maximum_gain_reduction_db > 17.9);
}

#[test]
fn processor_telemetry_indexes_survive_declaration_reordering() {
    let parsed = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../../tests/fixtures/production_mix.crump"),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let mut first = parsed.project.expect("Project");

    let mut extra_group = first.song.groups[0].clone();
    extra_group.id = "aux_group".to_owned();
    extra_group.channel.sends.clear();
    for processor in &mut extra_group.channel.inserts {
        processor.id = format!("aux_{}", processor.id);
    }
    first.song.groups.push(extra_group);
    let mut extra_return = first.song.returns[0].clone();
    extra_return.id = "aux_return".to_owned();
    for processor in &mut extra_return.channel.inserts {
        processor.id = format!("aux_{}", processor.id);
    }
    first.song.returns.push(extra_return);

    let mut second = first.clone();
    second.song.groups.reverse();
    second.song.returns.reverse();
    for project in [&first, &second] {
        let diagnostics = validate_project(project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    }
    let first_controls = CompiledControlProgram::compile(&first).expect("controls");
    let second_controls = CompiledControlProgram::compile(&second).expect("controls");
    let first_graph = CompiledMixGraph::compile(&first, &first_controls).expect("mix");
    let second_graph = CompiledMixGraph::compile(&second, &second_controls).expect("mix");
    assert!(
        first_graph
            .program
            .topology_compatible(&second_graph.program)
    );

    let first_indexes = first_graph
        .inspection
        .processors
        .iter()
        .map(|processor| {
            (
                (processor.owner.clone(), processor.id.clone()),
                processor.index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let second_indexes = second_graph
        .inspection
        .processors
        .iter()
        .map(|processor| {
            (
                (processor.owner.clone(), processor.id.clone()),
                processor.index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(first_indexes, second_indexes);

    let expected_index = first_indexes
        .get(&("group/aux_group".to_owned(), "aux_bus_eq".to_owned()))
        .copied()
        .expect("auxiliary Group processor index");
    let mut current = first_graph.runtime();
    current.processor_meters[expected_index].gain_reduction_db = 7.5;
    let mut replacement = second_graph.runtime();
    replacement.adopt_state_from(&mut current);
    let published_index = second_indexes
        .get(&("group/aux_group".to_owned(), "aux_bus_eq".to_owned()))
        .copied()
        .expect("published auxiliary Group processor index");
    assert_eq!(
        replacement.processor_meters[published_index].gain_reduction_db,
        7.5
    );
}

#[test]
fn estimate_includes_voice_nodes_buffers_and_transition_headroom() {
    let project = project(
        "    track drums \"Drums\" instrument drums level 0db pan 0 {\n      clip beat_clip pattern beat start 1.1 duration 1bar mode once\n    }",
    );
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
    let resources = &graph.inspection.resources;
    assert_eq!(resources.channels, 2);
    assert!(resources.bus_buffer_bytes > 0);
    assert!(resources.meter_bytes > 0);
    assert_eq!(
        resources.publication_peak_bytes,
        resources.runtime_bytes * 2
    );
    assert_eq!(
        resources.publication_transition_work_units_per_frame,
        resources
            .work_units_per_frame
            .saturating_mul(MAX_RESIDENT_PUBLICATION_RUNTIMES)
            .saturating_add((resources.tracks as u64).saturating_mul(4))
            .saturating_add((resources.channels as u64).saturating_mul(8))
            .saturating_add((resources.control_targets as u64).saturating_mul(2))
    );
    assert!(resources.admitted);

    let mut near_budget = resources.clone();
    near_budget.work_units_per_frame = MAX_WORK_UNITS_PER_FRAME / 2;
    near_budget.recalculate_memory_and_admission();
    assert!(
        !near_budget.admitted,
        "two-runtime publication work must be admitted, not only steady-state work"
    );
}

#[test]
fn production_fixture_compiles_every_slice_two_route_role() {
    let source = include_str!("../../../tests/fixtures/production_mix.crump");
    let parsed = parse_project(Path::new("production_mix.crump"), source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    for processor in project
        .song
        .instruments
        .iter()
        .flat_map(|instrument| &instrument.post_processors)
        .chain(channel_and_master_processors(&project))
    {
        let program = ProcessorProgram::compile(
            processor,
            &controls,
            project.sample_rate,
            project.song.tempo_bpm,
        )
        .expect("DSP");
        assert_eq!(
            authored_processor_estimate(processor, project.sample_rate, project.song.tempo_bpm,),
            (program.state_bytes(), program.work_units()),
            "authored admission estimate for {}",
            processor.id
        );
    }
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
    let inspection = graph.inspection();
    let processor_kinds = inspection
        .processors
        .iter()
        .map(|processor| processor.kind)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        processor_kinds,
        [
            ProcessorKind::Utility,
            ProcessorKind::Eq,
            ProcessorKind::Compressor,
            ProcessorKind::Gate,
            ProcessorKind::Delay,
            ProcessorKind::Reverb,
            ProcessorKind::Saturation,
            ProcessorKind::ModulationDelay,
            ProcessorKind::Crusher,
            ProcessorKind::Stutter,
            ProcessorKind::Limiter,
            ProcessorKind::Latency,
        ]
        .into_iter()
        .collect()
    );
    let modulation_delay = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bus_chorus")
        .expect("modulation-delay inspection");
    assert_eq!(
        modulation_delay.tail_policy,
        ProcessorTailPolicy::ProjectRenderTail
    );
    assert_eq!(modulation_delay.tail_frames_upper_bound, None);
    assert!(modulation_delay.state_bytes > 3_000 * 8);
    let crusher = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bus_crusher")
        .expect("crusher inspection");
    assert_eq!(crusher.tail_policy, ProcessorTailPolicy::BoundedFrames);
    assert_eq!(crusher.tail_frames_upper_bound, Some(960));
    assert_eq!(
        crusher
            .settings
            .iter()
            .find(|setting| setting.key == "bit_depth")
            .and_then(|setting| setting.unit),
        Some(Unit::Bits)
    );
    let stutter = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bus_stutter")
        .expect("stutter inspection");
    assert_eq!(stutter.tail_policy, ProcessorTailPolicy::BoundedFrames);
    assert_eq!(stutter.tail_frames_upper_bound, Some(24_000));
    assert!(stutter.state_bytes >= 6_000 * 8);
    assert!(
        inspection
            .channels
            .iter()
            .any(|channel| channel.role == ChannelRole::Group)
    );
    assert!(
        inspection
            .channels
            .iter()
            .any(|channel| channel.role == ChannelRole::Return)
    );
    assert!(
        inspection
            .routes
            .iter()
            .any(|route| route.role == RouteRole::Send)
    );
    assert!(
        inspection
            .routes
            .iter()
            .any(|route| route.role == RouteRole::Sidechain)
    );
    assert!(
        inspection
            .channels
            .iter()
            .any(|channel| channel.source_compensation_frames > 0)
    );
    assert_eq!(inspection.resources.supported_quanta, [64, 128, 256]);
    assert!(inspection.resources.admitted);
    assert_eq!(
        inspection.resources.compiled_program_bytes,
        graph.program.allocation_bytes()
    );
    assert_eq!(
        inspection.resources.runtime_structure_bytes,
        graph.program.runtime_structure_bytes()
    );
    assert_eq!(
        inspection.resources.runtime_bytes,
        inspection
            .resources
            .bus_buffer_bytes
            .saturating_add(inspection.resources.meter_bytes)
            .saturating_add(inspection.resources.processor_meter_bytes)
            .saturating_add(inspection.resources.compiled_program_bytes)
            .saturating_add(inspection.resources.runtime_structure_bytes)
            .saturating_add(inspection.resources.graph_voice_state_bytes)
            .saturating_add(inspection.resources.drum_voice_state_bytes)
            .saturating_add(inspection.resources.sampler_voice_state_bytes)
            .saturating_add(inspection.resources.processor_state_bytes)
            .saturating_add(inspection.resources.compensation_state_bytes)
            .saturating_add(inspection.resources.control_program_bytes)
            .saturating_add(inspection.resources.schedule_program_bytes)
            .saturating_add(inspection.resources.instrument_program_bytes)
            .saturating_add(inspection.resources.song_runtime_structure_bytes)
    );
    assert_eq!(
        inspection.resources.processor_meter_bytes,
        (inspection.processors.len() + MAX_PROCESSOR_INSTANCES) as u64
            * std::mem::size_of::<ProcessorMeter>() as u64
    );
    for (index, processor) in inspection.processors.iter().enumerate() {
        assert_eq!(processor.index, index);
        assert!(processor.work_units_per_frame > 0);
    }
    let compressor = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bass_comp")
        .expect("compressor inspection");
    let threshold = compressor
        .settings
        .iter()
        .find(|setting| setting.key == "threshold")
        .expect("resolved threshold");
    assert_eq!(threshold.unit, Some(Unit::Decibels));
    assert_eq!(threshold.value, Some(-24.0));
    assert_eq!(threshold.minimum, Some(-96.0));
    assert!(threshold.authored);
    assert!(threshold.parameter_target);
    let detector = compressor
        .settings
        .iter()
        .find(|setting| setting.key == "detector")
        .expect("resolved default detector");
    assert_eq!(detector.choice.as_deref(), Some("peak"));
    assert!(!detector.authored);
    assert!(!detector.parameter_target);
    let reverb = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "room_reverb")
        .expect("Reverb inspection");
    assert!(reverb.state_bytes >= (u64::from(project.sample_rate) + 1) * 8);
    let saturation = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "master_saturation")
        .expect("Saturation inspection");
    assert_eq!(saturation.nonlinear_substeps, 4);
    assert_eq!(saturation.oversampling_factor, 4);
    assert_eq!(saturation.oversampling_filter_taps, 129);
    assert_eq!(saturation.intrinsic_latency_frames, 32);
    let limiter = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "master_limiter")
        .expect("Limiter inspection");
    assert_eq!(limiter.authored_latency_frames, 0);
    assert_eq!(limiter.oversampling_factor, 4);
    assert_eq!(limiter.oversampling_filter_taps, 129);
    assert_eq!(limiter.true_peak_safety_margin_db, 0.1);
    assert_eq!(limiter.intrinsic_latency_frames, 256);
    assert_eq!(limiter.output_latency_frames, 256);
    let lookahead = limiter
        .settings
        .iter()
        .find(|setting| setting.key == "lookahead")
        .expect("resolved structural lookahead");
    assert!(lookahead.authored);
    assert!(!lookahead.parameter_target);
    let send_path = inspection
        .signal_paths
        .iter()
        .find(|path| path.source == "send/drum_room")
        .expect("Send path");
    assert!(send_path.stages.windows(3).any(|stages| stages
        == [
            "processor/room_reverb",
            "return/room",
            "processor/master_saturation"
        ]));
    assert_eq!(send_path.stages.last().map(String::as_str), Some("master"));
    let sidechain_path = inspection
        .signal_paths
        .iter()
        .find(|path| path.source == "sidechain/kick_detector")
        .expect("sidechain path");
    for stage in [
        "processor/bass_comp",
        "track/bass",
        "processor/bus_eq",
        "group/mix_bus",
        "processor/master_limiter",
        "master",
    ] {
        assert!(sidechain_path.stages.iter().any(|actual| actual == stage));
    }
    let mut runtime = graph.runtime();
    assert!(Arc::ptr_eq(&runtime.program, &graph.program));
    assert_eq!(Arc::strong_count(&graph.program), 2);
    let (activity, channel_count) = runtime.channel_activity();
    assert_eq!(channel_count, inspection.channels.len());
    assert_eq!(
        activity[..channel_count],
        vec![ChannelMeter::default(); channel_count]
    );
    let mut processor_activity = [ProcessorMeter::default(); MAX_PROCESSOR_INSTANCES];
    assert_eq!(
        runtime.copy_processor_activity(&mut processor_activity),
        inspection.processors.len()
    );
    assert_eq!(
        processor_activity[..inspection.processors.len()],
        vec![ProcessorMeter::default(); inspection.processors.len()]
    );

    let bass_source = graph
        .program
        .channels
        .iter()
        .find(|channel| channel.identity == "track/bass")
        .and_then(|channel| channel.source_index)
        .expect("Bass source");
    let drums_source = graph
        .program
        .channels
        .iter()
        .find(|channel| channel.identity == "track/drums")
        .and_then(|channel| channel.source_index)
        .expect("Drums source");
    let control_runtime = controls.runtime();
    let mut sources = vec![[0.0_f32, 0.0_f32]; graph.program.track_channels.len()];
    for _ in 0..2_000 {
        sources[bass_source] = [0.01, 0.01];
        sources[drums_source] = [1.0, 1.0];
        let _ = runtime.process(&sources, control_runtime.values(), false);
    }
    runtime.copy_processor_activity(&mut processor_activity);
    assert!(
        processor_activity[compressor.index].gain_reduction_db > 1.0,
        "external sidechain should drive measurable compressor reduction"
    );
    let gate = inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bass_gate")
        .expect("gate inspection");
    for _ in 0..10_000 {
        sources[bass_source] = [0.1, 0.1];
        sources[drums_source] = [0.0, 0.0];
        let _ = runtime.process(&sources, control_runtime.values(), false);
    }
    runtime.copy_processor_activity(&mut processor_activity);
    assert!(
        processor_activity[gate.index].gain_reduction_db > 1.0,
        "aligned external sidechain silence should close the Gate"
    );
}

#[test]
fn inspection_resolves_defaults_for_every_processor_kind() {
    let parsed = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../../tests/fixtures/production_mix.crump"),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let mut project = parsed.project.expect("Project");
    let mut delay = project.song.groups[0].channel.inserts[0].clone();
    delay.id = "inspection_delay".to_owned();
    delay.kind = ProcessorKind::Delay;
    delay.settings.clear();
    delay.sidechain_inputs.clear();
    delay.latency_frames = 0;
    delay.latency_site = None;
    project.song.groups[0].channel.inserts.push(delay.clone());
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");

    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
    let kinds = graph
        .inspection
        .processors
        .iter()
        .map(|processor| processor.kind)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            ProcessorKind::Utility,
            ProcessorKind::Eq,
            ProcessorKind::Compressor,
            ProcessorKind::Gate,
            ProcessorKind::Delay,
            ProcessorKind::Reverb,
            ProcessorKind::Saturation,
            ProcessorKind::ModulationDelay,
            ProcessorKind::Crusher,
            ProcessorKind::Stutter,
            ProcessorKind::Limiter,
            ProcessorKind::Latency,
        ])
    );
    let inspected = graph
        .inspection
        .processors
        .iter()
        .find(|processor| processor.id == delay.id)
        .expect("Delay inspection");
    assert_eq!(
        inspected
            .settings
            .iter()
            .map(|setting| {
                (
                    setting.key.as_str(),
                    setting.value,
                    setting.choice.as_deref(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("time", Some(0.25), None),
            ("tempo_sync", None, Some("false")),
            ("feedback", Some(0.25), None),
            ("mix", Some(0.25), None),
            ("low_pass", Some(20_000.0), None),
            ("high_pass", Some(20.0), None),
            ("time_crossfade", Some(0.01), None),
            ("mode", None, Some("stereo")),
        ]
    );
    assert!(
        inspected
            .settings
            .iter()
            .all(|setting| !setting.authored && !setting.parameter_target)
    );
    let program = ProcessorProgram::compile(
        &delay,
        &controls,
        project.sample_rate,
        project.song.tempo_bpm,
    )
    .expect("Delay DSP");
    assert_eq!(
        authored_processor_estimate(&delay, project.sample_rate, project.song.tempo_bpm,),
        (program.state_bytes(), program.work_units())
    );
    let reverb = &project.song.returns[0].channel.inserts[0];
    let program = ProcessorProgram::compile(
        reverb,
        &controls,
        project.sample_rate,
        project.song.tempo_bpm,
    )
    .expect("Reverb DSP");
    assert_eq!(
        authored_processor_estimate(reverb, project.sample_rate, project.song.tempo_bpm),
        (program.state_bytes(), program.work_units())
    );
}

#[test]
fn tempo_synced_ping_pong_delay_resolves_time_and_exact_state() {
    let source = include_str!("../../../tests/fixtures/production_mix.crump").replace(
            "    master level -1db {",
            "    master level -1db {\n      processor synced_delay delay {\n        setting tempo_sync true\n        setting division 1/8d\n        setting mode ping_pong\n        setting feedback 0.4\n        setting mix 0.5\n        setting time_crossfade 0.02s\n      }",
        );
    let parsed = parse_project(Path::new("synced-delay.crump"), &source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
    let delay = graph
        .inspection
        .processors
        .iter()
        .find(|processor| processor.id == "synced_delay")
        .expect("synced Delay");
    let setting = |key: &str| {
        delay
            .settings
            .iter()
            .find(|setting| setting.key == key)
            .unwrap_or_else(|| panic!("missing `{key}`"))
    };
    assert_eq!(setting("time").value, Some(0.375));
    assert!(!setting("time").authored);
    assert!(!setting("time").parameter_target);
    assert_eq!(setting("tempo_sync").choice.as_deref(), Some("true"));
    assert_eq!(setting("division").choice.as_deref(), Some("1/8d"));
    assert_eq!(setting("mode").choice.as_deref(), Some("ping_pong"));
    assert_eq!(setting("time_crossfade").value, Some(0.02));
    assert_eq!(
        delay.state_bytes,
        std::mem::size_of::<ProcessorRuntime>() as u64
            + (18_000_u64 + 2) * std::mem::size_of::<[f32; 2]>() as u64
            + 2 * std::mem::size_of::<[f32; 2]>() as u64
    );

    let conflicting = source.replace(
        "        setting division 1/8d",
        "        setting division 1/8d\n        setting time 0.2s",
    );
    let parsed = parse_project(Path::new("conflicting-delay.crump"), &conflicting);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let error = CompiledMixGraph::compile(&project, &controls)
        .expect_err("compiled Delay must reject free time while synchronized");
    assert!(error.to_string().contains("cannot author `time`"));

    let orphaned_division = source.replace(
        "        setting tempo_sync true",
        "        setting tempo_sync false",
    );
    let parsed = parse_project(Path::new("orphaned-division.crump"), &orphaned_division);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let error = CompiledMixGraph::compile(&project, &controls)
        .expect_err("compiled Delay must reject a division while tempo sync is disabled");
    assert!(error.to_string().contains("unless tempo sync is enabled"));
}

#[test]
fn return_reverb_uses_one_shared_state_and_preserves_its_tail() {
    let project = project(
        "    track a \"A\" instrument drums level 0db pan 0 {\n      send a_room target return/room tap pre level 0db\n      mute true\n      output master\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }\n    track b \"B\" instrument drums level 0db pan 0 {\n      send b_room target return/room tap pre level 0db\n      mute true\n      output master\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }\n    return room \"Room\" level 0db {\n      processor shared_room reverb {\n        setting pre_delay 0s\n        setting diffusion 0.7\n        setting decay 0.85\n        setting damping 0.2\n        setting modulation 0.1\n        setting width 1\n        setting mix 1\n      }\n      output master\n    }\n    master level 0db {\n    }",
    );
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
    let room_index = graph
        .program
        .channels
        .iter()
        .position(|channel| channel.identity == "return/room")
        .expect("Room Return");
    assert_eq!(graph.program.channels[room_index].inserts.len(), 1);
    assert_eq!(
        graph
            .program
            .routes
            .iter()
            .filter(|route| {
                route.role == RouteRole::Send
                    && route.destination == RouteDestination::Bus(room_index)
            })
            .count(),
        2
    );

    let control_runtime = controls.runtime();
    let mut runtime = graph.runtime();
    let mut first_wet = None;
    let mut late_maximum = 0.0_f32;
    for frame in 0..96_000 {
        let sources = if frame == 0 {
            [[1.0, 1.0], [0.5, 0.5]]
        } else {
            [[0.0, 0.0]; 2]
        };
        let output = runtime.process(&sources, control_runtime.values(), false);
        let peak = output[0].abs().max(output[1].abs());
        if first_wet.is_none() && peak > 1.0e-7 {
            first_wet = Some(frame);
        }
        if frame >= 48_000 {
            late_maximum = late_maximum.max(peak);
        }
    }
    assert!(first_wet.is_some_and(|frame| frame < 6_000));
    assert!(late_maximum > 1.0e-7);
}

#[test]
fn eq_inspection_exposes_every_active_band_value_and_safe_frequency_domain() {
    let source = include_str!("../../../tests/fixtures/production_mix.crump").replace(
            "        setting mid1_q 1",
            "        setting mid1_q 1\n        setting low_frequency 120hz\n        setting low_gain 2db\n        setting mid2_frequency 2.5khz\n        setting mid2_gain -1db\n        setting mid2_q 1.5\n        setting high_frequency 8khz\n        setting high_gain 1db\n        setting low_pass 20khz",
        );
    let parsed = parse_project(Path::new("all-eq.crump"), &source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let controls = CompiledControlProgram::compile(&project).expect("controls");
    let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
    let equalizer = graph
        .inspection
        .processors
        .iter()
        .find(|processor| processor.id == "bus_eq")
        .expect("EQ inspection");
    assert_eq!(
        equalizer
            .settings
            .iter()
            .map(|setting| setting.key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "high_pass",
            "low_frequency",
            "low_gain",
            "mid1_frequency",
            "mid1_gain",
            "mid1_q",
            "mid2_frequency",
            "mid2_gain",
            "mid2_q",
            "high_frequency",
            "high_gain",
            "low_pass",
        ]
    );
    assert_eq!(
        equalizer
            .settings
            .iter()
            .map(|setting| (setting.key.as_str(), setting.value))
            .collect::<Vec<_>>(),
        vec![
            ("high_pass", Some(20.0)),
            ("low_frequency", Some(120.0)),
            ("low_gain", Some(2.0)),
            ("mid1_frequency", Some(350.0)),
            ("mid1_gain", Some(-1.0)),
            ("mid1_q", Some(1.0)),
            ("mid2_frequency", Some(2_500.0)),
            ("mid2_gain", Some(-1.0)),
            ("mid2_q", Some(1.5)),
            ("high_frequency", Some(8_000.0)),
            ("high_gain", Some(1.0)),
            ("low_pass", Some(20_000.0)),
        ]
    );
    assert!(
        equalizer
            .settings
            .iter()
            .all(|setting| setting.authored && setting.parameter_target)
    );
    for setting in equalizer.settings.iter().filter(|setting| {
        setting.key.contains("frequency") || setting.key == "high_pass" || setting.key == "low_pass"
    }) {
        assert_eq!(setting.unit, Some(Unit::Hertz));
        assert_eq!(setting.minimum, Some(10.0));
        assert_eq!(setting.maximum, Some(23_520.0));
    }
}

#[test]
fn latency_compensation_aligns_every_convergence_at_supported_quanta() {
    fn production_project() -> Project {
        let parsed = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../../../tests/fixtures/production_mix.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.project.expect("Project")
    }

    fn neutralize(project: &mut Project) -> (Panner, crate::model::Processor) {
        let balance = project.song.tracks[0].channel.panner.clone();
        let latency = project.song.tracks[0].channel.inserts[0].clone();
        for instrument in &mut project.song.instruments {
            instrument.post_processors.clear();
        }
        for track in &mut project.song.tracks {
            track.channel.level_db = 0.0;
            track.channel.panner = balance.clone();
            track.channel.mute = false;
            track.channel.inserts.clear();
            track.channel.sends.clear();
        }
        for group in &mut project.song.groups {
            group.channel.level_db = 0.0;
            group.channel.panner = balance.clone();
            group.channel.mute = false;
            group.channel.inserts.clear();
            group.channel.sends.clear();
            group.channel.output = OutputTarget::Master;
        }
        for channel in &mut project.song.returns {
            channel.channel.level_db = 0.0;
            channel.channel.panner = balance.clone();
            channel.channel.mute = false;
            channel.channel.inserts.clear();
            channel.channel.sends.clear();
            channel.channel.output = OutputTarget::Master;
        }
        let master = project.song.master.as_mut().expect("Master");
        master.level_db = 0.0;
        master.inserts.clear();
        project.song.sidechains.clear();
        project.song.envelope_followers.clear();
        (balance, latency)
    }

    fn assert_aligned(project: &Project, expected_frame: usize) {
        let diagnostics = validate_project(project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(project).expect("controls");
        let graph = CompiledMixGraph::compile(project, &controls).expect("mix graph");
        for quantum in SUPPORTED_QUANTA {
            let mut runtime = graph.runtime();
            let mut control_runtime = controls.runtime();
            let mut active_frames = Vec::new();
            for block_start in (0..32).step_by(quantum) {
                for frame in block_start..(block_start + quantum).min(32) {
                    control_runtime.evaluate_frame(frame as u64);
                    let mut sources = [[0.0_f32; 2]; MAX_RUNTIME_TRACKS];
                    if frame == 0 {
                        for source in sources.iter_mut().take(project.song.tracks.len()) {
                            *source = [1.0, 1.0];
                        }
                    }
                    let output = runtime.process(
                        &sources[..project.song.tracks.len()],
                        control_runtime.values(),
                        false,
                    );
                    if output[0].abs().max(output[1].abs()) > 1.0e-5 {
                        active_frames.push(frame);
                    }
                }
            }
            assert_eq!(active_frames, [expected_frame], "quantum {quantum}");
        }
    }

    // Two main routes with unequal insert latency converge at one Group.
    let mut main = production_project();
    let (_, mut latency) = neutralize(&mut main);
    main.song.returns.clear();
    for track in &mut main.song.tracks {
        track.channel.output = OutputTarget::Group("mix_bus".to_owned());
    }
    latency.id = "main_latency".to_owned();
    main.song.tracks[0].channel.inserts.push(latency);
    assert_aligned(&main, 5);

    // A direct main route and a parallel Send/Return route converge at Master.
    let mut parallel = production_project();
    let (_, mut latency) = neutralize(&mut parallel);
    parallel.song.tracks.retain(|track| track.id == "drums");
    parallel.song.groups.clear();
    parallel.song.returns.truncate(1);
    parallel.song.tracks[0].channel.output = OutputTarget::Master;
    let mut send = production_project().song.tracks[0].channel.sends[0].clone();
    send.level_db = 0.0;
    send.tap = TapPoint::PostFader;
    parallel.song.tracks[0].channel.sends.push(send);
    latency.id = "return_latency".to_owned();
    parallel.song.returns[0].channel.inserts.push(latency);
    assert_aligned(&parallel, 5);

    // A delayed pre-fader sidechain and direct target source meet at the
    // compressor input; the muted key channel remains absent from Master.
    let mut sidechain = production_project();
    let (_, mut latency) = neutralize(&mut sidechain);
    sidechain.song.groups.clear();
    sidechain.song.returns.clear();
    for track in &mut sidechain.song.tracks {
        track.channel.output = OutputTarget::Master;
    }
    let drums = sidechain
        .song
        .tracks
        .iter_mut()
        .find(|track| track.id == "drums")
        .expect("drums");
    latency.id = "key_latency".to_owned();
    drums.channel.inserts.push(latency);
    drums.channel.mute = true;
    let compressor = production_project().song.tracks[1].channel.inserts[0].clone();
    sidechain
        .song
        .tracks
        .iter_mut()
        .find(|track| track.id == "bass")
        .expect("bass")
        .channel
        .inserts
        .push(compressor);
    let mut route = production_project().song.sidechains[0].clone();
    route.tap = TapPoint::PreFader;
    sidechain.song.sidechains.push(route);
    assert_aligned(&sidechain, 5);
}

#[test]
fn invariant_fader_and_panner_coefficients_are_cached() {
    let mut gain = GainRuntime::new(MixScalar::Constant(-6.0));
    for _ in 0..128 {
        let _ = gain.coefficient(&[]);
    }
    assert_eq!(gain.coefficient_updates, 0);

    let panner = CompiledPanner::Balance(MixScalar::Dynamic {
        index: ControlIndex(0),
        initial: 0.25,
    });
    let mut runtime = panner.runtime();
    for _ in 0..128 {
        let _ = runtime.process([1.0, 1.0], &[0.25]);
    }
    assert_eq!(runtime.coefficient_updates, 0);
    let _ = runtime.process([1.0, 1.0], &[0.5]);
    assert_eq!(runtime.coefficient_updates, 1);
}

#[test]
fn panner_modes_have_explicit_distinct_behavior() {
    let controls = [];
    let mono = CompiledPanner::MonoPan(MixScalar::Constant(0.0)).process([1.0, -1.0], &controls);
    assert_eq!(mono, [0.0, 0.0]);
    let balance = CompiledPanner::Balance(MixScalar::Constant(1.0)).process([1.0, 1.0], &controls);
    assert!(balance[0].abs() < 1.0e-6 && (balance[1] - 1.0).abs() < 1.0e-6);
    let stereo = CompiledPanner::StereoPan {
        left: MixScalar::Constant(-1.0),
        right: MixScalar::Constant(1.0),
    }
    .process([1.0, 1.0], &controls);
    assert!((stereo[0] - 1.0).abs() < 1.0e-6);
    assert!((stereo[1] - 1.0).abs() < 1.0e-6);
}
