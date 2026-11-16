//! Pattern, event, and Pattern Variation validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{Pattern, PatternEvent, Project},
};

use super::{located_error, source_site_error, valid_id};

pub(super) fn validate_patterns(project: &Project, diagnostics: &mut Diagnostics) {
    let patterns = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    for pattern in &project.song.patterns {
        if !valid_id(&pattern.id) || !ids.insert(pattern.id.as_str()) {
            located_error(
                diagnostics,
                "CRV060",
                format!("Pattern id `{}` is invalid or duplicated", pattern.id),
                &pattern.location,
            );
        }
        if !pattern.span.is_positive() {
            located_error(
                diagnostics,
                "CRV061",
                "Pattern span must be positive",
                &pattern.location,
            );
        }
        if let Some(variation) = &pattern.variation {
            let source = patterns.get(variation.source.as_str()).copied();
            if source.is_none() {
                source_site_error(
                    diagnostics,
                    "CRV153",
                    format!(
                        "Pattern Variation references missing Pattern `{}`",
                        variation.source
                    ),
                    &variation.sites.source,
                );
            }
            if source.is_some_and(|source| source.span != pattern.span) {
                source_site_error(
                    diagnostics,
                    "CRV154",
                    "Pattern Variation span must equal its source Pattern span",
                    &pattern.span_site,
                );
            }
            if variation.rotate < crate::time::MusicalTime::ZERO || variation.rotate >= pattern.span
            {
                source_site_error(
                    diagnostics,
                    "CRV155",
                    "Pattern Variation rotation must lie inside its half-open span",
                    &variation.sites.rotate,
                );
            }
            if !(-127..=127).contains(&variation.transpose_semitones) {
                source_site_error(
                    diagnostics,
                    "CRV156",
                    "Pattern Variation transpose must be between -127st and 127st",
                    &variation.sites.transpose,
                );
            }
            if !variation.velocity_scale.is_finite()
                || !(0.0..=2.0).contains(&variation.velocity_scale)
            {
                source_site_error(
                    diagnostics,
                    "CRV157",
                    "Pattern Variation velocity scale must be between 0 and 2",
                    &variation.sites.velocity,
                );
            }
            if !variation.probability.is_finite() || !(0.0..=1.0).contains(&variation.probability) {
                source_site_error(
                    diagnostics,
                    "CRV158",
                    "Pattern Variation probability must be between 0 and 1",
                    &variation.sites.probability,
                );
            }
            if !pattern.events.is_empty() || !pattern.locks.is_empty() {
                located_error(
                    diagnostics,
                    "CRV159",
                    "Pattern Variation cannot also contain flattened Events or Parameter locks",
                    &pattern.location,
                );
            }
            if pattern_variation_has_cycle(&pattern.id, &patterns) {
                located_error(
                    diagnostics,
                    "CRV160",
                    "Pattern Variation lineage contains a cycle",
                    &pattern.location,
                );
            }
        }
        let mut lock_ids = BTreeSet::new();
        let mut lock_positions = BTreeSet::new();
        for lock in &pattern.locks {
            if !valid_id(&lock.id) || !lock_ids.insert(lock.id.as_str()) {
                located_error(
                    diagnostics,
                    "CRV161",
                    "Parameter lock ID is invalid or duplicated in this Pattern",
                    &lock.location,
                );
            }
            if lock.at < crate::time::MusicalTime::ZERO || lock.at >= pattern.span {
                source_site_error(
                    diagnostics,
                    "CRV162",
                    "Parameter lock onset must lie inside the Pattern's half-open span",
                    &lock.sites.at,
                );
            }
            if !valid_id(&lock.parameter) {
                source_site_error(
                    diagnostics,
                    "CRV163",
                    "Parameter lock key must be source-safe",
                    &lock.sites.parameter,
                );
            }
            if !lock_positions.insert((lock.at, lock.parameter.as_str())) {
                located_error(
                    diagnostics,
                    "CRV164",
                    "Pattern cannot lock the same Parameter more than once at one onset",
                    &lock.location,
                );
            }
        }
        let mut sample_ids = BTreeSet::new();
        for event in &pattern.events {
            if event.at() < crate::time::MusicalTime::ZERO || event.at() >= pattern.span {
                source_site_error(
                    diagnostics,
                    "CRV062",
                    "Event onset must lie inside the Pattern's half-open span",
                    event.site(),
                );
            }
            match event {
                PatternEvent::Note {
                    duration, velocity, ..
                } => {
                    if !duration.is_positive() {
                        source_site_error(
                            diagnostics,
                            "CRV063",
                            "note duration must be positive",
                            event.site(),
                        );
                    }
                    validate_velocity(*velocity, event.site(), diagnostics);
                }
                PatternEvent::Hit { velocity, .. } => {
                    validate_velocity(*velocity, event.site(), diagnostics)
                }
                PatternEvent::Sample {
                    id,
                    duration,
                    start_frame,
                    end_frame,
                    gain_db,
                    pan,
                    fade_in_frames,
                    fade_out_frames,
                    velocity,
                    loop_region,
                    ..
                } => {
                    if !valid_id(id) || !sample_ids.insert(id.as_str()) {
                        source_site_error(
                            diagnostics,
                            "CRV065",
                            "sample occurrence ID is invalid or duplicated in this Pattern",
                            event.site(),
                        );
                    }
                    if !duration.is_positive() || start_frame >= end_frame {
                        source_site_error(
                            diagnostics,
                            "CRV066",
                            "sample occurrence requires positive duration and start < end",
                            event.site(),
                        );
                    }
                    if !gain_db.is_finite() || !(-96.0..=24.0).contains(gain_db) {
                        source_site_error(
                            diagnostics,
                            "CRV067",
                            "sample occurrence gain must be -96db..=24db",
                            event.site(),
                        );
                    }
                    if !pan.is_finite() || !(-1.0..=1.0).contains(pan) {
                        source_site_error(
                            diagnostics,
                            "CRV068",
                            "sample occurrence pan must be -1..=1",
                            event.site(),
                        );
                    }
                    let frames = end_frame.saturating_sub(*start_frame);
                    if u64::from(*fade_in_frames).saturating_add(u64::from(*fade_out_frames))
                        > frames
                    {
                        source_site_error(
                            diagnostics,
                            "CRV069",
                            "sample occurrence fades cannot exceed its source region",
                            event.site(),
                        );
                    }
                    if let Some(loop_region) = loop_region {
                        let loop_frames = loop_region
                            .end_frame
                            .saturating_sub(loop_region.start_frame);
                        if loop_region.start_frame < *start_frame
                            || loop_region.end_frame > *end_frame
                            || loop_region.start_frame >= loop_region.end_frame
                            || u64::from(loop_region.crossfade_frames).saturating_mul(2)
                                > loop_frames
                        {
                            source_site_error(
                                diagnostics,
                                "CRV070",
                                "sample occurrence loop must be inside its source region with a crossfade no longer than half the loop",
                                event.site(),
                            );
                        }
                    }
                    validate_velocity(*velocity, event.site(), diagnostics);
                }
            }
        }
    }
    if project.song.patterns.is_empty() {
        diagnostics.push(Diagnostic::error(
            "CRV064",
            "Song must declare at least one Pattern",
        ));
    }
}

fn pattern_variation_has_cycle(start: &str, patterns: &BTreeMap<&str, &Pattern>) -> bool {
    let mut seen = BTreeSet::new();
    let mut current = start;
    loop {
        if !seen.insert(current) {
            return true;
        }
        let Some(pattern) = patterns.get(current) else {
            return false;
        };
        let Some(variation) = &pattern.variation else {
            return false;
        };
        current = &variation.source;
    }
}

fn validate_velocity(velocity: f64, site: &crate::model::ValueSite, diagnostics: &mut Diagnostics) {
    if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
        source_site_error(
            diagnostics,
            "CRV090",
            "Event velocity must be between 0 and 1",
            site,
        );
    }
}
