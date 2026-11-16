use super::*;

/// Builds a checked insertion for a new Song Automation lane and required zero point.
pub fn create_automation_lane(
    bundle: &ProjectBundle,
    lane_id: &str,
    target: &ParameterTarget,
    initial_value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if project
        .song
        .automations
        .iter()
        .any(|lane| lane.id == lane_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Automation lane `{lane_id}` already exists"
        )));
    }
    if project
        .song
        .automations
        .iter()
        .any(|lane| lane.target == *target)
    {
        return Err(EditError::InvalidValue(format!(
            "Parameter `{target}` already has absolute Automation"
        )));
    }
    let spec = project
        .parameter_spec(target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{target}`")))?;
    if !spec.automatable {
        return Err(EditError::WrongTargetKind(format!(
            "Parameter `{target}` is not automatable"
        )));
    }
    let value = canonical_automation_value(target, spec, initial_value)?;
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    let point_indentation = format!("{indentation}  ");
    let replacement = format!(
        "{indentation}automation {lane_id} target {target} {{\n{point_indentation}point start 0qn {value} step\n{indentation}}}\n"
    );
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement,
    })
}

/// Builds a checked insertion for one durable Automation point.
pub fn create_automation_point(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    at: &str,
    value: &str,
    interpolation: AutomationInterpolation,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let lane = automation_lane(bundle, lane_id)?;
    if lane.points.iter().any(|point| point.id == point_id) {
        return Err(EditError::InvalidValue(format!(
            "Automation point `{point_id}` already exists in lane `{lane_id}`"
        )));
    }
    let at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_automation_time(project, lane, None, at)?;
    let spec = project
        .parameter_spec(&lane.target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{}`", lane.target)))?;
    let value = canonical_automation_value(&lane.target, spec, value)?;
    let (file, offset, indentation) =
        if let Some(next) = lane.points.iter().find(|point| point.at > at) {
            line_insertion(bundle, &next.location, "Automation point")?
        } else {
            block_insertion(bundle, &lane.location, "Automation")?
        };
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{indentation}point {point_id} {at} {value} {interpolation}\n"),
    })
}

/// Builds a checked physical-value replacement for one Automation point.
pub fn set_automation_point_value(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (lane, point) = automation_point(bundle, lane_id, point_id)?;
    let spec = project
        .parameter_spec(&lane.target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{}`", lane.target)))?;
    Ok(CheckedReplacement::at(
        &point.value_site,
        canonical_automation_value(&lane.target, spec, value)?,
    ))
}

/// Builds a checked exact-time replacement for one Automation point.
pub fn set_automation_point_at(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (lane, point) = automation_point(bundle, lane_id, point_id)?;
    let at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_automation_time(project, lane, Some(point_id), at)?;
    let current_index = lane
        .points
        .iter()
        .position(|candidate| candidate.id == point_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Automation point `{point_id}`")))?;
    if current_index == 0 && at != crate::time::MusicalTime::ZERO {
        return Err(EditError::InvalidValue(
            "Automation lane's first point must remain at Song time zero".to_owned(),
        ));
    }
    let remains_ordered = current_index
        .checked_sub(1)
        .and_then(|index| lane.points.get(index))
        .is_none_or(|previous| previous.at < at)
        && lane
            .points
            .get(current_index + 1)
            .is_none_or(|next| at < next.at);
    if remains_ordered {
        return Ok(CheckedReplacement::at(&point.at_site, at.to_string()));
    }

    let source = bundle
        .sources
        .get(&lane.location.file)
        .ok_or_else(|| EditError::UnknownSource(lane.location.file.clone()))?;
    let (moved_start, moved_end) = line_bounds(source, &point.location)?;
    let target_offset = if let Some(next) = lane
        .points
        .iter()
        .filter(|candidate| candidate.id != point_id)
        .find(|candidate| candidate.at > at)
    {
        line_bounds(source, &next.location)?.0
    } else {
        block_insertion(bundle, &lane.location, "Automation")?.1
    };
    if moved_start < lane.location.start
        || moved_end > lane.location.end
        || target_offset < lane.location.start
        || target_offset > lane.location.end
    {
        return Err(EditError::OverlappingOrInvalidRange(
            lane.location.file.clone(),
        ));
    }
    let mut moved_line = source[moved_start..moved_end].to_owned();
    moved_line.replace_range(
        point.at_site.start - moved_start..point.at_site.end - moved_start,
        &at.to_string(),
    );
    let moved_start = moved_start - lane.location.start;
    let moved_end = moved_end - lane.location.start;
    let mut insertion = target_offset - lane.location.start;
    let mut replacement = source[lane.location.start..lane.location.end].to_owned();
    replacement.replace_range(moved_start..moved_end, "");
    if insertion > moved_start {
        insertion -= moved_end - moved_start;
    }
    replacement.insert_str(insertion, &moved_line);
    Ok(CheckedReplacement {
        file: lane.location.file.clone(),
        start: lane.location.start,
        end: lane.location.end,
        expected: source[lane.location.start..lane.location.end].to_owned(),
        replacement,
    })
}

/// Builds a checked interpolation replacement for one Automation point.
pub fn set_automation_point_interpolation(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    interpolation: AutomationInterpolation,
) -> Result<CheckedReplacement, EditError> {
    let (_, point) = automation_point(bundle, lane_id, point_id)?;
    Ok(CheckedReplacement::at(
        &point.interpolation_site,
        interpolation.to_string(),
    ))
}

/// Builds a checked whole-directive deletion for one Automation point.
pub fn delete_automation_point(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let (_, point) = automation_point(bundle, lane_id, point_id)?;
    line_replacement(bundle, &point.location)
}

/// Builds a checked whole-block deletion for one Automation lane.
pub fn delete_automation_lane(
    bundle: &ProjectBundle,
    lane_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let lane = automation_lane(bundle, lane_id)?;
    line_replacement(bundle, &lane.location)
}

fn canonical_automation_value(
    target: &ParameterTarget,
    spec: crate::model::ParameterSpec,
    value: &str,
) -> Result<String, EditError> {
    let parsed = spec
        .unit
        .parse_value(value)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if !parsed.is_finite() || !(spec.minimum..=spec.maximum).contains(&parsed) {
        return Err(EditError::OutOfRange {
            target: target.to_string(),
            minimum: spec.minimum,
            maximum: spec.maximum,
            unit: spec.unit,
        });
    }
    Ok(spec.unit.format_value(parsed))
}

fn automation_lane<'a>(
    bundle: &'a ProjectBundle,
    lane_id: &str,
) -> Result<&'a AutomationLane, EditError> {
    bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .automations
        .iter()
        .find(|lane| lane.id == lane_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Automation lane `{lane_id}`")))
}

fn automation_point<'a>(
    bundle: &'a ProjectBundle,
    lane_id: &str,
    point_id: &str,
) -> Result<(&'a AutomationLane, &'a AutomationPoint), EditError> {
    let lane = automation_lane(bundle, lane_id)?;
    let point = lane
        .points
        .iter()
        .find(|point| point.id == point_id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!("Automation point `{point_id}` in lane `{lane_id}`"))
        })?;
    Ok((lane, point))
}

fn validate_automation_time(
    project: &crate::model::Project,
    lane: &AutomationLane,
    except_point: Option<&str>,
    at: crate::time::MusicalTime,
) -> Result<u64, EditError> {
    if at < crate::time::MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Automation point must lie between Song time zero and the arrangement end".to_owned(),
        ));
    }
    let frame = at
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    for point in lane
        .points
        .iter()
        .filter(|point| Some(point.id.as_str()) != except_point)
    {
        if point.at == at {
            return Err(EditError::InvalidValue(format!(
                "Automation lane `{}` already has a point at {at}",
                lane.id
            )));
        }
        let point_frame = point
            .at
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .map_err(|error| EditError::InvalidValue(error.to_string()))?;
        if point_frame == frame {
            return Err(EditError::InvalidValue(format!(
                "Automation point at {at} would share sample frame {frame} in lane `{}`",
                lane.id
            )));
        }
    }
    Ok(frame)
}
