//! Song Automation and Modulation control validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{
        AutomationInterpolation, ChannelRef, InstrumentKind, ModulationOperator, ModulationRate,
        ModulationScope, NodeKind, ParameterTarget, Project, SettingValue, Unit,
    },
};

use super::{
    MAX_AUTOMATION_LANES, MAX_AUTOMATION_POINTS, MAX_AUTOMATION_POINTS_PER_LANE,
    MAX_CONTROL_TARGETS, MAX_MODULATION_OPERATORS, MAX_MODULATION_OPERATORS_PER_ROUTE,
    MAX_MODULATION_ROUTES, located_error, source_site_error, valid_id,
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
    let route_count = project
        .song
        .modulations
        .len()
        .saturating_add(project.song.envelope_followers.len());
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
        .sum::<usize>();
    if total_operators > MAX_MODULATION_OPERATORS {
        diagnostics.push(Diagnostic::error(
            "CRV120",
            format!(
                "Song declares {total_operators} Modulation operators; maximum is {MAX_MODULATION_OPERATORS}"
            ),
        ));
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
                node.settings.get("time"),
                Some(SettingValue::Parameter(bound)) if bound == parameter
            )
    })
}
