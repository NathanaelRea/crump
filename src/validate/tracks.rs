//! Groove Template, Track, Clip, and Parameter lock validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{ClipMode, InstrumentKind, ParameterTarget, Pattern, PatternEvent, Project},
    routing::{AuthoredTopology, ChannelRole, RoutingFindingKind},
};

use super::{
    controls::automation_target_is_runtime_safe, located_error, source_site_error, valid_id,
};

pub(super) fn validate_tracks(
    project: &Project,
    topology: &AuthoredTopology,
    diagnostics: &mut Diagnostics,
) {
    let instruments: BTreeMap<_, _> = project
        .song
        .instruments
        .iter()
        .map(|instrument| (instrument.id.as_str(), instrument))
        .collect();
    let patterns: BTreeMap<_, _> = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect();
    let mut groove_ids = BTreeSet::new();
    for groove in &project.song.grooves {
        if !valid_id(&groove.id) || !groove_ids.insert(groove.id.as_str()) {
            located_error(
                diagnostics,
                "CRV146",
                format!(
                    "Groove Template id `{}` is invalid or duplicated",
                    groove.id
                ),
                &groove.location,
            );
        }
        if !groove.grid.is_positive() {
            source_site_error(
                diagnostics,
                "CRV147",
                "Groove grid must be positive",
                &groove.sites.grid,
            );
        }
        if !groove.swing.is_finite() || !(0.5..=0.75).contains(&groove.swing) {
            source_site_error(
                diagnostics,
                "CRV148",
                "Groove swing must be between 0.5 (straight) and 0.75",
                &groove.sites.swing,
            );
        }
        let timing_valid = groove.timing >= crate::time::MusicalTime::ZERO
            && groove
                .timing
                .checked_mul(2)
                .is_ok_and(|timing| timing <= groove.grid);
        if !timing_valid {
            source_site_error(
                diagnostics,
                "CRV149",
                "Groove timing humanization must be non-negative and at most half the grid",
                &groove.sites.timing,
            );
        }
        if !groove.velocity.is_finite() || !(0.0..=1.0).contains(&groove.velocity) {
            source_site_error(
                diagnostics,
                "CRV150",
                "Groove velocity humanization must be between 0 and 1",
                &groove.sites.velocity,
            );
        }
        if !groove.probability.is_finite() || !(0.0..=1.0).contains(&groove.probability) {
            source_site_error(
                diagnostics,
                "CRV151",
                "Groove probability must be between 0 and 1",
                &groove.sites.probability,
            );
        }
    }

    let mut clip_ids = BTreeSet::new();
    let mut instrument_clips = Vec::new();
    for (track_index, track) in project.song.tracks.iter().enumerate() {
        if topology.findings().iter().any(|finding| {
            matches!(
                &finding.kind,
                RoutingFindingKind::InvalidChannelId {
                    role: ChannelRole::Track,
                    index,
                    ..
                } if *index == track_index
            )
        }) {
            located_error(
                diagnostics,
                "CRV070",
                format!("Track id `{}` is invalid or duplicated", track.id),
                &track.location,
            );
        }
        let instrument = instruments.get(track.instrument.as_str()).copied();
        if instrument.is_none() {
            located_error(
                diagnostics,
                "CRV071",
                format!("Track references missing instrument `{}`", track.instrument),
                &track.location,
            );
        }
        if !track.channel.level_db.is_finite() || !(-96.0..=12.0).contains(&track.channel.level_db)
        {
            source_site_error(
                diagnostics,
                "CRV072",
                "Track level must be between -96db and 12db",
                &track.channel.level_site,
            );
        }
        let pan = track.channel.panner.primary_position();
        if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
            source_site_error(
                diagnostics,
                "CRV073",
                "Track pan must be between -1 and 1",
                track.channel.panner.primary_site(),
            );
        }
        for clip in &track.clips {
            if !valid_id(&clip.id) || !clip_ids.insert(clip.id.as_str()) {
                located_error(
                    diagnostics,
                    "CRV074",
                    format!("Clip id `{}` is invalid or duplicated", clip.id),
                    &clip.location,
                );
            }
            let pattern = patterns.get(clip.pattern.as_str()).copied();
            let resolved_pattern =
                pattern.and_then(|pattern| resolve_pattern_source(pattern, &patterns));
            if pattern.is_none() {
                located_error(
                    diagnostics,
                    "CRV075",
                    format!("Clip references missing Pattern `{}`", clip.pattern),
                    &clip.location,
                );
            }
            if clip.start < crate::time::MusicalTime::ZERO || !clip.duration.is_positive() {
                located_error(
                    diagnostics,
                    "CRV076",
                    "Clip start must be non-negative and duration must be positive",
                    &clip.location,
                );
            }
            let end = clip.start.checked_add(clip.duration);
            if end.is_err() || end.is_ok_and(|end| end > project.song.end) {
                located_error(
                    diagnostics,
                    "CRV077",
                    "Clip extends beyond the Song arrangement end",
                    &clip.location,
                );
            }
            if clip.mode == ClipMode::Loop
                && pattern.is_some_and(|pattern| !pattern.span.is_positive())
            {
                located_error(
                    diagnostics,
                    "CRV078",
                    "loop Clip requires a positive Pattern span",
                    &clip.location,
                );
            }
            if pattern.is_some_and(|pattern| {
                clip.source_offset < crate::time::MusicalTime::ZERO
                    || clip.source_offset >= pattern.span
            }) {
                if let Some(site) = clip.sites.source_offset.as_ref() {
                    source_site_error(
                        diagnostics,
                        "CRV084",
                        "Clip source offset must lie inside the Pattern span",
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV084",
                        "Clip source offset must lie inside the Pattern span",
                        &clip.location,
                    );
                }
            }
            if !(-127..=127).contains(&clip.transpose_semitones) {
                if let Some(site) = clip.sites.transpose.as_ref() {
                    source_site_error(
                        diagnostics,
                        "CRV085",
                        "Clip transpose must be between -127st and 127st",
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV085",
                        "Clip transpose must be between -127st and 127st",
                        &clip.location,
                    );
                }
            }
            if let Some(groove) = clip.groove.as_deref()
                && !groove_ids.contains(groove)
            {
                if let Some(site) = clip.sites.groove.as_ref() {
                    source_site_error(
                        diagnostics,
                        "CRV152",
                        format!("Clip references missing Groove Template `{groove}`"),
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV152",
                        format!("Clip references missing Groove Template `{groove}`"),
                        &clip.location,
                    );
                }
            }
            if !clip.gain_db.is_finite() || !(-96.0..=24.0).contains(&clip.gain_db) {
                if let Some(site) = clip.sites.gain.as_ref() {
                    source_site_error(
                        diagnostics,
                        "CRV086",
                        "Clip gain must be between -96db and 24db",
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV086",
                        "Clip gain must be between -96db and 24db",
                        &clip.location,
                    );
                }
            }
            let fades_valid = clip.fade_in >= crate::time::MusicalTime::ZERO
                && clip.fade_out >= crate::time::MusicalTime::ZERO
                && clip
                    .fade_in
                    .checked_add(clip.fade_out)
                    .is_ok_and(|total| total <= clip.duration);
            if !fades_valid {
                let site = clip.sites.fade_in.as_ref().or(clip.sites.fade_out.as_ref());
                if let Some(site) = site {
                    source_site_error(
                        diagnostics,
                        "CRV087",
                        "Clip fades must be non-negative and cannot exceed Clip duration",
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV087",
                        "Clip fades must be non-negative and cannot exceed Clip duration",
                        &clip.location,
                    );
                }
            }
            if let Some((pattern, variation_transpose)) = resolved_pattern {
                instrument_clips.push((track.instrument.as_str(), clip, !pattern.locks.is_empty()));
                for lock in &pattern.locks {
                    let target = ParameterTarget::Instrument {
                        instrument: track.instrument.clone(),
                        parameter: lock.parameter.clone(),
                    };
                    let Some(spec) = project.parameter_spec(&target) else {
                        source_site_error(
                            diagnostics,
                            "CRV165",
                            format!("Parameter lock targets missing Parameter `{target}`"),
                            &lock.sites.parameter,
                        );
                        continue;
                    };
                    if !spec.automatable || !automation_target_is_runtime_safe(project, &target) {
                        source_site_error(
                            diagnostics,
                            "CRV166",
                            format!("Parameter `{target}` cannot be locked safely at runtime"),
                            &lock.sites.parameter,
                        );
                    }
                    match spec.unit.parse_value(&lock.value) {
                        Ok(value) if (spec.minimum..=spec.maximum).contains(&value) => {}
                        Ok(_) => source_site_error(
                            diagnostics,
                            "CRV167",
                            format!(
                                "Parameter lock value must be within {} … {}",
                                spec.unit.format_value(spec.minimum),
                                spec.unit.format_value(spec.maximum)
                            ),
                            &lock.sites.value,
                        ),
                        Err(error) => source_site_error(
                            diagnostics,
                            "CRV168",
                            format!("invalid Parameter lock value for {}: {error}", spec.unit),
                            &lock.sites.value,
                        ),
                    }
                }
                for event in &pattern.events {
                    if let PatternEvent::Note { pitch, .. } = event
                        && !(0..=127).contains(
                            &(i32::from(*pitch)
                                + variation_transpose
                                + i32::from(clip.transpose_semitones)),
                        )
                    {
                        source_site_error(
                            diagnostics,
                            "CRV088",
                            "Pattern Variation and Clip transpose move a note outside the MIDI pitch domain",
                            event.site(),
                        );
                    }
                }
            }
            if let (Some(instrument), Some((pattern, _))) = (instrument, resolved_pattern) {
                let has_notes = pattern
                    .events
                    .iter()
                    .any(|event| matches!(event, PatternEvent::Note { .. }));
                let has_hits = pattern
                    .events
                    .iter()
                    .any(|event| matches!(event, PatternEvent::Hit { .. }));
                let has_samples = pattern
                    .events
                    .iter()
                    .any(|event| matches!(event, PatternEvent::Sample { .. }));
                if instrument.kind == InstrumentKind::Graph && (has_hits || has_samples) {
                    located_error(
                        diagnostics,
                        "CRV079",
                        "graph instrument Track cannot consume percussion or sample Events",
                        &clip.location,
                    );
                }
                if instrument.kind == InstrumentKind::StockDrums && (has_notes || has_samples) {
                    located_error(
                        diagnostics,
                        "CRV080",
                        "stock_drums Track cannot consume pitched note or sample Events",
                        &clip.location,
                    );
                }
                if instrument.kind == InstrumentKind::Sampler {
                    if has_hits {
                        located_error(
                            diagnostics,
                            "CRV082",
                            "sampler Track cannot consume stock percussion hit Events",
                            &clip.location,
                        );
                    }
                    let mappings = instrument
                        .sampler
                        .as_ref()
                        .map(|sampler| {
                            sampler
                                .mappings
                                .iter()
                                .map(|mapping| mapping.id.as_str())
                                .collect::<BTreeSet<_>>()
                        })
                        .unwrap_or_default();
                    for event in &pattern.events {
                        if let PatternEvent::Sample { mapping, .. } = event
                            && !mappings.contains(mapping.as_str())
                        {
                            source_site_error(
                                diagnostics,
                                "CRV083",
                                format!("sample Event references missing mapping `{mapping}`"),
                                event.site(),
                            );
                        }
                    }
                }
            }
        }
    }
    for (index, (instrument, left, left_has_locks)) in instrument_clips.iter().enumerate() {
        let left_end = left.start.checked_add(left.duration).ok();
        for (right_instrument, right, right_has_locks) in &instrument_clips[index + 1..] {
            if instrument != right_instrument || !(*left_has_locks || *right_has_locks) {
                continue;
            }
            let right_end = right.start.checked_add(right.duration).ok();
            if left_end.is_some_and(|end| right.start < end)
                && right_end.is_some_and(|end| left.start < end)
            {
                located_error(
                    diagnostics,
                    "CRV169",
                    format!(
                        "overlapping Clips `{}` and `{}` share an Instrument with Parameter locks",
                        left.id, right.id
                    ),
                    &right.location,
                );
            }
        }
    }
    if project.song.tracks.is_empty() {
        diagnostics.push(Diagnostic::error(
            "CRV081",
            "Song must declare at least one Track",
        ));
    }
}

fn resolve_pattern_source<'a>(
    mut pattern: &'a Pattern,
    patterns: &BTreeMap<&str, &'a Pattern>,
) -> Option<(&'a Pattern, i32)> {
    let mut transpose = 0i32;
    let mut seen = BTreeSet::new();
    while let Some(variation) = &pattern.variation {
        if !seen.insert(pattern.id.as_str()) {
            return None;
        }
        transpose = transpose.checked_add(i32::from(variation.transpose_semitones))?;
        pattern = patterns.get(variation.source.as_str()).copied()?;
    }
    Some((pattern, transpose))
}
