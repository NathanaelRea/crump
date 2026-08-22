use super::*;

/// Copies one external WAV into `media/`, declares it, and publishes atomically.
pub fn import_media_asset(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    source: impl AsRef<Path>,
    id: &str,
    name: &str,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    if !valid_source_id(id) {
        return Err(EditError::InvalidValue(
            "media asset id must start with an ASCII letter and contain only letters, digits, `_`, or `-`"
                .to_owned(),
        ));
    }
    if name.trim().is_empty() || name.chars().any(|character| character.is_control()) {
        return Err(EditError::InvalidValue(
            "media asset name must be non-empty and contain no control characters".to_owned(),
        ));
    }
    let source = source.as_ref();
    let bytes = match read_regular_asset_bytes(source) {
        Ok(bytes) => bytes,
        Err(AssetReadError::Unsafe(message)) => {
            return Err(EditError::InvalidValue(format!(
                "media import input `{}` is unsafe: {message}",
                source.display()
            )));
        }
        Err(AssetReadError::Unreadable(source_error)) => {
            return Err(EditError::Io {
                path: source.to_path_buf(),
                source: source_error,
            });
        }
        Err(AssetReadError::TooLarge) => {
            return Err(EditError::InvalidValue(format!(
                "media import exceeds the {MAX_MEDIA_ASSET_BYTES}-byte maximum"
            )));
        }
    };
    let bundle = load_project(root.as_ref())?;
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if project.assets.iter().any(|asset| asset.id == id) {
        return Err(EditError::InvalidValue(format!(
            "media asset id `{id}` already exists"
        )));
    }
    let destination = PathBuf::from(format!("media/{id}.wav"));
    if project.assets.iter().any(|asset| asset.path == destination) {
        return Err(EditError::AssetDestinationExists(destination));
    }
    let (file, position, indentation) = line_insertion(&bundle, &project.song.location, "Song")?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let declaration = format!(
        "{indentation}asset {id} {} wav \"{}\" sha256 \"{digest}\"\n",
        quote_source_string(name),
        destination.display()
    );
    apply_project_mutation(
        root,
        expected_revision,
        ProjectMutation {
            replacements: vec![CheckedReplacement {
                file,
                start: position,
                end: position,
                expected: String::new(),
                replacement: declaration,
            }],
            asset_writes: vec![StagedAssetWrite {
                path: destination,
                bytes,
                replace: false,
            }],
            asset_removals: Vec::new(),
        },
        publication,
    )
}

/// Collects every declared WAV into the canonical `media/<asset-id>.wav` layout.
///
/// Exact bytes, hashes, durable IDs, and history provenance are preserved; only
/// normalized source paths and corresponding owned files change.
pub fn collect_media_assets(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    let bundle = load_project(root.as_ref())?;
    if bundle.diagnostics.has_errors() {
        return Err(EditError::InvalidBase);
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let mut mutation = ProjectMutation::default();
    for asset in &project.assets {
        let destination = PathBuf::from("media").join(format!("{}.wav", asset.id));
        if asset.path == destination {
            continue;
        }
        let loaded = bundle
            .assets
            .get(&asset.id)
            .ok_or_else(|| EditError::UnknownTarget(format!("media asset `{}`", asset.id)))?;
        mutation.replacements.push(CheckedReplacement::at(
            &asset.path_site,
            quote_source_string(&destination.to_string_lossy()),
        ));
        mutation.asset_writes.push(StagedAssetWrite {
            path: destination,
            bytes: loaded.bytes().to_vec(),
            replace: false,
        });
        mutation.asset_removals.push(asset.path.clone());
    }
    apply_project_mutation(root, expected_revision, mutation, publication)
}

pub(super) fn valid_source_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

pub(super) fn quote_source_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Source-backed arguments for placing one durable sample occurrence.
#[derive(Debug, Clone)]
pub struct SamplePlacement {
    /// Durable occurrence identity within the Pattern.
    pub id: String,
    /// Durable mapping identity within the Track's sampler Instrument.
    pub mapping: String,
    /// Pattern-local onset in authored musical units.
    pub at: String,
    /// Occurrence duration in authored musical units.
    pub duration: String,
    /// Inclusive source start in original asset frames.
    pub start_frame: u64,
    /// Exclusive source end in original asset frames.
    pub end_frame: u64,
    /// Occurrence gain trim in decibels.
    pub gain_db: f64,
    /// Occurrence pan in the inclusive -1..=1 domain.
    pub pan: f64,
    /// Click-prevention fade-in in original asset frames.
    pub fade_in_frames: u32,
    /// Click-prevention fade-out in original asset frames.
    pub fade_out_frames: u32,
    /// Occurrence velocity in the inclusive 0..=1 domain.
    pub velocity: f64,
}

/// Places one non-destructive sample occurrence into a Pattern block.
pub fn place_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    placement: &SamplePlacement,
) -> Result<CheckedReplacement, EditError> {
    if !valid_source_id(&placement.id) || !valid_source_id(&placement.mapping) {
        return Err(EditError::InvalidValue(
            "sample occurrence and mapping IDs must be source-safe".to_owned(),
        ));
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    if pattern.events.iter().any(
        |event| matches!(event, PatternEvent::Sample { id, .. } if id == placement.id.as_str()),
    ) {
        return Err(EditError::InvalidValue(format!(
            "sample occurrence `{}` already exists in Pattern `{pattern_id}`",
            placement.id
        )));
    }
    let at = parse_musical_time(&placement.at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let duration = parse_musical_time(&placement.duration, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_sample_values(
        pattern,
        at,
        duration,
        placement.start_frame,
        placement.end_frame,
        placement.gain_db,
        placement.pan,
        placement.fade_in_frames,
        placement.fade_out_frames,
        placement.velocity,
    )?;
    let event = canonical_sample_event(
        &placement.id,
        at,
        &placement.mapping,
        duration,
        placement.start_frame,
        placement.end_frame,
        placement.gain_db,
        placement.pan,
        placement.fade_in_frames,
        placement.fade_out_frames,
        placement.velocity,
    );
    let (file, start, indentation) = block_insertion(bundle, &pattern.location, "Pattern")?;
    Ok(CheckedReplacement {
        file,
        start,
        end: start,
        expected: String::new(),
        replacement: format!("{indentation}{event}\n"),
    })
}

/// Moves one durable sample occurrence without depending on Event ordering.
pub fn move_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (pattern, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let parsed = parse_musical_time(value, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if parsed < crate::time::MusicalTime::ZERO || parsed >= pattern.span {
        return Err(EditError::InvalidValue(format!(
            "sample onset must be within Pattern `{pattern_id}` range 0..{}",
            pattern.span
        )));
    }
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(&sites.at, parsed.to_string()))
}

/// Trims one sample occurrence to an explicit half-open source-frame region.
pub fn trim_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    start_frame: u64,
    end_frame: u64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        fade_in_frames,
        fade_out_frames,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    validate_source_region(start_frame, end_frame, *fade_in_frames, *fade_out_frames)?;
    Ok(vec![
        CheckedReplacement::at(&sites.start, start_frame.to_string()),
        CheckedReplacement::at(&sites.end, end_frame.to_string()),
    ])
}

/// Slips one occurrence through its source while preserving selected length.
pub fn slip_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    frame_delta: i64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        start_frame,
        end_frame,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    let shifted = |frame: u64| {
        if frame_delta >= 0 {
            frame.checked_add(frame_delta as u64)
        } else {
            frame.checked_sub(frame_delta.unsigned_abs())
        }
        .ok_or_else(|| EditError::InvalidValue("sample slip exceeds source-frame range".to_owned()))
    };
    trim_sample_occurrence(
        bundle,
        pattern_id,
        occurrence_id,
        shifted(*start_frame)?,
        shifted(*end_frame)?,
    )
}

/// Sets click-prevention fades for one durable sample occurrence.
pub fn fade_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    fade_in_frames: u32,
    fade_out_frames: u32,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        start_frame,
        end_frame,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    validate_source_region(*start_frame, *end_frame, fade_in_frames, fade_out_frames)?;
    Ok(vec![
        CheckedReplacement::at(&sites.fade_in, fade_in_frames.to_string()),
        CheckedReplacement::at(&sites.fade_out, fade_out_frames.to_string()),
    ])
}

/// Sets one occurrence's gain trim in decibels.
pub fn set_sample_occurrence_gain(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    gain_db: f64,
) -> Result<CheckedReplacement, EditError> {
    if !gain_db.is_finite() || !(-96.0..=24.0).contains(&gain_db) {
        return Err(EditError::InvalidValue(
            "sample gain must be -96db..=24db".to_owned(),
        ));
    }
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(
        &sites.gain,
        Unit::Decibels.format_value(gain_db),
    ))
}

/// Sets one occurrence's stereo pan.
pub fn set_sample_occurrence_pan(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    pan: f64,
) -> Result<CheckedReplacement, EditError> {
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(EditError::InvalidValue(
            "sample pan must be -1..=1".to_owned(),
        ));
    }
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(
        &sites.pan,
        Unit::Pan.format_value(pan),
    ))
}

/// Duplicates one occurrence under a new durable ID and onset.
pub fn duplicate_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    new_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        mapping,
        duration,
        start_frame,
        end_frame,
        gain_db,
        pan,
        fade_in_frames,
        fade_out_frames,
        velocity,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    place_sample_occurrence(
        bundle,
        pattern_id,
        &SamplePlacement {
            id: new_id.to_owned(),
            mapping: mapping.clone(),
            at: at.to_owned(),
            duration: duration.to_string(),
            start_frame: *start_frame,
            end_frame: *end_frame,
            gain_db: *gain_db,
            pan: *pan,
            fade_in_frames: *fade_in_frames,
            fade_out_frames: *fade_out_frames,
            velocity: *velocity,
        },
    )
}

/// Splits one occurrence into two source-backed occurrences in one checked edit.
pub fn split_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    new_id: &str,
    at: &str,
    source_frame: u64,
) -> Result<CheckedReplacement, EditError> {
    if !valid_source_id(new_id) {
        return Err(EditError::InvalidValue(
            "new sample occurrence ID is invalid".to_owned(),
        ));
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    if sample_occurrence(bundle, pattern_id, new_id).is_ok() {
        return Err(EditError::InvalidValue(format!(
            "sample occurrence `{new_id}` already exists"
        )));
    }
    let PatternEvent::Sample {
        id,
        at: original_at,
        mapping,
        duration,
        start_frame,
        end_frame,
        gain_db,
        pan,
        fade_in_frames,
        fade_out_frames,
        velocity,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    let split_at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let event_end = original_at
        .checked_add(*duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if split_at <= *original_at
        || split_at >= event_end
        || source_frame <= *start_frame
        || source_frame >= *end_frame
    {
        return Err(EditError::InvalidValue(
            "split must lie strictly inside both the occurrence time and source region".to_owned(),
        ));
    }
    let left_duration = split_at
        .checked_sub(*original_at)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let right_duration = event_end
        .checked_sub(split_at)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let left_frames = source_frame - *start_frame;
    let right_frames = *end_frame - source_frame;
    let left_fade_in = u64::from(*fade_in_frames).min(left_frames) as u32;
    let left_fade_out =
        u64::from(*fade_out_frames).min(left_frames - u64::from(left_fade_in)) as u32;
    let right_fade_out = u64::from(*fade_out_frames).min(right_frames) as u32;
    let right_fade_in =
        u64::from(*fade_in_frames).min(right_frames - u64::from(right_fade_out)) as u32;
    let left = canonical_sample_event(
        id,
        *original_at,
        mapping,
        left_duration,
        *start_frame,
        source_frame,
        *gain_db,
        *pan,
        left_fade_in,
        left_fade_out,
        *velocity,
    );
    let right = canonical_sample_event(
        new_id,
        split_at,
        mapping,
        right_duration,
        source_frame,
        *end_frame,
        *gain_db,
        *pan,
        right_fade_in,
        right_fade_out,
        *velocity,
    );
    let source = bundle
        .source(&sites.site.file)
        .ok_or_else(|| EditError::UnknownSource(sites.site.file.clone()))?;
    let line_start = source[..sites.site.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let indentation = &source[line_start..sites.site.start];
    Ok(CheckedReplacement::at(
        &sites.site,
        format!("{left}\n{indentation}{right}"),
    ))
}

#[allow(clippy::too_many_arguments)]
fn canonical_sample_event(
    id: &str,
    at: crate::time::MusicalTime,
    mapping: &str,
    duration: crate::time::MusicalTime,
    start_frame: u64,
    end_frame: u64,
    gain_db: f64,
    pan: f64,
    fade_in_frames: u32,
    fade_out_frames: u32,
    velocity: f64,
) -> String {
    format!(
        "sample {id} {at} mapping {mapping} duration {duration} start {start_frame} end {end_frame} gain {} pan {} fade {fade_in_frames} {fade_out_frames} velocity {}",
        Unit::Decibels.format_value(gain_db),
        Unit::Pan.format_value(pan),
        Unit::Ratio.format_value(velocity),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_sample_values(
    pattern: &crate::model::Pattern,
    at: crate::time::MusicalTime,
    duration: crate::time::MusicalTime,
    start_frame: u64,
    end_frame: u64,
    gain_db: f64,
    pan: f64,
    fade_in_frames: u32,
    fade_out_frames: u32,
    velocity: f64,
) -> Result<(), EditError> {
    if at < crate::time::MusicalTime::ZERO || at >= pattern.span || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "sample time must lie in the Pattern and duration must be positive".to_owned(),
        ));
    }
    if !gain_db.is_finite() || !(-96.0..=24.0).contains(&gain_db) {
        return Err(EditError::InvalidValue(
            "sample gain must be -96db..=24db".to_owned(),
        ));
    }
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(EditError::InvalidValue(
            "sample pan must be -1..=1".to_owned(),
        ));
    }
    if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
        return Err(EditError::InvalidValue(
            "sample velocity must be 0..=1".to_owned(),
        ));
    }
    validate_source_region(start_frame, end_frame, fade_in_frames, fade_out_frames)
}

fn validate_source_region(
    start_frame: u64,
    end_frame: u64,
    fade_in_frames: u32,
    fade_out_frames: u32,
) -> Result<(), EditError> {
    if start_frame >= end_frame
        || u64::from(fade_in_frames).saturating_add(u64::from(fade_out_frames))
            > end_frame.saturating_sub(start_frame)
    {
        return Err(EditError::InvalidValue(
            "sample source region must have start < end and contain both fades".to_owned(),
        ));
    }
    Ok(())
}

fn sample_occurrence<'a>(
    bundle: &'a ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
) -> Result<(&'a crate::model::Pattern, &'a PatternEvent), EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    let event = pattern
        .events
        .iter()
        .find(|event| matches!(event, PatternEvent::Sample { id, .. } if id == occurrence_id))
        .ok_or_else(|| {
            EditError::InvalidValue(format!(
                "Pattern `{pattern_id}` has no sample occurrence `{occurrence_id}`"
            ))
        })?;
    Ok((pattern, event))
}

/// Builds a checked stock-percussion voice replacement.
pub fn set_hit_voice(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    voice: DrumVoice,
) -> Result<CheckedReplacement, EditError> {
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Hit { voice_site, .. } => Ok(CheckedReplacement::at(
            voice_site,
            format!("{voice:?}").to_lowercase(),
        )),
        PatternEvent::Note { .. } | PatternEvent::Sample { .. } => Err(EditError::WrongTargetKind(
            "this Event has no percussion voice".to_owned(),
        )),
    }
}
