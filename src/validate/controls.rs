//! Song Automation and Modulation control validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{
        AutomationInterpolation, ChannelRef, ControlPolarity, InstrumentKind,
        InstrumentModulationScope, InstrumentModulationSourceKind, LfoRetrigger, ModulationCurve,
        ModulationOperator, ModulationRate, ModulationScope, NodeKind, ParameterTarget, Project,
        SettingValue, Unit,
    },
};

use super::{
    MAX_AUTOMATION_LANES, MAX_AUTOMATION_POINTS, MAX_AUTOMATION_POINTS_PER_LANE,
    MAX_CONTROL_TARGETS, MAX_INSTRUMENT_LFOS, MAX_INSTRUMENT_MACROS,
    MAX_INSTRUMENT_MODULATION_SOURCES, MAX_MODULATION_OPERATORS,
    MAX_MODULATION_OPERATORS_PER_ROUTE, MAX_MODULATION_ROUTES, located_error, source_site_error,
    valid_id, valid_key,
};

pub(super) fn validate_automations(project: &Project, diagnostics: &mut Diagnostics) {
    let control_targets = project.parameter_specs().len();
    if control_targets > MAX_CONTROL_TARGETS {
        diagnostics.push(Diagnostic::error(
            "CRV118",
            format!(
                "Project declares {control_targets} callback-time Parameter targets; maximum is {MAX_CONTROL_TARGETS}"
            ),
        ));
    }
    if project.song.automations.len() > MAX_AUTOMATION_LANES {
        diagnostics.push(Diagnostic::error(
            "CRV100",
            format!(
                "Song declares {} Automation lanes; maximum is {MAX_AUTOMATION_LANES}",
                project.song.automations.len()
            ),
        ));
    }
    let total_points = project
        .song
        .automations
        .iter()
        .map(|lane| lane.points.len())
        .sum::<usize>();
    if total_points > MAX_AUTOMATION_POINTS {
        diagnostics.push(Diagnostic::error(
            "CRV101",
            format!(
                "Song declares {total_points} Automation points; maximum is {MAX_AUTOMATION_POINTS}"
            ),
        ));
    }

    let mut lane_ids = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for lane in &project.song.automations {
        if !valid_id(&lane.id) || !lane_ids.insert(lane.id.as_str()) {
            located_error(
                diagnostics,
                "CRV102",
                format!("Automation lane id `{}` is invalid or duplicated", lane.id),
                &lane.location,
            );
        }
        if !targets.insert(lane.target.clone()) {
            located_error(
                diagnostics,
                "CRV103",
                format!(
                    "Parameter target `{}` has more than one absolute Automation lane",
                    lane.target
                ),
                &lane.location,
            );
        }
        let Some(spec) = project.parameter_spec(&lane.target) else {
            located_error(
                diagnostics,
                "CRV104",
                format!("Automation targets missing Parameter `{}`", lane.target),
                &lane.location,
            );
            continue;
        };
        if !spec.automatable {
            located_error(
                diagnostics,
                "CRV105",
                format!("Parameter `{}` is not declared automatable", lane.target),
                &lane.location,
            );
        }
        if !automation_target_is_runtime_safe(project, &lane.target) {
            located_error(
                diagnostics,
                "CRV106",
                format!(
                    "Parameter `{}` controls runtime allocation and cannot be automated safely",
                    lane.target
                ),
                &lane.location,
            );
        }
        if lane.points.is_empty() {
            located_error(
                diagnostics,
                "CRV107",
                "Automation lane must contain at least one point at Song time zero",
                &lane.location,
            );
            continue;
        }
        if lane.points.len() > MAX_AUTOMATION_POINTS_PER_LANE {
            located_error(
                diagnostics,
                "CRV108",
                format!(
                    "Automation lane contains {} points; maximum is {MAX_AUTOMATION_POINTS_PER_LANE}",
                    lane.points.len()
                ),
                &lane.location,
            );
        }
        if lane.points[0].at != crate::time::MusicalTime::ZERO {
            located_error(
                diagnostics,
                "CRV109",
                "Automation lane's first point must be at Song time zero",
                &lane.points[0].location,
            );
        }
        let mut point_ids = BTreeSet::new();
        let mut previous_time = None;
        let mut previous_frame = None;
        for point in &lane.points {
            if !valid_id(&point.id) || !point_ids.insert(point.id.as_str()) {
                located_error(
                    diagnostics,
                    "CRV110",
                    format!(
                        "Automation point id `{}` is invalid or duplicated in lane `{}`",
                        point.id, lane.id
                    ),
                    &point.location,
                );
            }
            if point.at < crate::time::MusicalTime::ZERO || point.at > project.song.end {
                source_site_error(
                    diagnostics,
                    "CRV111",
                    "Automation point must lie between Song time zero and the arrangement end",
                    &point.at_site,
                );
            }
            if previous_time.is_some_and(|previous| point.at <= previous) {
                source_site_error(
                    diagnostics,
                    "CRV112",
                    "Automation point positions must be strictly increasing and unique",
                    &point.at_site,
                );
            }
            match point
                .at
                .to_frames(project.song.tempo_bpm, project.sample_rate)
            {
                Ok(frame) => {
                    if previous_frame.is_some_and(|previous| frame <= previous) {
                        source_site_error(
                            diagnostics,
                            "CRV113",
                            "Automation points must resolve to distinct increasing sample frames",
                            &point.at_site,
                        );
                    }
                    previous_frame = Some(frame);
                }
                Err(error) => source_site_error(
                    diagnostics,
                    "CRV114",
                    format!("Automation point cannot resolve to sample time: {error}"),
                    &point.at_site,
                ),
            }
            previous_time = Some(point.at);

            match spec.unit.parse_value(&point.value) {
                Ok(value) if (spec.minimum..=spec.maximum).contains(&value) => {}
                Ok(_) => source_site_error(
                    diagnostics,
                    "CRV115",
                    format!(
                        "Automation value must be within {} … {}",
                        spec.unit.format_value(spec.minimum),
                        spec.unit.format_value(spec.maximum)
                    ),
                    &point.value_site,
                ),
                Err(error) => source_site_error(
                    diagnostics,
                    "CRV116",
                    format!("invalid Automation value for {}: {error}", spec.unit),
                    &point.value_site,
                ),
            }
            if spec.unit == Unit::Boolean && point.interpolation != AutomationInterpolation::Step {
                source_site_error(
                    diagnostics,
                    "CRV132",
                    "boolean Automation is step-only",
                    &point.interpolation_site,
                );
            }
            if let AutomationInterpolation::Power(exponent) = point.interpolation
                && (!exponent.is_finite() || exponent <= 0.0)
            {
                source_site_error(
                    diagnostics,
                    "CRV117",
                    "Automation power exponent must be finite and greater than zero",
                    &point.interpolation_site,
                );
            }
        }
    }
}

pub(super) fn validate_modulations(project: &Project, diagnostics: &mut Diagnostics) {
    let instrument_route_count = project
        .song
        .instruments
        .iter()
        .map(|instrument| instrument.modulation_routes.len())
        .sum::<usize>();
    let route_count = project
        .song
        .modulations
        .len()
        .saturating_add(project.song.envelope_followers.len())
        .saturating_add(instrument_route_count);
    if route_count > MAX_MODULATION_ROUTES {
        diagnostics.push(Diagnostic::error(
            "CRV119",
            format!(
                "Song declares {route_count} Modulation and Envelope Follower routes; maximum is {MAX_MODULATION_ROUTES}"
            ),
        ));
    }
    let total_operators = project
        .song
        .modulations
        .iter()
        .map(|route| route.operators.len())
        .chain(
            project
                .song
                .envelope_followers
                .iter()
                .map(|route| route.operators.len()),
        )
        .chain(
            project
                .song
                .instruments
                .iter()
                .flat_map(|instrument| &instrument.modulation_routes)
                .map(|route| route.operators.len()),
        )
        .sum::<usize>();
    if total_operators > MAX_MODULATION_OPERATORS {
        diagnostics.push(Diagnostic::error(
            "CRV120",
            format!(
                "Song declares {total_operators} Modulation operators; maximum is {MAX_MODULATION_OPERATORS}"
            ),
        ));
    }
    let locked_targets = crate::engine::schedule::compile_track_parameter_lock_lanes(project)
        .map(|lanes| {
            lanes
                .into_keys()
                .map(|(_, target)| target)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for instrument in &project.song.instruments {
        validate_instrument_modulation(project, instrument, &locked_targets, diagnostics);
    }

    let instruments = project
        .song
        .instruments
        .iter()
        .map(|instrument| (instrument.id.as_str(), instrument))
        .collect::<BTreeMap<_, _>>();
    let mut route_ids = BTreeSet::new();
    for route in &project.song.modulations {
        if !valid_id(&route.id) || !route_ids.insert(route.id.as_str()) {
            located_error(
                diagnostics,
                "CRV121",
                format!(
                    "Modulation route id `{}` is invalid or duplicated",
                    route.id
                ),
                &route.location,
            );
        }
        let Some(spec) = project.parameter_spec(&route.target) else {
            located_error(
                diagnostics,
                "CRV122",
                format!("Modulation targets missing Parameter `{}`", route.target),
                &route.location,
            );
            continue;
        };
        if !spec.modulatable {
            located_error(
                diagnostics,
                "CRV123",
                format!("Parameter `{}` is not declared modulatable", route.target),
                &route.location,
            );
        }
        if !automation_target_is_runtime_safe(project, &route.target) {
            located_error(
                diagnostics,
                "CRV124",
                format!("Parameter `{}` cannot be modulated safely", route.target),
                &route.location,
            );
        }
        match &route.scope {
            ModulationScope::Global => {}
            ModulationScope::Instrument(owner) | ModulationScope::PerVoice(owner) => {
                let Some(instrument) = instruments.get(owner.as_str()) else {
                    located_error(
                        diagnostics,
                        "CRV125",
                        format!("Modulation scope references missing Instrument `{owner}`"),
                        &route.location,
                    );
                    continue;
                };
                if !matches!(
                    &route.target,
                    ParameterTarget::Instrument { instrument: target, .. } if target == owner
                ) {
                    located_error(
                        diagnostics,
                        "CRV126",
                        "Instrument and per-voice Modulation may target only their owning Instrument",
                        &route.location,
                    );
                }
                if matches!(route.scope, ModulationScope::PerVoice(_))
                    && instrument.kind != InstrumentKind::Graph
                {
                    located_error(
                        diagnostics,
                        "CRV127",
                        "per-voice Modulation requires a graph Instrument",
                        &route.location,
                    );
                }
            }
        }
        match route.rate {
            ModulationRate::FreeHertz(rate) if rate.is_finite() && rate > 0.0 && rate <= 50.0 => {}
            ModulationRate::Synchronized(period) if period > crate::time::MusicalTime::ZERO => {}
            _ => located_error(
                diagnostics,
                "CRV128",
                "Modulation LFO rate must be positive, finite, and no greater than 50hz",
                &route.location,
            ),
        }
        match spec.unit.parse_value(&route.depth) {
            Ok(depth) if depth.is_finite() => {}
            Ok(_) => unreachable!("Unit::parse_value rejects non-finite values"),
            Err(error) => located_error(
                diagnostics,
                "CRV129",
                format!("invalid Modulation depth for {}: {error}", spec.unit),
                &route.location,
            ),
        }
        if route.operators.len() > MAX_MODULATION_OPERATORS_PER_ROUTE {
            located_error(
                diagnostics,
                "CRV130",
                format!(
                    "Modulation route has {} operators; maximum is {MAX_MODULATION_OPERATORS_PER_ROUTE}",
                    route.operators.len()
                ),
                &route.location,
            );
        }
        for operator in &route.operators {
            let valid = match *operator {
                ModulationOperator::Add { value }
                | ModulationOperator::Multiply { value }
                | ModulationOperator::Scale { value }
                | ModulationOperator::Bias { value } => value.is_finite(),
                ModulationOperator::Clamp { minimum, maximum } => {
                    minimum.is_finite() && maximum.is_finite() && minimum < maximum
                }
            };
            if !valid {
                located_error(
                    diagnostics,
                    "CRV131",
                    "Modulation operators require finite scalars and clamp minimum < maximum",
                    &route.location,
                );
            }
        }
    }

    for route in &project.song.envelope_followers {
        if !valid_id(&route.id) || !route_ids.insert(route.id.as_str()) {
            located_error(
                diagnostics,
                "CRV148",
                format!(
                    "Envelope Follower route id `{}` is invalid or duplicated",
                    route.id
                ),
                &route.location,
            );
        }
        let source_exists = match &route.source {
            ChannelRef::Track(id) => project.song.tracks.iter().any(|channel| channel.id == *id),
            ChannelRef::Group(id) => project.song.groups.iter().any(|channel| channel.id == *id),
            ChannelRef::Return(id) => project.song.returns.iter().any(|channel| channel.id == *id),
        };
        if !source_exists {
            located_error(
                diagnostics,
                "CRV149",
                format!("Envelope Follower source `{}` does not exist", route.source),
                &route.location,
            );
        }
        let Some(spec) = project.parameter_spec(&route.target) else {
            located_error(
                diagnostics,
                "CRV150",
                format!(
                    "Envelope Follower targets missing Parameter `{}`",
                    route.target
                ),
                &route.location,
            );
            continue;
        };
        if !spec.modulatable || !automation_target_is_runtime_safe(project, &route.target) {
            located_error(
                diagnostics,
                "CRV151",
                format!(
                    "Parameter `{}` cannot receive Envelope Follower modulation safely",
                    route.target
                ),
                &route.location,
            );
        }
        if !route.attack_seconds.is_finite()
            || !route.release_seconds.is_finite()
            || !(0.000_01..=10.0).contains(&route.attack_seconds)
            || !(0.000_01..=10.0).contains(&route.release_seconds)
        {
            located_error(
                diagnostics,
                "CRV152",
                "Envelope Follower attack/release must be 0.00001s..=10s",
                &route.location,
            );
        }
        if !route.floor_db.is_finite()
            || !route.ceiling_db.is_finite()
            || !(-144.0..=24.0).contains(&route.floor_db)
            || !(-144.0..=24.0).contains(&route.ceiling_db)
            || route.floor_db >= route.ceiling_db
        {
            located_error(
                diagnostics,
                "CRV153",
                "Envelope Follower requires finite -144db..=24db floor < ceiling",
                &route.location,
            );
        }
        if let Err(error) = spec.unit.parse_value(&route.depth) {
            located_error(
                diagnostics,
                "CRV154",
                format!("invalid Envelope Follower depth for {}: {error}", spec.unit),
                &route.location,
            );
        }
        if route.operators.len() > MAX_MODULATION_OPERATORS_PER_ROUTE {
            located_error(
                diagnostics,
                "CRV155",
                format!(
                    "Envelope Follower route has {} operators; maximum is {MAX_MODULATION_OPERATORS_PER_ROUTE}",
                    route.operators.len()
                ),
                &route.location,
            );
        }
        for operator in &route.operators {
            let valid = match *operator {
                ModulationOperator::Add { value }
                | ModulationOperator::Multiply { value }
                | ModulationOperator::Scale { value }
                | ModulationOperator::Bias { value } => value.is_finite(),
                ModulationOperator::Clamp { minimum, maximum } => {
                    minimum.is_finite() && maximum.is_finite() && minimum < maximum
                }
            };
            if !valid {
                located_error(
                    diagnostics,
                    "CRV156",
                    "Envelope Follower operators require finite scalars and clamp minimum < maximum",
                    &route.location,
                );
            }
        }
    }
}

fn validate_instrument_modulation(
    project: &Project,
    instrument: &crate::model::Instrument,
    locked_targets: &BTreeSet<ParameterTarget>,
    diagnostics: &mut Diagnostics,
) {
    if instrument.kind != InstrumentKind::Graph
        && (!instrument.modulation_sources.is_empty() || !instrument.modulation_routes.is_empty())
    {
        located_error(
            diagnostics,
            "CRV169",
            "Instrument-owned Modulation is supported only by graph Instruments",
            &instrument.location,
        );
    }
    let macro_count = instrument
        .parameters
        .iter()
        .filter(|parameter| parameter.macro_source)
        .count();
    if macro_count > MAX_INSTRUMENT_MACROS {
        located_error(
            diagnostics,
            "CRV170",
            format!(
                "Instrument declares {macro_count} macro Parameters; maximum is {MAX_INSTRUMENT_MACROS}"
            ),
            &instrument.location,
        );
    }
    let lfo_count = instrument
        .modulation_sources
        .iter()
        .filter(|source| matches!(source.kind, InstrumentModulationSourceKind::Lfo { .. }))
        .count();
    if lfo_count > MAX_INSTRUMENT_LFOS {
        located_error(
            diagnostics,
            "CRV170",
            format!(
                "Instrument declares {lfo_count} reusable LFO sources; maximum is {MAX_INSTRUMENT_LFOS}"
            ),
            &instrument.location,
        );
    }
    if instrument.modulation_sources.len() > MAX_INSTRUMENT_MODULATION_SOURCES {
        located_error(
            diagnostics,
            "CRV170",
            format!(
                "Instrument declares {} Modulation sources; maximum is {MAX_INSTRUMENT_MODULATION_SOURCES}",
                instrument.modulation_sources.len()
            ),
            &instrument.location,
        );
    }
    let mut source_ids = BTreeSet::new();
    let mut sources = BTreeMap::new();
    for source in &instrument.modulation_sources {
        if !valid_id(&source.id) || !source_ids.insert(source.id.as_str()) {
            located_error(
                diagnostics,
                "CRV171",
                format!(
                    "Instrument Modulation source id `{}` is invalid or duplicated",
                    source.id
                ),
                &source.location,
            );
        }
        sources.insert(source.id.as_str(), source);
        let scope_valid = match source.kind {
            InstrumentModulationSourceKind::GraphPort { .. }
            | InstrumentModulationSourceKind::NoteVelocity
            | InstrumentModulationSourceKind::NoteKey { .. }
            | InstrumentModulationSourceKind::NoteGate
            | InstrumentModulationSourceKind::NoteAge { .. } => {
                source.scope == InstrumentModulationScope::PerVoice
            }
            _ => true,
        };
        if !scope_valid {
            located_error(
                diagnostics,
                "CRV172",
                "graph-port and note-relative Modulation sources must use `per_voice` scope",
                &source.location,
            );
        }
        match &source.kind {
            InstrumentModulationSourceKind::GraphPort { node, port } => {
                let graph_node = instrument
                    .graph
                    .as_ref()
                    .and_then(|graph| graph.nodes.iter().find(|candidate| candidate.id == *node));
                if !graph_node.is_some_and(|node| {
                    matches!(
                        node.kind,
                        NodeKind::Envelope | NodeKind::Lfo | NodeKind::ToControl
                    ) && port == "control"
                }) {
                    located_error(
                        diagnostics,
                        "CRV173",
                        format!(
                            "Modulation source references missing or non-Control graph Port `{node}.{port}`"
                        ),
                        &source.location,
                    );
                } else if let Some(graph_node) = graph_node {
                    let native_polarity = match graph_node.kind {
                        NodeKind::Envelope => ControlPolarity::Unipolar,
                        NodeKind::Lfo | NodeKind::ToControl => ControlPolarity::Bipolar,
                        _ => unreachable!("Port-kind guard above limits graph Control sources"),
                    };
                    if source.polarity != native_polarity {
                        located_error(
                            diagnostics,
                            "CRV191",
                            format!(
                                "Modulation source `{}` must declare the native {} polarity of `{node}.{port}`",
                                source.id,
                                match native_polarity {
                                    ControlPolarity::Unipolar => "unipolar",
                                    ControlPolarity::Bipolar => "bipolar",
                                }
                            ),
                            &source.location,
                        );
                    }
                }
            }
            InstrumentModulationSourceKind::Parameter { parameter } => {
                let candidate = instrument
                    .parameters
                    .iter()
                    .find(|candidate| candidate.key == *parameter);
                if !candidate.is_some_and(|candidate| candidate.macro_source) {
                    located_error(
                        diagnostics,
                        "CRV174",
                        format!(
                            "Modulation source Parameter `{parameter}` is missing or not declared `macro`"
                        ),
                        &source.location,
                    );
                } else if !candidate.is_some_and(|candidate| {
                    candidate.unit == Unit::Ratio
                        && candidate.minimum == 0.0
                        && candidate.maximum == 1.0
                }) {
                    located_error(
                        diagnostics,
                        "CRV174",
                        format!(
                            "macro Parameter `{parameter}` must use the normalized 0..=1 ratio domain"
                        ),
                        &source.location,
                    );
                }
            }
            InstrumentModulationSourceKind::Lfo {
                rate,
                phase,
                retrigger,
                ..
            } => {
                if !rate_is_valid(rate, project)
                    || !phase.is_finite()
                    || !(0.0..=1.0).contains(phase)
                {
                    located_error(
                        diagnostics,
                        "CRV175",
                        "Instrument LFO rate/phase is outside its finite domain",
                        &source.location,
                    );
                }
                let expected = if source.scope == InstrumentModulationScope::PerVoice {
                    LfoRetrigger::Note
                } else {
                    LfoRetrigger::Transport
                };
                if *retrigger != expected {
                    located_error(
                        diagnostics,
                        "CRV175",
                        "per_voice LFOs require retrigger note; global/instrument LFOs require retrigger transport",
                        &source.location,
                    );
                }
            }
            InstrumentModulationSourceKind::RandomHold { rate, .. } => {
                if !rate_is_valid(rate, project) {
                    located_error(
                        diagnostics,
                        "CRV175",
                        "random-hold rate is outside its finite domain",
                        &source.location,
                    );
                }
            }
            InstrumentModulationSourceKind::NoteKey {
                range_semitones, ..
            } => {
                if !range_semitones.is_finite() || *range_semitones <= 0.0 {
                    located_error(
                        diagnostics,
                        "CRV175",
                        "note-key range must be finite and positive",
                        &source.location,
                    );
                }
            }
            InstrumentModulationSourceKind::NoteAge { duration_seconds } => {
                if !duration_seconds.is_finite() || *duration_seconds <= 0.0 {
                    located_error(
                        diagnostics,
                        "CRV175",
                        "note-age duration must be finite and positive",
                        &source.location,
                    );
                }
            }
            InstrumentModulationSourceKind::NoteVelocity
            | InstrumentModulationSourceKind::NoteGate => {}
        }
    }

    if instrument.modulation_routes.len() > MAX_MODULATION_ROUTES {
        located_error(
            diagnostics,
            "CRV119",
            format!(
                "Instrument declares {} Modulation routes; maximum is {MAX_MODULATION_ROUTES}",
                instrument.modulation_routes.len()
            ),
            &instrument.location,
        );
    }
    let mut route_ids = BTreeSet::new();
    let mut macro_edges = BTreeMap::<String, Vec<String>>::new();
    let mut parameter_producer_scopes =
        BTreeMap::<String, Vec<(InstrumentModulationScope, String)>>::new();
    let graph_adjacency = instrument.graph.as_ref().map(|graph| {
        let mut adjacency = BTreeMap::<String, Vec<String>>::new();
        let latency_nodes = graph
            .nodes
            .iter()
            .filter(|node| node.kind.breaks_feedback())
            .map(|node| node.id.as_str())
            .collect::<BTreeSet<_>>();
        for connection in &graph.connections {
            if connection.from.node != "note"
                && !latency_nodes.contains(connection.to.node.as_str())
            {
                adjacency
                    .entry(connection.from.node.clone())
                    .or_default()
                    .push(connection.to.node.clone());
            }
        }
        adjacency
    });
    for route in &instrument.modulation_routes {
        if !valid_id(&route.id) || !route_ids.insert(route.id.as_str()) {
            located_error(
                diagnostics,
                "CRV176",
                format!(
                    "Instrument route id `{}` is invalid or duplicated",
                    route.id
                ),
                &route.location,
            );
        }
        if !valid_key(&route.target) {
            located_error(
                diagnostics,
                "CRV177",
                format!("Instrument route target key `{}` is invalid", route.target),
                &route.location,
            );
        }
        let Some(source) = sources.get(route.source.as_str()) else {
            located_error(
                diagnostics,
                "CRV178",
                format!(
                    "Instrument route references missing source `{}`",
                    route.source
                ),
                &route.location,
            );
            continue;
        };
        let target = instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == route.target);
        let Some(target) = target else {
            located_error(
                diagnostics,
                "CRV179",
                format!(
                    "Instrument route targets missing Parameter `{}`",
                    route.target
                ),
                &route.location,
            );
            continue;
        };
        parameter_producer_scopes
            .entry(route.target.clone())
            .or_default()
            .push((source.scope, route.id.clone()));
        if !target.modulatable
            || !automation_target_is_runtime_safe(
                project,
                &ParameterTarget::Instrument {
                    instrument: instrument.id.clone(),
                    parameter: target.key.clone(),
                },
            )
        {
            located_error(
                diagnostics,
                "CRV180",
                format!("Parameter `{}` cannot be modulated safely", target.key),
                &route.location,
            );
        }
        if let Err(error) = target.unit.parse_value(&route.depth) {
            source_site_error(
                diagnostics,
                "CRV181",
                format!("invalid Instrument route depth: {error}"),
                &route.sites.depth,
            );
        }
        if let ModulationCurve::Power(exponent) = route.curve
            && (!exponent.is_finite() || exponent <= 0.0)
        {
            located_error(
                diagnostics,
                "CRV182",
                "Modulation power exponent must be finite and positive",
                &route.location,
            );
        }
        if route.operators.len() > MAX_MODULATION_OPERATORS_PER_ROUTE
            || !route.operators.iter().all(valid_operator)
        {
            located_error(
                diagnostics,
                "CRV183",
                "Instrument route operators exceed limits or contain invalid finite domains",
                &route.location,
            );
        }
        if let Some(auxiliary) = &route.auxiliary {
            let Some(aux) = sources.get(auxiliary.as_str()) else {
                located_error(
                    diagnostics,
                    "CRV184",
                    format!("Instrument route references missing auxiliary source `{auxiliary}`"),
                    &route.location,
                );
                continue;
            };
            if scope_rank(aux.scope) > scope_rank(source.scope) {
                located_error(
                    diagnostics,
                    "CRV185",
                    "auxiliary source cannot have narrower runtime scope than the primary source",
                    &route.location,
                );
            }
        }
        for consumed in std::iter::once(*source).chain(
            route
                .auxiliary
                .as_ref()
                .and_then(|id| sources.get(id.as_str()))
                .copied(),
        ) {
            if consumed.scope == InstrumentModulationScope::Global
                && let InstrumentModulationSourceKind::Parameter { parameter } = &consumed.kind
                && locked_targets.contains(&ParameterTarget::Instrument {
                    instrument: instrument.id.clone(),
                    parameter: parameter.clone(),
                })
            {
                located_error(
                    diagnostics,
                    "CRV192",
                    format!(
                        "Song-global Parameter source `{}` cannot read Track-owned lock `{parameter}`",
                        consumed.id
                    ),
                    &route.location,
                );
            }
        }
        if let InstrumentModulationSourceKind::Parameter { parameter } = &source.kind {
            macro_edges
                .entry(parameter.clone())
                .or_default()
                .push(route.target.clone());
        }
        if let Some(auxiliary) = route
            .auxiliary
            .as_ref()
            .and_then(|id| sources.get(id.as_str()))
            && let InstrumentModulationSourceKind::Parameter { parameter } = &auxiliary.kind
        {
            macro_edges
                .entry(parameter.clone())
                .or_default()
                .push(route.target.clone());
        }
        let graph_source_nodes = std::iter::once(source)
            .chain(
                route
                    .auxiliary
                    .as_ref()
                    .and_then(|id| sources.get(id.as_str())),
            )
            .filter_map(|source| match &source.kind {
                InstrumentModulationSourceKind::GraphPort { node, .. } => Some(node),
                _ => None,
            });
        if let (Some(graph), Some(adjacency)) = (&instrument.graph, &graph_adjacency) {
            for node in graph_source_nodes {
                for consumer in graph.nodes.iter().filter(|candidate| {
                    !candidate.kind.breaks_feedback()
                        && candidate.references_parameter(&route.target)
                }) {
                    if reaches(adjacency, &consumer.id, node) || consumer.id == *node {
                        located_error(
                            diagnostics,
                            "CRV186",
                            format!(
                                "graph-port source on route `{}` creates a Control dependency cycle",
                                route.id
                            ),
                            &route.location,
                        );
                    }
                }
            }
        }
    }
    for route in &instrument.modulation_routes {
        let Some(primary) = sources.get(route.source.as_str()) else {
            continue;
        };
        for consumer in std::iter::once(*primary).chain(
            route
                .auxiliary
                .as_ref()
                .and_then(|id| sources.get(id.as_str()))
                .copied(),
        ) {
            let InstrumentModulationSourceKind::Parameter { parameter } = &consumer.kind else {
                continue;
            };
            if parameter_producer_scopes
                .get(parameter)
                .is_some_and(|producers| {
                    producers
                        .iter()
                        .any(|(scope, _)| scope_rank(*scope) > scope_rank(consumer.scope))
                })
            {
                located_error(
                    diagnostics,
                    "CRV188",
                    format!(
                        "route `{}` reads macro `{parameter}` from a broader scope than one of its producers",
                        route.id
                    ),
                    &route.location,
                );
            }
        }
    }
    for source in macro_edges.keys() {
        if reaches(&macro_edges, source, source) {
            located_error(
                diagnostics,
                "CRV187",
                "macro/route dependency graph contains a cycle",
                &instrument.location,
            );
            break;
        }
    }
}

fn scope_rank(scope: InstrumentModulationScope) -> u8 {
    match scope {
        InstrumentModulationScope::Global => 0,
        InstrumentModulationScope::Instrument => 1,
        InstrumentModulationScope::PerVoice => 2,
    }
}

fn rate_is_valid(rate: &crate::model::InstrumentModulationRate, project: &Project) -> bool {
    match rate {
        crate::model::InstrumentModulationRate::FreeHertz(rate) => {
            rate.is_finite() && *rate > 0.0 && *rate <= 50.0
        }
        crate::model::InstrumentModulationRate::Synchronized(literal) => {
            crate::time::parse_musical_time(literal, project.song.meter)
                .is_ok_and(|period| period > crate::time::MusicalTime::ZERO)
        }
    }
}

fn valid_operator(operator: &ModulationOperator) -> bool {
    match *operator {
        ModulationOperator::Add { value }
        | ModulationOperator::Multiply { value }
        | ModulationOperator::Scale { value }
        | ModulationOperator::Bias { value } => value.is_finite(),
        ModulationOperator::Clamp { minimum, maximum } => {
            minimum.is_finite() && maximum.is_finite() && minimum < maximum
        }
    }
}

fn reaches(adjacency: &BTreeMap<String, Vec<String>>, start: &str, target: &str) -> bool {
    let mut pending = adjacency.get(start).cloned().unwrap_or_default();
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if seen.insert(node.clone())
            && let Some(next) = adjacency.get(&node)
        {
            pending.extend(next.iter().cloned());
        }
    }
    false
}

pub(super) fn automation_target_is_runtime_safe(
    project: &Project,
    target: &ParameterTarget,
) -> bool {
    let ParameterTarget::Instrument {
        instrument,
        parameter,
    } = target
    else {
        return true;
    };
    let Some(graph) = project
        .song
        .instruments
        .iter()
        .find(|candidate| candidate.id == *instrument)
        .and_then(|instrument| instrument.graph.as_ref())
    else {
        return true;
    };
    !graph.nodes.iter().any(|node| {
        node.kind == NodeKind::Delay
            && matches!(
                node.setting("time"),
                Some(SettingValue::Parameter(bound)) if bound == parameter
            )
    })
}
