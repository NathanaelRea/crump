use super::*;

/// Policy for Song-scoped Automation during arrangement-time transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangementAutomation {
    /// Move Automation points with the transformed Timeline material.
    FollowArrangement,
    /// Leave absolute Automation times fixed; invalid post-edit ranges are rejected.
    LockToSong,
}

/// Inserts empty arrangement time and atomically shifts later Clips, Locators, and Automation.
///
/// A Clip spanning the insertion boundary must be split first so this operation never invents
/// hidden source traversal. The required Automation point at Song zero remains pinned at zero.
pub fn insert_arrangement_time(
    bundle: &ProjectBundle,
    at: &str,
    duration: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let at = parse_clip_time(project, at, "arrangement insertion time")?;
    let duration = parse_clip_time(project, duration, "arrangement insertion duration")?;
    plan_arrangement_insertion(bundle, at, duration, automation)
}

/// Deletes a half-open arrangement-time range and closes the resulting gap atomically.
///
/// Fully covered Clips and Automation points are deleted; later objects shift left. Clips that
/// cross either deletion boundary must be split first, making the crop policy explicit in source.
pub fn delete_arrangement_time(
    bundle: &ProjectBundle,
    at: &str,
    duration: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let at = parse_clip_time(project, at, "arrangement deletion time")?;
    let duration = parse_clip_time(project, duration, "arrangement deletion duration")?;
    if at < MusicalTime::ZERO || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement deletion requires non-negative time and positive duration".to_owned(),
        ));
    }
    let end = at
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if end > project.song.end || duration >= project.song.end {
        return Err(EditError::InvalidValue(
            "arrangement deletion must leave a positive Song and remain within its end".to_owned(),
        ));
    }
    let new_song_end = project
        .song
        .end
        .checked_sub(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let mut replacements = vec![CheckedReplacement::at(
        &project.song.end_site,
        new_song_end.to_string(),
    )];

    for track in &project.song.tracks {
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            let crosses_left = clip.start < at && clip_end > at;
            let crosses_right = clip.start < end && clip_end > end;
            if crosses_left || crosses_right {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses a deletion boundary; split it at {} and/or {} first",
                    clip.id, at, end
                )));
            }
            if clip.start >= at && clip_end <= end {
                replacements.push(line_replacement(bundle, &clip.location)?);
            } else if clip.start >= end {
                let shifted = clip
                    .start
                    .checked_sub(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(
                    &clip.sites.start,
                    shifted.to_string(),
                ));
            }
        }
    }
    for locator in &project.song.locators {
        let shifted = if locator.at < at {
            None
        } else if locator.at < end {
            Some(at)
        } else {
            Some(
                locator
                    .at
                    .checked_sub(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?,
            )
        };
        if let Some(shifted) = shifted
            && shifted != locator.at
        {
            replacements.push(CheckedReplacement::at(
                &locator.at_site,
                shifted.to_string(),
            ));
        }
    }
    match automation {
        ArrangementAutomation::FollowArrangement => {
            if at == MusicalTime::ZERO
                && project
                    .song
                    .automations
                    .iter()
                    .any(|lane| !lane.points.iter().any(|point| point.at == end))
            {
                return Err(EditError::InvalidValue(
                    "deleting from Song zero requires every Automation lane to have a point at the deletion end"
                        .to_owned(),
                ));
            }
            for lane in &project.song.automations {
                for point in &lane.points {
                    if point.at >= at && point.at < end {
                        replacements.push(line_replacement(bundle, &point.location)?);
                    } else if point.at >= end {
                        let shifted = point
                            .at
                            .checked_sub(duration)
                            .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                        replacements
                            .push(CheckedReplacement::at(&point.at_site, shifted.to_string()));
                    }
                }
            }
        }
        ArrangementAutomation::LockToSong => {
            if project
                .song
                .automations
                .iter()
                .flat_map(|lane| &lane.points)
                .any(|point| point.at > new_song_end)
            {
                return Err(EditError::InvalidValue(
                    "locked Automation would extend beyond the shortened Song".to_owned(),
                ));
            }
        }
    }
    Ok(replacements)
}

/// Duplicates all complete Clips in a source range into newly inserted arrangement time.
///
/// Copies receive `ORIGINAL-SUFFIX` identities. Clips crossing the source or insertion boundaries
/// must be split first. Locators remain unique navigation objects; Automation follows the insertion
/// policy but is not copied, so section Automation can be authored explicitly after duplication.
pub fn duplicate_arrangement_time(
    bundle: &ProjectBundle,
    source_at: &str,
    duration: &str,
    destination: &str,
    id_suffix: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(id_suffix) {
        return Err(EditError::InvalidValue(
            "arrangement duplicate suffix must be source-safe".to_owned(),
        ));
    }
    let source_at = parse_clip_time(project, source_at, "duplicate source time")?;
    let duration = parse_clip_time(project, duration, "duplicate duration")?;
    let destination = parse_clip_time(project, destination, "duplicate destination")?;
    if source_at < MusicalTime::ZERO || destination < MusicalTime::ZERO || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement duplication requires non-negative times and positive duration".to_owned(),
        ));
    }
    let source_end = source_at
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if source_end > project.song.end || destination > project.song.end {
        return Err(EditError::InvalidValue(
            "arrangement duplicate range and destination must lie within the Song".to_owned(),
        ));
    }
    if destination > source_at && destination < source_end {
        return Err(EditError::InvalidValue(
            "arrangement duplicate destination cannot lie inside its source range".to_owned(),
        ));
    }

    let mut selected = Vec::new();
    let existing_ids = project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .map(|clip| clip.id.as_str())
        .collect::<BTreeSet<_>>();
    for track in &project.song.tracks {
        let mut copies = Vec::new();
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            let overlaps = clip.start < source_end && clip_end > source_at;
            let contained = clip.start >= source_at && clip_end <= source_end;
            if overlaps && !contained {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses the duplicate source boundary; split it first",
                    clip.id
                )));
            }
            if contained {
                let id = format!("{}-{id_suffix}", clip.id);
                if !valid_source_id(&id) || existing_ids.contains(id.as_str()) {
                    return Err(EditError::InvalidValue(format!(
                        "duplicated Clip ID `{id}` is invalid or already exists"
                    )));
                }
                let relative = clip
                    .start
                    .checked_sub(source_at)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                let start = destination
                    .checked_add(relative)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                copies.push((id, clip, start));
            }
        }
        if !copies.is_empty() {
            selected.push((track, copies));
        }
    }
    if selected.is_empty() {
        return Err(EditError::InvalidValue(
            "arrangement duplicate range contains no complete Clips".to_owned(),
        ));
    }

    let mut replacements = plan_arrangement_insertion(bundle, destination, duration, automation)?;
    for (track, copies) in selected {
        let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
        let mut replacement = String::new();
        for (id, clip, start) in copies {
            let mut values = ClipValues::from(clip);
            values.id = &id;
            values.start = start;
            validate_clip_values_with_end(
                project,
                values,
                project
                    .song
                    .end
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?,
            )?;
            replacement.push_str(&canonical_clip(values, &indentation, &indentation));
            replacement.push('\n');
        }
        replacements.push(CheckedReplacement {
            file,
            start: offset,
            end: offset,
            expected: String::new(),
            replacement,
        });
    }
    Ok(replacements)
}

fn plan_arrangement_insertion(
    bundle: &ProjectBundle,
    at: MusicalTime,
    duration: MusicalTime,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if at < MusicalTime::ZERO || at > project.song.end || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement insertion requires a position within the Song and positive duration"
                .to_owned(),
        ));
    }
    let new_song_end = project
        .song
        .end
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let mut replacements = vec![CheckedReplacement::at(
        &project.song.end_site,
        new_song_end.to_string(),
    )];
    for track in &project.song.tracks {
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            if clip.start < at && clip_end > at {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses the insertion boundary; split it at {at} first",
                    clip.id
                )));
            }
            if clip.start >= at {
                let shifted = clip
                    .start
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(
                    &clip.sites.start,
                    shifted.to_string(),
                ));
            }
        }
    }
    for locator in &project.song.locators {
        if locator.at >= at {
            let shifted = locator
                .at
                .checked_add(duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            replacements.push(CheckedReplacement::at(
                &locator.at_site,
                shifted.to_string(),
            ));
        }
    }
    if automation == ArrangementAutomation::FollowArrangement {
        for point in project
            .song
            .automations
            .iter()
            .flat_map(|lane| &lane.points)
        {
            if point.at >= at && point.at > MusicalTime::ZERO {
                let shifted = point
                    .at
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(&point.at_site, shifted.to_string()));
            }
        }
    }
    Ok(replacements)
}

/// Complete source-backed values for one reusable deterministic Groove Template.
#[derive(Debug, Clone)]
pub struct GroovePlacement {
    /// Fresh or existing Groove identity.
    pub id: String,
    /// Positive subdivision grid literal.
    pub grid: String,
    /// Odd-subdivision swing ratio from 0.5 to 0.75.
    pub swing: f64,
    /// Non-negative maximum timing displacement literal.
    pub timing: String,
    /// Maximum normalized signed velocity displacement.
    pub velocity: f64,
    /// Deterministic event inclusion probability.
    pub probability: f64,
    /// Deterministic template seed.
    pub seed: u64,
}

/// Creates one reusable deterministic Groove Template at the Song scope.
pub fn create_groove(
    bundle: &ProjectBundle,
    placement: &GroovePlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Groove Template ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .grooves
        .iter()
        .any(|groove| groove.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Groove Template `{}` already exists",
            placement.id
        )));
    }
    let (grid, timing) = parse_groove_placement(project, placement)?;
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}{}\n",
            canonical_groove(placement, grid, timing)
        ),
    })
}

/// Replaces every checked value of one existing Groove Template without flattening Clips.
pub fn update_groove(
    bundle: &ProjectBundle,
    placement: &GroovePlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let groove = project
        .song
        .grooves
        .iter()
        .find(|groove| groove.id == placement.id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Groove Template `{}`", placement.id)))?;
    let (grid, timing) = parse_groove_placement(project, placement)?;
    Ok(vec![
        CheckedReplacement::at(&groove.sites.grid, grid.to_string()),
        CheckedReplacement::at(&groove.sites.swing, placement.swing.to_string()),
        CheckedReplacement::at(&groove.sites.timing, timing.to_string()),
        CheckedReplacement::at(&groove.sites.velocity, placement.velocity.to_string()),
        CheckedReplacement::at(&groove.sites.probability, placement.probability.to_string()),
        CheckedReplacement::at(&groove.sites.seed, placement.seed.to_string()),
    ])
}

/// Deletes one unused reusable Groove Template declaration.
pub fn delete_groove(
    bundle: &ProjectBundle,
    groove_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let groove = project
        .song
        .grooves
        .iter()
        .find(|groove| groove.id == groove_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Groove Template `{groove_id}`")))?;
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.groove.as_deref() == Some(groove_id))
    {
        return Err(EditError::InvalidValue(format!(
            "Groove Template `{groove_id}` is still referenced by a Clip"
        )));
    }
    line_replacement(bundle, &groove.location)
}

/// Assigns or clears one Clip's reusable Groove Template while preserving procedural source.
pub fn set_clip_groove(
    bundle: &ProjectBundle,
    clip_id: &str,
    groove: Option<&str>,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.groove = groove;
    validate_clip_values(project, values)?;
    replace_clip_declaration(bundle, clip, values)
}

fn parse_groove_placement(
    project: &crate::model::Project,
    placement: &GroovePlacement,
) -> Result<(MusicalTime, MusicalTime), EditError> {
    let grid = parse_clip_time(project, &placement.grid, "Groove grid")?;
    let timing = parse_clip_time(project, &placement.timing, "Groove timing")?;
    if !grid.is_positive() {
        return Err(EditError::InvalidValue(
            "Groove grid must be positive".to_owned(),
        ));
    }
    if !placement.swing.is_finite() || !(0.5..=0.75).contains(&placement.swing) {
        return Err(EditError::InvalidValue(
            "Groove swing must be between 0.5 and 0.75".to_owned(),
        ));
    }
    if timing < MusicalTime::ZERO || !timing.checked_mul(2).is_ok_and(|timing| timing <= grid) {
        return Err(EditError::InvalidValue(
            "Groove timing must be non-negative and at most half the grid".to_owned(),
        ));
    }
    if !placement.velocity.is_finite() || !(0.0..=1.0).contains(&placement.velocity) {
        return Err(EditError::InvalidValue(
            "Groove velocity humanization must be between 0 and 1".to_owned(),
        ));
    }
    if !placement.probability.is_finite() || !(0.0..=1.0).contains(&placement.probability) {
        return Err(EditError::InvalidValue(
            "Groove probability must be between 0 and 1".to_owned(),
        ));
    }
    Ok((grid, timing))
}

fn canonical_groove(placement: &GroovePlacement, grid: MusicalTime, timing: MusicalTime) -> String {
    format!(
        "groove {} grid {} swing {} timing {} velocity {} probability {} seed {}",
        placement.id,
        grid,
        placement.swing,
        timing,
        placement.velocity,
        placement.probability,
        placement.seed
    )
}

/// Complete source-backed values for one procedural Pattern Variation.
#[derive(Debug, Clone)]
pub struct PatternVariationPlacement {
    /// Fresh or existing Pattern identity.
    pub id: String,
    /// Referenced Pattern lineage.
    pub source: String,
    /// Exact cyclic onset rotation literal.
    pub rotate: String,
    /// Whole-semitone note transposition.
    pub transpose_semitones: i16,
    /// Normalized velocity multiplier.
    pub velocity_scale: f64,
    /// Whether onsets are mirrored within the Pattern span.
    pub reverse: bool,
    /// Deterministic event inclusion probability.
    pub probability: f64,
    /// Authored deterministic seed.
    pub seed: u64,
}

/// Creates one procedural Pattern Variation without flattening source Events.
pub fn create_pattern_variation(
    bundle: &ProjectBundle,
    placement: &PatternVariationPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .patterns
        .iter()
        .any(|pattern| pattern.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Pattern `{}` already exists",
            placement.id
        )));
    }
    let (source, rotate) = parse_pattern_variation_placement(project, placement)?;
    if pattern_lineage_reaches(project, &placement.source, &placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation lineage cannot contain a cycle".to_owned(),
        ));
    }
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: canonical_pattern_variation(placement, source.span, rotate, &indentation),
    })
}

/// Replaces every checked transform of one procedural Pattern Variation.
pub fn update_pattern_variation(
    bundle: &ProjectBundle,
    placement: &PatternVariationPlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.id)))?;
    let variation = pattern.variation.as_ref().ok_or_else(|| {
        EditError::WrongTargetKind(format!("Pattern `{}` is not a Variation", placement.id))
    })?;
    let (source, rotate) = parse_pattern_variation_placement(project, placement)?;
    if source.span != pattern.span {
        return Err(EditError::InvalidValue(
            "Pattern Variation span must equal its source Pattern span".to_owned(),
        ));
    }
    if pattern_lineage_reaches(project, &placement.source, &placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation lineage cannot contain a cycle".to_owned(),
        ));
    }
    Ok(vec![
        CheckedReplacement::at(&variation.sites.source, placement.source.clone()),
        CheckedReplacement::at(&variation.sites.rotate, rotate.to_string()),
        CheckedReplacement::at(
            &variation.sites.transpose,
            format!("{}st", placement.transpose_semitones),
        ),
        CheckedReplacement::at(
            &variation.sites.velocity,
            placement.velocity_scale.to_string(),
        ),
        CheckedReplacement::at(&variation.sites.reverse, placement.reverse.to_string()),
        CheckedReplacement::at(
            &variation.sites.probability,
            placement.probability.to_string(),
        ),
        CheckedReplacement::at(&variation.sites.seed, placement.seed.to_string()),
    ])
}

/// Deletes one unreferenced procedural Pattern Variation declaration.
pub fn delete_pattern_variation(
    bundle: &ProjectBundle,
    pattern_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    if pattern.variation.is_none() {
        return Err(EditError::WrongTargetKind(format!(
            "Pattern `{pattern_id}` is not a Variation"
        )));
    }
    if project
        .song
        .patterns
        .iter()
        .filter_map(|candidate| candidate.variation.as_ref())
        .any(|variation| variation.source == pattern_id)
        || project
            .song
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|clip| clip.pattern == pattern_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Pattern Variation `{pattern_id}` is still referenced"
        )));
    }
    line_replacement(bundle, &pattern.location)
}

fn parse_pattern_variation_placement<'a>(
    project: &'a crate::model::Project,
    placement: &PatternVariationPlacement,
) -> Result<(&'a crate::model::Pattern, MusicalTime), EditError> {
    let source = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.source)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.source)))?;
    let rotate = parse_clip_time(project, &placement.rotate, "Pattern Variation rotation")?;
    if rotate < MusicalTime::ZERO || rotate >= source.span {
        return Err(EditError::InvalidValue(
            "Pattern Variation rotation must lie inside its half-open span".to_owned(),
        ));
    }
    if !(-127..=127).contains(&placement.transpose_semitones) {
        return Err(EditError::InvalidValue(
            "Pattern Variation transpose must be between -127st and 127st".to_owned(),
        ));
    }
    if !placement.velocity_scale.is_finite() || !(0.0..=2.0).contains(&placement.velocity_scale) {
        return Err(EditError::InvalidValue(
            "Pattern Variation velocity scale must be between 0 and 2".to_owned(),
        ));
    }
    if !placement.probability.is_finite() || !(0.0..=1.0).contains(&placement.probability) {
        return Err(EditError::InvalidValue(
            "Pattern Variation probability must be between 0 and 1".to_owned(),
        ));
    }
    Ok((source, rotate))
}

fn pattern_lineage_reaches(
    project: &crate::model::Project,
    source_id: &str,
    target_id: &str,
) -> bool {
    let mut current = source_id;
    let mut seen = BTreeSet::new();
    loop {
        if current == target_id || !seen.insert(current) {
            return true;
        }
        let Some(variation) = project
            .song
            .patterns
            .iter()
            .find(|pattern| pattern.id == current)
            .and_then(|pattern| pattern.variation.as_ref())
        else {
            return false;
        };
        current = &variation.source;
    }
}

fn canonical_pattern_variation(
    placement: &PatternVariationPlacement,
    span: MusicalTime,
    rotate: MusicalTime,
    indentation: &str,
) -> String {
    format!(
        "{indentation}pattern {} {span} from {} {{\n\
         {indentation}  rotate {rotate}\n\
         {indentation}  transpose {}st\n\
         {indentation}  velocity {}\n\
         {indentation}  reverse {}\n\
         {indentation}  probability {}\n\
         {indentation}  seed {}\n\
         {indentation}}}\n",
        placement.id,
        placement.source,
        placement.transpose_semitones,
        placement.velocity_scale,
        placement.reverse,
        placement.probability,
        placement.seed,
    )
}

/// Complete source-backed values for one durable Pattern Parameter lock.
#[derive(Debug, Clone)]
pub struct ParameterLockPlacement {
    /// Owning source Pattern.
    pub pattern: String,
    /// Fresh or existing lock identity.
    pub id: String,
    /// Pattern-relative onset literal.
    pub at: String,
    /// Instrument Parameter key.
    pub parameter: String,
    /// Physical value literal.
    pub value: String,
}

/// Creates one checked Pattern Parameter lock declaration.
pub fn create_parameter_lock(
    bundle: &ProjectBundle,
    placement: &ParameterLockPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.pattern)))?;
    if pattern.variation.is_some() {
        return Err(EditError::WrongTargetKind(
            "Parameter locks are authored on source Patterns and inherited by Variations"
                .to_owned(),
        ));
    }
    if !valid_source_id(&placement.id) || pattern.locks.iter().any(|lock| lock.id == placement.id) {
        return Err(EditError::InvalidValue(
            "Parameter lock ID must be fresh and source-safe within the Pattern".to_owned(),
        ));
    }
    let at = validate_parameter_lock_placement(project, pattern, placement, None)?;
    let (file, offset, indentation) = block_insertion(bundle, &pattern.location, "Pattern")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}lock {} at {at} parameter {} value {}\n",
            placement.id, placement.parameter, placement.value
        ),
    })
}

/// Updates one durable Pattern Parameter lock without flattening Events.
pub fn update_parameter_lock(
    bundle: &ProjectBundle,
    placement: &ParameterLockPlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.pattern)))?;
    let lock = pattern
        .locks
        .iter()
        .find(|lock| lock.id == placement.id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!(
                "Parameter lock `{}` in Pattern `{}`",
                placement.id, placement.pattern
            ))
        })?;
    let at = validate_parameter_lock_placement(project, pattern, placement, Some(&placement.id))?;
    Ok(vec![
        CheckedReplacement::at(&lock.sites.at, at.to_string()),
        CheckedReplacement::at(&lock.sites.parameter, placement.parameter.clone()),
        CheckedReplacement::at(&lock.sites.value, placement.value.clone()),
    ])
}

/// Deletes one durable Pattern Parameter lock declaration.
pub fn delete_parameter_lock(
    bundle: &ProjectBundle,
    pattern_id: &str,
    lock_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let pattern = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    let lock = pattern
        .locks
        .iter()
        .find(|lock| lock.id == lock_id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!(
                "Parameter lock `{lock_id}` in Pattern `{pattern_id}`"
            ))
        })?;
    line_replacement(bundle, &lock.location)
}

fn validate_parameter_lock_placement(
    project: &crate::model::Project,
    pattern: &crate::model::Pattern,
    placement: &ParameterLockPlacement,
    updating: Option<&str>,
) -> Result<MusicalTime, EditError> {
    if !valid_source_id(&placement.parameter) || placement.value.is_empty() {
        return Err(EditError::InvalidValue(
            "Parameter lock key must be source-safe and value must be non-empty".to_owned(),
        ));
    }
    let at = parse_clip_time(project, &placement.at, "Parameter lock onset")?;
    if at < MusicalTime::ZERO || at >= pattern.span {
        return Err(EditError::InvalidValue(
            "Parameter lock onset must lie inside the Pattern's half-open span".to_owned(),
        ));
    }
    if pattern.locks.iter().any(|lock| {
        Some(lock.id.as_str()) != updating && lock.at == at && lock.parameter == placement.parameter
    }) {
        return Err(EditError::InvalidValue(
            "Pattern cannot lock the same Parameter more than once at one onset".to_owned(),
        ));
    }
    for track in &project.song.tracks {
        let consumes_pattern = track
            .clips
            .iter()
            .any(|clip| pattern_lineage_reaches(project, &clip.pattern, &pattern.id));
        if !consumes_pattern {
            continue;
        }
        let target = crate::model::ParameterTarget::Instrument {
            instrument: track.instrument.clone(),
            parameter: placement.parameter.clone(),
        };
        let spec = project.parameter_spec(&target).ok_or_else(|| {
            EditError::InvalidValue(format!(
                "Parameter lock targets missing Parameter `{target}`"
            ))
        })?;
        if !spec.automatable {
            return Err(EditError::InvalidValue(format!(
                "Parameter `{target}` is not declared automatable"
            )));
        }
        let value = spec.unit.parse_value(&placement.value).map_err(|error| {
            EditError::InvalidValue(format!(
                "invalid Parameter lock value for {}: {error}",
                spec.unit
            ))
        })?;
        if !(spec.minimum..=spec.maximum).contains(&value) {
            return Err(EditError::InvalidValue(format!(
                "Parameter lock value must be within {} … {}",
                spec.unit.format_value(spec.minimum),
                spec.unit.format_value(spec.maximum)
            )));
        }
    }
    Ok(at)
}

/// Creates one checked named Song Locator declaration.
pub fn create_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
    name: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(locator_id) {
        return Err(EditError::InvalidValue(
            "Locator ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .locators
        .iter()
        .any(|locator| locator.id == locator_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Locator `{locator_id}` already exists"
        )));
    }
    let at = parse_clip_time(project, at, "Locator time")?;
    if at < MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Locator time must lie within the Song arrangement".to_owned(),
        ));
    }
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}locator {locator_id} {} at {at}\n",
            quote_source_string(name)
        ),
    })
}

/// Moves one named Song Locator to an exact arrangement time.
pub fn move_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let locator = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == locator_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Locator `{locator_id}`")))?;
    let at = parse_clip_time(project, at, "Locator time")?;
    if at < MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Locator time must lie within the Song arrangement".to_owned(),
        ));
    }
    Ok(CheckedReplacement::at(&locator.at_site, at.to_string()))
}

/// Deletes one complete named Song Locator declaration.
pub fn delete_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let locator = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == locator_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Locator `{locator_id}`")))?;
    line_replacement(bundle, &locator.location)
}

/// Complete source-backed placement for one new Timeline Clip.
#[derive(Debug, Clone)]
pub struct ClipPlacement {
    /// Fresh Song-wide Clip identity.
    pub id: String,
    /// Existing Pattern identity.
    pub pattern: String,
    /// Timeline start literal.
    pub start: String,
    /// Positive duration literal.
    pub duration: String,
    /// Once or looping traversal.
    pub mode: ClipMode,
    /// Pattern-local phase at the Clip's left edge.
    pub source_offset: String,
    /// Whole-semitone transposition.
    pub transpose_semitones: i16,
    /// Event-excitation gain.
    pub gain_db: f64,
    /// Left-edge fade duration.
    pub fade_in: String,
    /// Right-edge fade duration.
    pub fade_out: String,
    /// Optional reusable Groove Template identity.
    pub groove: Option<String>,
}

#[derive(Clone, Copy)]
struct ClipValues<'a> {
    id: &'a str,
    pattern: &'a str,
    start: MusicalTime,
    duration: MusicalTime,
    mode: ClipMode,
    source_offset: MusicalTime,
    transpose_semitones: i16,
    gain_db: f64,
    fade_in: MusicalTime,
    fade_out: MusicalTime,
    groove: Option<&'a str>,
}

impl<'a> From<&'a Clip> for ClipValues<'a> {
    fn from(clip: &'a Clip) -> Self {
        Self {
            id: &clip.id,
            pattern: &clip.pattern,
            start: clip.start,
            duration: clip.duration,
            mode: clip.mode,
            source_offset: clip.source_offset,
            transpose_semitones: clip.transpose_semitones,
            gain_db: clip.gain_db,
            fade_in: clip.fade_in,
            fade_out: clip.fade_out,
            groove: clip.groove.as_deref(),
        }
    }
}

/// Creates one checked Clip declaration at the end of a Track block.
pub fn create_clip(
    bundle: &ProjectBundle,
    track_id: &str,
    placement: &ClipPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Clip ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Clip `{}` already exists",
            placement.id
        )));
    }
    let track = project
        .song
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Track `{track_id}`")))?;
    let values = ClipValues {
        id: &placement.id,
        pattern: &placement.pattern,
        start: parse_clip_time(project, &placement.start, "Clip start")?,
        duration: parse_clip_time(project, &placement.duration, "Clip duration")?,
        mode: placement.mode,
        source_offset: parse_clip_time(project, &placement.source_offset, "Clip source offset")?,
        transpose_semitones: placement.transpose_semitones,
        gain_db: placement.gain_db,
        fade_in: parse_clip_time(project, &placement.fade_in, "Clip fade in")?,
        fade_out: parse_clip_time(project, &placement.fade_out, "Clip fade out")?,
        groove: placement.groove.as_deref(),
    };
    validate_clip_values(project, values)?;
    let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{}\n", canonical_clip(values, &indentation, &indentation)),
    })
}

/// Deletes one durably identified Clip declaration.
pub fn delete_clip(bundle: &ProjectBundle, clip_id: &str) -> Result<CheckedReplacement, EditError> {
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    line_replacement(bundle, &clip.location)
}

/// Moves one Clip without changing its Pattern traversal or duration.
pub fn move_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    start: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let start = parse_clip_time(project, start, "Clip start")?;
    let mut values = ClipValues::from(clip);
    values.start = start;
    validate_clip_values(project, values)?;
    Ok(CheckedReplacement::at(&clip.sites.start, start.to_string()))
}

/// Resizes one Clip's half-open Timeline duration.
pub fn resize_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    duration: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let duration = parse_clip_time(project, duration, "Clip duration")?;
    let mut values = ClipValues::from(clip);
    values.duration = duration;
    validate_clip_values(project, values)?;
    Ok(CheckedReplacement::at(
        &clip.sites.duration,
        duration.to_string(),
    ))
}

/// Duplicates one Clip with a fresh identity and independent Timeline start.
pub fn duplicate_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    new_id: &str,
    start: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    ensure_fresh_clip_id(project, new_id)?;
    let (track, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.id = new_id;
    values.start = parse_clip_time(project, start, "Clip start")?;
    validate_clip_values(project, values)?;
    let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{}\n", canonical_clip(values, &indentation, &indentation)),
    })
}

/// Splits one Clip into a shortened left declaration and a fresh right declaration.
///
/// The split must lie after any left-edge fade. The right Clip carries the original right-edge
/// fade, and loop traversal continues from the exact Pattern-local phase.
pub fn split_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    new_id: &str,
    at: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    ensure_fresh_clip_id(project, new_id)?;
    let (track, clip) = arrangement_clip(bundle, clip_id)?;
    let split = parse_clip_time(project, at, "Clip split")?;
    let end = clip
        .start
        .checked_add(clip.duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if split <= clip.start || split >= end {
        return Err(EditError::InvalidValue(
            "Clip split must lie strictly inside the Clip".to_owned(),
        ));
    }
    let left_duration = split
        .checked_sub(clip.start)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if left_duration < clip.fade_in {
        return Err(EditError::InvalidValue(
            "Clip split cannot cut through its fade-in".to_owned(),
        ));
    }
    let right_duration = end
        .checked_sub(split)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == clip.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", clip.pattern)))?;
    let advanced = clip
        .source_offset
        .checked_add(left_duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let right_offset = match clip.mode {
        ClipMode::Loop => musical_time_mod(advanced, pattern.span)?,
        ClipMode::Once if advanced < pattern.span => advanced,
        ClipMode::Once => {
            return Err(EditError::InvalidValue(
                "cannot split an exhausted once Clip after its Pattern span".to_owned(),
            ));
        }
    };

    let mut left = ClipValues::from(clip);
    left.duration = left_duration;
    left.fade_out = MusicalTime::ZERO;
    validate_clip_values(project, left)?;
    let mut right = ClipValues::from(clip);
    right.id = new_id;
    right.start = split;
    right.duration = right_duration;
    right.source_offset = right_offset;
    right.fade_in = MusicalTime::ZERO;
    validate_clip_values(project, right)?;

    let indentation = declaration_indentation(bundle, &clip.location)?;
    let left_replacement =
        CheckedReplacement::at(&clip.sites.site, canonical_clip(left, "", &indentation));
    let (file, offset, track_indentation) = block_insertion(bundle, &track.location, "Track")?;
    let right_insertion = CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{}\n",
            canonical_clip(right, &track_indentation, &track_indentation)
        ),
    };
    Ok(vec![left_replacement, right_insertion])
}

/// Sets a Clip's Pattern-local source offset.
pub fn set_clip_source_offset(
    bundle: &ProjectBundle,
    clip_id: &str,
    source_offset: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let source_offset = parse_clip_time(project, source_offset, "Clip source offset")?;
    let mut values = ClipValues::from(clip);
    values.source_offset = source_offset;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.source_offset.as_ref(),
        source_offset.to_string(),
        values,
    )
}

/// Sets a Clip's whole-semitone transposition.
pub fn set_clip_transpose(
    bundle: &ProjectBundle,
    clip_id: &str,
    semitones: i16,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.transpose_semitones = semitones;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.transpose.as_ref(),
        format!("{semitones}st"),
        values,
    )
}

/// Sets a Clip's event-excitation gain in decibels.
pub fn set_clip_gain(
    bundle: &ProjectBundle,
    clip_id: &str,
    gain_db: f64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.gain_db = gain_db;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.gain.as_ref(),
        Unit::Decibels.format_value(gain_db),
        values,
    )
}

/// Sets both source-backed Clip fade durations.
pub fn set_clip_fades(
    bundle: &ProjectBundle,
    clip_id: &str,
    fade_in: &str,
    fade_out: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let fade_in = parse_clip_time(project, fade_in, "Clip fade in")?;
    let fade_out = parse_clip_time(project, fade_out, "Clip fade out")?;
    let mut values = ClipValues::from(clip);
    values.fade_in = fade_in;
    values.fade_out = fade_out;
    validate_clip_values(project, values)?;
    if let (Some(fade_in_site), Some(fade_out_site)) =
        (clip.sites.fade_in.as_ref(), clip.sites.fade_out.as_ref())
    {
        Ok(vec![
            CheckedReplacement::at(fade_in_site, fade_in.to_string()),
            CheckedReplacement::at(fade_out_site, fade_out.to_string()),
        ])
    } else {
        Ok(vec![replace_clip_declaration(bundle, clip, values)?])
    }
}

fn replace_clip_field_or_declaration(
    bundle: &ProjectBundle,
    clip: &Clip,
    site: Option<&ValueSite>,
    literal: String,
    values: ClipValues<'_>,
) -> Result<Vec<CheckedReplacement>, EditError> {
    if let Some(site) = site {
        Ok(vec![CheckedReplacement::at(site, literal)])
    } else {
        Ok(vec![replace_clip_declaration(bundle, clip, values)?])
    }
}

fn replace_clip_declaration(
    bundle: &ProjectBundle,
    clip: &Clip,
    values: ClipValues<'_>,
) -> Result<CheckedReplacement, EditError> {
    let indentation = declaration_indentation(bundle, &clip.location)?;
    Ok(CheckedReplacement::at(
        &clip.sites.site,
        canonical_clip(values, "", &indentation),
    ))
}

fn arrangement_clip<'a>(
    bundle: &'a ProjectBundle,
    clip_id: &str,
) -> Result<(&'a Track, &'a Clip), EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    project
        .song
        .tracks
        .iter()
        .find_map(|track| {
            track
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| (track, clip))
        })
        .ok_or_else(|| EditError::UnknownTarget(format!("Clip `{clip_id}`")))
}

fn ensure_fresh_clip_id(project: &crate::model::Project, clip_id: &str) -> Result<(), EditError> {
    if !valid_source_id(clip_id) {
        return Err(EditError::InvalidValue(
            "Clip ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.id == clip_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Clip `{clip_id}` already exists"
        )));
    }
    Ok(())
}

fn parse_clip_time(
    project: &crate::model::Project,
    value: &str,
    label: &str,
) -> Result<MusicalTime, EditError> {
    parse_musical_time(value, project.song.meter)
        .map_err(|error| EditError::InvalidValue(format!("{label}: {error}")))
}

fn validate_clip_values(
    project: &crate::model::Project,
    values: ClipValues<'_>,
) -> Result<(), EditError> {
    validate_clip_values_with_end(project, values, project.song.end)
}

fn validate_clip_values_with_end(
    project: &crate::model::Project,
    values: ClipValues<'_>,
    arrangement_end: MusicalTime,
) -> Result<(), EditError> {
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == values.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", values.pattern)))?;
    if values.start < MusicalTime::ZERO || !values.duration.is_positive() {
        return Err(EditError::InvalidValue(
            "Clip start must be non-negative and duration must be positive".to_owned(),
        ));
    }
    let end = values
        .start
        .checked_add(values.duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if end > arrangement_end {
        return Err(EditError::InvalidValue(
            "Clip extends beyond the Song arrangement end".to_owned(),
        ));
    }
    if values.source_offset < MusicalTime::ZERO || values.source_offset >= pattern.span {
        return Err(EditError::InvalidValue(
            "Clip source offset must lie inside the Pattern span".to_owned(),
        ));
    }
    if !(-127..=127).contains(&values.transpose_semitones) {
        return Err(EditError::InvalidValue(
            "Clip transpose must be between -127st and 127st".to_owned(),
        ));
    }
    if pattern.events.iter().any(|event| {
        matches!(
            event,
            PatternEvent::Note { pitch, .. }
                if !(0..=127).contains(&(i16::from(*pitch) + values.transpose_semitones))
        )
    }) {
        return Err(EditError::InvalidValue(
            "Clip transpose moves a note outside the MIDI pitch domain".to_owned(),
        ));
    }
    if let Some(groove) = values.groove
        && !project
            .song
            .grooves
            .iter()
            .any(|template| template.id == groove)
    {
        return Err(EditError::UnknownTarget(format!(
            "Groove Template `{groove}`"
        )));
    }
    if !values.gain_db.is_finite() || !(-96.0..=24.0).contains(&values.gain_db) {
        return Err(EditError::OutOfRange {
            target: "Clip gain".to_owned(),
            minimum: -96.0,
            maximum: 24.0,
            unit: Unit::Decibels,
        });
    }
    let fades = values
        .fade_in
        .checked_add(values.fade_out)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if values.fade_in < MusicalTime::ZERO
        || values.fade_out < MusicalTime::ZERO
        || fades > values.duration
    {
        return Err(EditError::InvalidValue(
            "Clip fades must be non-negative and cannot exceed Clip duration".to_owned(),
        ));
    }
    Ok(())
}

fn declaration_indentation(
    bundle: &ProjectBundle,
    location: &SourceLocation,
) -> Result<String, EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    let line_start = source[..location.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let indentation = source
        .get(line_start..location.start)
        .ok_or_else(|| EditError::OverlappingOrInvalidRange(location.file.clone()))?;
    if !indentation.chars().all(char::is_whitespace) {
        return Err(EditError::InvalidValue(
            "Clip declaration indentation is not whitespace".to_owned(),
        ));
    }
    Ok(indentation.to_owned())
}

fn canonical_clip(values: ClipValues<'_>, first_indent: &str, indentation: &str) -> String {
    let mode = match values.mode {
        ClipMode::Once => "once",
        ClipMode::Loop => "loop",
    };
    let child = format!("{indentation}  ");
    let mut source = format!(
        "{first_indent}clip {} pattern {} start {} duration {} mode {mode} {{\n{child}offset {}\n{child}transpose {}st\n{child}gain {}\n{child}fade {} {}",
        values.id,
        values.pattern,
        values.start,
        values.duration,
        values.source_offset,
        values.transpose_semitones,
        Unit::Decibels.format_value(values.gain_db),
        values.fade_in,
        values.fade_out,
    );
    if let Some(groove) = values.groove {
        source.push_str(&format!("\n{child}groove {groove}"));
    }
    source.push_str(&format!("\n{indentation}}}"));
    source
}

fn musical_time_mod(value: MusicalTime, modulus: MusicalTime) -> Result<MusicalTime, EditError> {
    if !modulus.is_positive() || value < MusicalTime::ZERO {
        return Err(EditError::InvalidValue(
            "musical modulo requires non-negative time and positive span".to_owned(),
        ));
    }
    let numerator = i128::from(value.numerator()) * i128::from(modulus.denominator());
    let denominator = i128::from(value.denominator()) * i128::from(modulus.numerator());
    let quotient = u64::try_from(numerator / denominator)
        .map_err(|_| EditError::InvalidValue("musical modulo overflows".to_owned()))?;
    let consumed = modulus
        .checked_mul(quotient)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    value
        .checked_sub(consumed)
        .map_err(|error| EditError::InvalidValue(error.to_string()))
}

/// Builds a checked Event Grid pitch replacement.
pub fn set_note_pitch(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    pitch: u8,
) -> Result<CheckedReplacement, EditError> {
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { pitch_site, .. } => {
            Ok(CheckedReplacement::at(pitch_site, format_pitch(pitch)))
        }
        PatternEvent::Hit { .. } | PatternEvent::Sample { .. } => Err(EditError::WrongTargetKind(
            "this Event has no pitched-note value".to_owned(),
        )),
    }
}

/// Builds a checked Event Grid onset replacement in authored musical units.
pub fn set_event_at(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let (pattern, event) = pattern_event(bundle, pattern_id, event_index)?;
    let meter = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .meter;
    let parsed = parse_musical_time(value, meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if parsed < crate::time::MusicalTime::ZERO || parsed >= pattern.span {
        return Err(EditError::InvalidValue(format!(
            "Event onset must be within Pattern `{pattern_id}` range 0..{}",
            pattern.span
        )));
    }
    let site = match event {
        PatternEvent::Note { at_site, .. } | PatternEvent::Hit { at_site, .. } => at_site,
        PatternEvent::Sample { sites, .. } => &sites.at,
    };
    Ok(CheckedReplacement::at(site, parsed.to_string()))
}

/// Builds a checked pitched-note duration replacement.
pub fn set_note_duration(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let meter = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .meter;
    let parsed = parse_musical_time(value, meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if !parsed.is_positive() {
        return Err(EditError::InvalidValue(
            "note duration must be greater than zero".to_owned(),
        ));
    }
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { duration_site, .. } => {
            Ok(CheckedReplacement::at(duration_site, parsed.to_string()))
        }
        PatternEvent::Sample { sites, .. } => {
            Ok(CheckedReplacement::at(&sites.duration, parsed.to_string()))
        }
        PatternEvent::Hit { .. } => Err(EditError::WrongTargetKind(
            "a percussion hit has no duration".to_owned(),
        )),
    }
}

/// Builds a checked note-or-hit velocity replacement.
pub fn set_event_velocity(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    velocity: f64,
) -> Result<CheckedReplacement, EditError> {
    if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
        return Err(EditError::OutOfRange {
            target: format!("Event {event_index} velocity"),
            minimum: 0.0,
            maximum: 1.0,
            unit: Unit::Ratio,
        });
    }
    let site = match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { velocity_site, .. } | PatternEvent::Hit { velocity_site, .. } => {
            velocity_site
        }
        PatternEvent::Sample { sites, .. } => &sites.velocity,
    };
    Ok(CheckedReplacement::at(
        site,
        Unit::Ratio.format_value(velocity),
    ))
}
