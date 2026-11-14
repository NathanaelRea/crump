//! Instrument definitions and Composable DSP Graph validation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{
        GlideMode, Graph, GraphDomain, Instrument, InstrumentKind, MULTISAMPLE_ZONE_SYNTAX,
        NodeKind, NotePriority, PresentationItemKind, Project, RetriggerPolicy, SettingValue,
        StealPriority, Unit, VoicingMode, parse_pitch,
    },
    time::{Meter, parse_musical_time},
};

use super::{
    MAX_GRAPH_NODES, MAX_GRAPH_VOICES, MAX_INSTRUMENT_TAGS, MAX_MULTISAMPLE_ZONES,
    MAX_PRESENTATION_ITEMS, MAX_PRESENTATION_SECTIONS, MAX_SAMPLE_SLICES, MAX_SPECTRAL_PEAKS,
    MAX_SPECTRAL_SOURCES_PER_VOICE, MAX_STEREO_SUM_INPUTS, located_error, source_site_error,
    valid_id, valid_key,
};

/// Reuses the authoritative graph contract as a defensive compiler boundary.
pub(crate) fn validate_graph_for_compile(instrument: &Instrument, graph: &Graph) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    validate_graph(instrument, graph, &mut diagnostics);
    diagnostics
}

/// Validates standalone declarative Instrument Module contents.
#[must_use]
pub fn validate_instrument_module(instruments: &[Instrument]) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    validate_instruments(instruments, &mut diagnostics);
    if instruments.len() != 1 {
        diagnostics.push(Diagnostic::error(
            "CRV017",
            format!(
                "an Instrument Module must contain exactly one instrument; found {}",
                instruments.len()
            ),
        ));
    }
    diagnostics
}

pub(super) fn validate_instruments(instruments: &[Instrument], diagnostics: &mut Diagnostics) {
    let mut ids = BTreeSet::new();
    for instrument in instruments {
        if !valid_id(&instrument.id) {
            located_error(
                diagnostics,
                "CRV010",
                format!("invalid instrument id `{}`", instrument.id),
                &instrument.location,
            );
        }
        if !ids.insert(instrument.id.as_str()) {
            located_error(
                diagnostics,
                "CRV011",
                format!("duplicate instrument id `{}`", instrument.id),
                &instrument.location,
            );
        }
        if !(1..=64).contains(&instrument.polyphony) {
            located_error(
                diagnostics,
                "CRV012",
                "instrument polyphony must be between 1 and 64",
                &instrument.location,
            );
        }
        validate_voicing(instrument, diagnostics);
        validate_parameters(instrument, diagnostics);
        validate_faceplate(instrument, diagnostics);
        validate_presentation(instrument, diagnostics);
        match (instrument.kind, &instrument.graph) {
            (InstrumentKind::Graph, Some(graph)) if graph.domain == GraphDomain::Voice => {
                validate_graph(instrument, graph, diagnostics);
            }
            (InstrumentKind::Graph, Some(graph)) => located_error(
                diagnostics,
                "CRV058",
                "the primary graph of a graph Instrument must use the `voice` domain",
                &graph.location,
            ),
            (InstrumentKind::Graph, None) => located_error(
                diagnostics,
                "CRV013",
                "graph instrument is missing its Composable DSP Graph",
                &instrument.location,
            ),
            (InstrumentKind::StockDrums, Some(_)) => located_error(
                diagnostics,
                "CRV014",
                "stock_drums cannot also declare a graph",
                &instrument.location,
            ),
            (InstrumentKind::StockDrums, None) => {
                if instrument.sampler.is_some() {
                    located_error(
                        diagnostics,
                        "CRV014",
                        "stock_drums cannot declare sampler mappings",
                        &instrument.location,
                    );
                }
            }
            (InstrumentKind::Sampler, Some(_)) => located_error(
                diagnostics,
                "CRV014",
                "sampler cannot also declare a graph",
                &instrument.location,
            ),
            (InstrumentKind::Sampler, None) => match &instrument.sampler {
                Some(sampler) => validate_sampler(instrument, sampler, diagnostics),
                None => located_error(
                    diagnostics,
                    "CRV018",
                    "sampler instrument has no mapping definition",
                    &instrument.location,
                ),
            },
        }
        if let Some(shared) = &instrument.shared_graph {
            if instrument.kind != InstrumentKind::Graph {
                located_error(
                    diagnostics,
                    "CRV058",
                    "only graph Instruments may declare a shared post-voice graph",
                    &shared.location,
                );
            } else if shared.domain != GraphDomain::Shared {
                located_error(
                    diagnostics,
                    "CRV058",
                    "the shared post-voice graph must use the `shared` domain",
                    &shared.location,
                );
            } else {
                validate_graph(instrument, shared, diagnostics);
            }
        }
    }
    if instruments.is_empty() {
        diagnostics.push(Diagnostic::error(
            "CRV016",
            "Song must declare at least one instrument",
        ));
    }
}

fn validate_presentation(instrument: &Instrument, diagnostics: &mut Diagnostics) {
    let presentation = &instrument.presentation;
    if presentation.tags.len() > MAX_INSTRUMENT_TAGS {
        located_error(
            diagnostics,
            "CRV201",
            format!(
                "instrument declares {} tags; maximum is {MAX_INSTRUMENT_TAGS}",
                presentation.tags.len()
            ),
            &instrument.location,
        );
    }
    let mut tags = BTreeSet::new();
    for tag in &presentation.tags {
        if !valid_tag(tag) || !tags.insert(tag.as_str()) {
            located_error(
                diagnostics,
                "CRV201",
                format!("instrument tag `{tag}` is invalid or duplicated"),
                &instrument.location,
            );
        }
    }
    if let Some(preview) = &presentation.preview
        && (preview.low_pitch > preview.high_pitch
            || !preview.low_velocity.is_finite()
            || !preview.high_velocity.is_finite()
            || !(0.0..=preview.high_velocity).contains(&preview.low_velocity)
            || preview.high_velocity > 1.0
            || !preview.note_duration.is_positive()
            || preview.note_duration.as_f64() > 2.0)
    {
        located_error(
            diagnostics,
            "CRV201",
            "presentation preview requires ordered pitches, ordered 0..=1 velocities, and a positive duration no longer than 2qn",
            &preview.location,
        );
    }
    if presentation.sections.len() > MAX_PRESENTATION_SECTIONS {
        located_error(
            diagnostics,
            "CRV194",
            format!(
                "instrument declares {} presentation sections; maximum is {MAX_PRESENTATION_SECTIONS}",
                presentation.sections.len()
            ),
            &instrument.location,
        );
    }
    let item_count = presentation
        .sections
        .iter()
        .map(|section| section.items.len())
        .sum::<usize>();
    if item_count > MAX_PRESENTATION_ITEMS {
        located_error(
            diagnostics,
            "CRV194",
            format!(
                "instrument presentation declares {item_count} items; maximum is {MAX_PRESENTATION_ITEMS}"
            ),
            &instrument.location,
        );
    }

    let node_ids = instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
        .flat_map(|graph| graph.nodes.iter().map(|node| node.id.as_str()))
        .collect::<BTreeSet<_>>();
    let bus_ids = instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
        .flat_map(|graph| graph.connections.iter().map(|bus| bus.id.as_str()))
        .collect::<BTreeSet<_>>();
    let parameter_ids = instrument
        .parameters
        .iter()
        .map(|parameter| parameter.key.as_str())
        .collect::<BTreeSet<_>>();
    let source_ids = instrument
        .modulation_sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    let route_ids = instrument
        .modulation_routes
        .iter()
        .map(|route| route.id.as_str())
        .collect::<BTreeSet<_>>();
    let processor_ids = instrument
        .post_processors
        .iter()
        .map(|processor| processor.id.as_str())
        .collect::<BTreeSet<_>>();

    let mut section_ids = BTreeSet::new();
    for section in &presentation.sections {
        if !valid_id(&section.id) || !section_ids.insert(section.id.as_str()) {
            located_error(
                diagnostics,
                "CRV194",
                format!(
                    "presentation section ID `{}` is invalid or duplicated",
                    section.id
                ),
                &section.location,
            );
        }
        if section.name.trim().is_empty() {
            located_error(
                diagnostics,
                "CRV194",
                "presentation section name must not be empty",
                &section.location,
            );
        }
        let mut items = BTreeSet::new();
        for item in &section.items {
            if !items.insert((item.kind, item.id.as_str())) {
                source_site_error(
                    diagnostics,
                    "CRV194",
                    format!(
                        "presentation section `{}` repeats {:?} `{}`",
                        section.id, item.kind, item.id
                    ),
                    &item.id_site,
                );
                continue;
            }
            let exists = match item.kind {
                PresentationItemKind::Node => node_ids.contains(item.id.as_str()),
                PresentationItemKind::Bus => bus_ids.contains(item.id.as_str()),
                PresentationItemKind::Parameter => parameter_ids.contains(item.id.as_str()),
                PresentationItemKind::Source => source_ids.contains(item.id.as_str()),
                PresentationItemKind::Route => route_ids.contains(item.id.as_str()),
                PresentationItemKind::Processor => processor_ids.contains(item.id.as_str()),
                // Project-owned asset references are checked by whole-Project validation;
                // standalone Instrument Modules may resolve them at import time.
                PresentationItemKind::Asset => true,
            };
            if !exists {
                source_site_error(
                    diagnostics,
                    "CRV194",
                    format!(
                        "presentation section `{}` references missing {:?} `{}`",
                        section.id, item.kind, item.id
                    ),
                    &item.id_site,
                );
            }
        }
    }
}

fn valid_tag(tag: &str) -> bool {
    let mut characters = tag.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
}

fn validate_voicing(instrument: &Instrument, diagnostics: &mut Diagnostics) {
    let voicing = &instrument.voicing;
    if instrument.kind != InstrumentKind::Graph && !voicing.is_legacy_default() {
        located_error(
            diagnostics,
            "CRV193",
            "only graph Instruments may declare non-default voicing",
            &instrument.location,
        );
    }
    if !voicing.glide_seconds.is_finite() || !(0.0..=4.0).contains(&voicing.glide_seconds) {
        if let Some(site) = &voicing.glide_site {
            source_site_error(
                diagnostics,
                "CRV193",
                "voicing glide must be finite and between 0s and 4s",
                site,
            );
        } else {
            located_error(
                diagnostics,
                "CRV193",
                "voicing glide must be finite and between 0s and 4s",
                &instrument.location,
            );
        }
    }
    if voicing.glide_mode == GlideMode::Off && voicing.glide_seconds != 0.0 {
        located_error(
            diagnostics,
            "CRV193",
            "voicing glide_mode off requires a zero glide duration",
            &instrument.location,
        );
    }
    match voicing.mode {
        VoicingMode::Mono => {
            if instrument.polyphony != 1 {
                located_error(
                    diagnostics,
                    "CRV193",
                    "mono voicing requires instrument polyphony 1",
                    &instrument.location,
                );
            }
            if voicing.steal_priority != StealPriority::ReleaseOldest {
                located_error(
                    diagnostics,
                    "CRV193",
                    "mono voicing does not use steal_priority; keep release_oldest",
                    &instrument.location,
                );
            }
        }
        VoicingMode::Poly => {
            if voicing.retrigger != RetriggerPolicy::Always {
                located_error(
                    diagnostics,
                    "CRV193",
                    "poly voicing requires retrigger always",
                    &instrument.location,
                );
            }
            if voicing.note_priority != NotePriority::Last {
                located_error(
                    diagnostics,
                    "CRV193",
                    "poly voicing does not use note_priority; keep last",
                    &instrument.location,
                );
            }
        }
    }
}

pub(super) fn validate_runtime_graph_voices(project: &Project, diagnostics: &mut Diagnostics) {
    let instruments: BTreeMap<_, _> = project
        .song
        .instruments
        .iter()
        .map(|instrument| (instrument.id.as_str(), instrument))
        .collect();
    let total_graph_voices = project.song.tracks.iter().fold(0_u32, |total, track| {
        let voices = instruments
            .get(track.instrument.as_str())
            .filter(|instrument| instrument.kind == InstrumentKind::Graph)
            .map_or(0, |instrument| u32::from(instrument.polyphony));
        total.saturating_add(voices)
    });
    if total_graph_voices > MAX_GRAPH_VOICES {
        diagnostics.push(Diagnostic::error(
            "CRV015",
            format!(
                "Project instantiates {total_graph_voices} graph voices across Tracks; maximum is {MAX_GRAPH_VOICES}"
            ),
        ));
    }
}

fn validate_parameters(instrument: &Instrument, diagnostics: &mut Diagnostics) {
    let mut keys = BTreeSet::new();
    for parameter in &instrument.parameters {
        if !valid_key(&parameter.key) || !keys.insert(parameter.key.as_str()) {
            located_error(
                diagnostics,
                "CRV020",
                format!("Parameter key `{}` is invalid or duplicated", parameter.key),
                &parameter.location,
            );
        }
        if ![
            parameter.minimum,
            parameter.maximum,
            parameter.default,
            parameter.value,
        ]
        .into_iter()
        .all(f64::is_finite)
        {
            located_error(
                diagnostics,
                "CRV021",
                "Parameter values must be finite",
                &parameter.location,
            );
            continue;
        }
        if parameter.minimum >= parameter.maximum {
            located_error(
                diagnostics,
                "CRV022",
                "Parameter minimum must be less than maximum",
                &parameter.location,
            );
        }
        if !(parameter.minimum..=parameter.maximum).contains(&parameter.default) {
            located_error(
                diagnostics,
                "CRV023",
                "Parameter default is outside its legal domain",
                &parameter.location,
            );
        }
        if !(parameter.minimum..=parameter.maximum).contains(&parameter.value) {
            located_error(
                diagnostics,
                "CRV024",
                "authored Parameter value is outside its legal domain",
                &parameter.location,
            );
        }
        if instrument.kind == InstrumentKind::StockDrums {
            if !matches!(parameter.key.as_str(), "tone" | "punch") {
                located_error(
                    diagnostics,
                    "CRV025",
                    format!(
                        "stock_drums Parameter `{}` has no native sound mapping; use `tone` or `punch`",
                        parameter.key
                    ),
                    &parameter.location,
                );
            }
            if parameter.unit != Unit::Ratio || parameter.minimum < 0.0 || parameter.maximum > 1.0 {
                located_error(
                    diagnostics,
                    "CRV026",
                    format!(
                        "stock_drums Parameter `{}` must use a ratio domain within 0..=1",
                        parameter.key
                    ),
                    &parameter.location,
                );
            }
        } else if instrument.kind == InstrumentKind::Sampler {
            let valid = match parameter.key.as_str() {
                "gain" => {
                    parameter.unit == Unit::Decibels
                        && parameter.minimum >= -96.0
                        && parameter.maximum <= 24.0
                }
                "tune" => {
                    parameter.unit == Unit::Semitones
                        && parameter.minimum >= -48.0
                        && parameter.maximum <= 48.0
                }
                "pan" => {
                    parameter.unit == Unit::Pan
                        && parameter.minimum >= -1.0
                        && parameter.maximum <= 1.0
                }
                "attack" | "release" => {
                    parameter.unit == Unit::Seconds
                        && parameter.minimum >= 0.0
                        && parameter.maximum <= 8.0
                }
                _ => false,
            };
            if !valid {
                located_error(
                    diagnostics,
                    "CRV027",
                    format!(
                        "sampler Parameter `{}` must be gain(db), tune(semitones), pan, attack(seconds), or release(seconds) in its supported domain",
                        parameter.key
                    ),
                    &parameter.location,
                );
            }
        }
    }
}

fn validate_sampler(
    instrument: &Instrument,
    sampler: &crate::model::SamplerDefinition,
    diagnostics: &mut Diagnostics,
) {
    const MAX_MAPPINGS: usize = 256;
    if sampler.mappings.is_empty() || sampler.mappings.len() > MAX_MAPPINGS {
        located_error(
            diagnostics,
            "CRV018",
            format!("sampler must declare between 1 and {MAX_MAPPINGS} mappings"),
            &sampler.location,
        );
    }
    let mut ids = BTreeSet::new();
    let mut occupied_keys = [false; 128];
    for mapping in &sampler.mappings {
        if !valid_id(&mapping.id) || !ids.insert(mapping.id.as_str()) {
            located_error(
                diagnostics,
                "CRV019",
                format!(
                    "sample mapping id `{}` is invalid or duplicated",
                    mapping.id
                ),
                &mapping.location,
            );
        }
        if !valid_id(&mapping.asset) {
            located_error(
                diagnostics,
                "CRV019",
                format!(
                    "sample mapping `{}` has invalid asset id `{}`",
                    mapping.id, mapping.asset
                ),
                &mapping.location,
            );
        }
        if mapping.low_key > mapping.high_key
            || !(mapping.low_key..=mapping.high_key).contains(&mapping.root_key)
        {
            located_error(
                diagnostics,
                "CRV019",
                "sample mapping requires LOW <= ROOT <= HIGH",
                &mapping.location,
            );
        } else {
            for key in mapping.low_key..=mapping.high_key {
                if std::mem::replace(&mut occupied_keys[usize::from(key)], true) {
                    located_error(
                        diagnostics,
                        "CRV019",
                        format!(
                            "sampler `{}` has overlapping mapping key {key}",
                            instrument.id
                        ),
                        &mapping.location,
                    );
                    break;
                }
            }
        }
        let region_frames = mapping.end_frame.saturating_sub(mapping.start_frame);
        if mapping.start_frame >= mapping.end_frame {
            source_site_error(
                diagnostics,
                "CRV019",
                "sample mapping region must have start < end",
                &mapping.start_site,
            );
        }
        if !mapping.gain_db.is_finite() || !(-96.0..=24.0).contains(&mapping.gain_db) {
            source_site_error(
                diagnostics,
                "CRV019",
                "sample mapping gain must be -96db..=24db",
                &mapping.gain_site,
            );
        }
        if !mapping.pan.is_finite() || !(-1.0..=1.0).contains(&mapping.pan) {
            source_site_error(
                diagnostics,
                "CRV019",
                "sample mapping pan must be -1..=1",
                &mapping.pan_site,
            );
        }
        if !mapping.attack_seconds.is_finite()
            || !mapping.release_seconds.is_finite()
            || !(0.0..=8.0).contains(&mapping.attack_seconds)
            || !(0.0..=8.0).contains(&mapping.release_seconds)
        {
            located_error(
                diagnostics,
                "CRV019",
                "sample envelope attack/release must be 0s..=8s",
                &mapping.location,
            );
        }
        if u64::from(mapping.fade_in_frames).saturating_add(u64::from(mapping.fade_out_frames))
            > region_frames
        {
            source_site_error(
                diagnostics,
                "CRV019",
                "sample mapping fades cannot exceed its region",
                &mapping.fade_out_site,
            );
        }
        if let Some(loop_region) = mapping.loop_region {
            let loop_frames = loop_region
                .end_frame
                .saturating_sub(loop_region.start_frame);
            if loop_region.start_frame < mapping.start_frame
                || loop_region.end_frame > mapping.end_frame
                || loop_region.start_frame >= loop_region.end_frame
                || u64::from(loop_region.crossfade_frames).saturating_mul(2) > loop_frames
            {
                if let Some(site) = &mapping.loop_site {
                    source_site_error(
                        diagnostics,
                        "CRV019",
                        "sample loop must be inside the mapping region with a crossfade no longer than half the loop",
                        site,
                    );
                } else {
                    located_error(
                        diagnostics,
                        "CRV019",
                        "sample loop must be inside the mapping region with a crossfade no longer than half the loop",
                        &mapping.location,
                    );
                }
            }
            if mapping.playback != crate::model::SamplePlayback::Gated {
                located_error(
                    diagnostics,
                    "CRV019",
                    "a looped sample mapping must use gated playback so Note Off can stop it",
                    &mapping.location,
                );
            }
        }
        if mapping
            .choke_group
            .as_deref()
            .is_some_and(|group| !valid_id(group))
        {
            located_error(
                diagnostics,
                "CRV019",
                "sample choke group must be a source-safe ID",
                &mapping.location,
            );
        }
    }
}

fn validate_faceplate(instrument: &Instrument, diagnostics: &mut Diagnostics) {
    if instrument.faceplate.is_empty() || instrument.faceplate.len() > 8 {
        located_error(
            diagnostics,
            "CRV030",
            "an Instrument Faceplate must expose between 1 and 8 meaningful controls",
            &instrument.location,
        );
    }
    let parameters: BTreeSet<_> = instrument
        .parameters
        .iter()
        .map(|parameter| parameter.key.as_str())
        .collect();
    let mut keys = BTreeSet::new();
    for control in &instrument.faceplate {
        if !valid_key(&control.key) || !keys.insert(control.key.as_str()) {
            located_error(
                diagnostics,
                "CRV031",
                format!(
                    "Faceplate control key `{}` is invalid or duplicated",
                    control.key
                ),
                &control.location,
            );
        }
        if !parameters.contains(control.parameter.as_str()) {
            located_error(
                diagnostics,
                "CRV032",
                format!(
                    "Faceplate control maps missing Parameter `{}`",
                    control.parameter
                ),
                &control.location,
            );
        } else {
            let has_sound_mapping = match instrument.kind {
                InstrumentKind::Graph => instrument.graph.as_ref().is_some_and(|graph| {
                    let mut pending = vec![control.parameter.as_str()];
                    let mut seen = BTreeSet::new();
                    let mut audible = false;
                    while let Some(parameter) = pending.pop() {
                        if !seen.insert(parameter) {
                            continue;
                        }
                        if std::iter::once(graph)
                            .chain(instrument.shared_graph.iter())
                            .any(|graph| {
                                graph.nodes.iter().any(|node| node.references_parameter(parameter))
                            })
                        {
                            audible = true;
                            break;
                        }
                        for route in &instrument.modulation_routes {
                            let source_matches = instrument.modulation_sources.iter().any(|source| {
                                source.id == route.source
                                    && matches!(&source.kind, crate::model::InstrumentModulationSourceKind::Parameter { parameter: source_parameter } if source_parameter == parameter)
                            });
                            let auxiliary_matches = route.auxiliary.as_ref().is_some_and(|auxiliary| {
                                instrument.modulation_sources.iter().any(|source| {
                                    source.id == *auxiliary
                                        && matches!(&source.kind, crate::model::InstrumentModulationSourceKind::Parameter { parameter: source_parameter } if source_parameter == parameter)
                                })
                            });
                            if source_matches || auxiliary_matches {
                                pending.push(route.target.as_str());
                            }
                        }
                    }
                    audible
                }),
                InstrumentKind::StockDrums => {
                    matches!(control.parameter.as_str(), "tone" | "punch")
                }
                InstrumentKind::Sampler => matches!(
                    control.parameter.as_str(),
                    "gain" | "tune" | "pan" | "attack" | "release"
                ),
            };
            if !has_sound_mapping {
                located_error(
                    diagnostics,
                    "CRV034",
                    format!(
                        "Faceplate control `{}` does not map to an audible Instrument setting",
                        control.key
                    ),
                    &control.location,
                );
            }
        }
        if control.label.trim().is_empty() || control.explanation.trim().is_empty() {
            located_error(
                diagnostics,
                "CRV033",
                "Faceplate controls require a label and contextual explanation",
                &control.location,
            );
        }
    }
}

const fn node_kind_has_runtime(_kind: NodeKind) -> bool {
    true
}

fn validate_graph(instrument: &Instrument, graph: &Graph, diagnostics: &mut Diagnostics) {
    if graph.max_nodes == 0 || graph.max_nodes > MAX_GRAPH_NODES {
        located_error(
            diagnostics,
            "CRV040",
            format!("graph max_nodes must be between 1 and {MAX_GRAPH_NODES}"),
            &graph.location,
        );
    }
    if graph.nodes.len() > usize::from(graph.max_nodes) {
        located_error(
            diagnostics,
            "CRV041",
            "graph contains more nodes than its declared max_nodes bound",
            &graph.location,
        );
    }
    let spectral_sources = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Spectral)
        .count();
    if graph.domain == GraphDomain::Voice && spectral_sources > MAX_SPECTRAL_SOURCES_PER_VOICE {
        located_error(
            diagnostics,
            "CRV200",
            format!(
                "voice graph contains {spectral_sources} spectral sources; at most {MAX_SPECTRAL_SOURCES_PER_VOICE} are admitted"
            ),
            &graph.location,
        );
    }
    let parameters: BTreeMap<_, _> = instrument
        .parameters
        .iter()
        .map(|parameter| (parameter.key.as_str(), parameter))
        .collect();
    let mut nodes = BTreeMap::new();
    let mut outputs = 0;
    for node in &graph.nodes {
        if !valid_id(&node.id) || nodes.insert(node.id.as_str(), node).is_some() {
            located_error(
                diagnostics,
                "CRV042",
                format!("graph node id `{}` is invalid or duplicated", node.id),
                &node.location,
            );
        }
        if node.kind == NodeKind::Output {
            outputs += 1;
        }
        if node_kind_has_runtime(node.kind) {
            if node.kind == NodeKind::AnalogOscillator {
                validate_analog_directives(node, &parameters, diagnostics);
            } else if node.kind == NodeKind::Wavetable {
                validate_wavetable_directives(node, &parameters, diagnostics);
            } else if node.kind == NodeKind::SampleSource {
                validate_sample_source_directives(node, &parameters, diagnostics);
            } else if node.kind == NodeKind::Multisample {
                validate_multisample_directives(node, diagnostics);
            } else if node.kind == NodeKind::Spectral {
                validate_spectral_settings(node, &parameters, diagnostics);
            } else if node.kind == NodeKind::Lfo {
                validate_lfo_directives(node, diagnostics);
            } else if node.kind == NodeKind::Shaper {
                validate_shaper_directives(node, diagnostics);
            } else if node.kind == NodeKind::Envelope {
                validate_envelope_mode(node, diagnostics);
            } else if !node.directives.is_empty() {
                located_error(
                    diagnostics,
                    "CRV050",
                    format!(
                        "{} node does not support structured directives",
                        node_kind_name(node.kind)
                    ),
                    &node.location,
                );
            }
            validate_node_settings(node, &parameters, diagnostics);
        } else {
            located_error(
                diagnostics,
                "CRV059",
                format!(
                    "{} source is reserved in source version 2 but has no runtime implementation",
                    node_kind_name(node.kind)
                ),
                &node.location,
            );
        }
    }
    if outputs != 1 {
        located_error(
            diagnostics,
            "CRV043",
            format!("graph must contain exactly one output node; found {outputs}"),
            &graph.location,
        );
    }

    let mut bus_ids = BTreeSet::new();
    let mut input_counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut voice_sum_connections = 0_usize;
    // Same-frame dependencies. A Bus into a feedback-breaking Node is consumed after the
    // frame, so it never orders Nodes and may close a loop.
    let mut adjacency: BTreeMap<&str, Vec<&str>> =
        nodes.keys().map(|id| (*id, Vec::new())).collect();
    let breaks_feedback = |id: &str| {
        nodes
            .get(id)
            .is_some_and(|node| node.kind.breaks_feedback())
    };
    let mut reverse: BTreeMap<&str, Vec<&str>> = nodes.keys().map(|id| (*id, Vec::new())).collect();
    for connection in &graph.connections {
        let duplicate = !bus_ids.insert(connection.id.as_str());
        if let Some(site) = &connection.id_site
            && (!valid_id(&connection.id) || duplicate)
        {
            source_site_error(
                diagnostics,
                "CRV060",
                format!("graph Bus id `{}` is invalid or duplicated", connection.id),
                site,
            );
        } else if duplicate {
            located_error(
                diagnostics,
                "CRV060",
                "generated graph connection identity unexpectedly collided",
                &connection.location,
            );
        }
        if connection.from.node == "voice_sum" && connection.from.port == "audio" {
            voice_sum_connections = voice_sum_connections.saturating_add(1);
        }
        let from_type = output_type(
            graph.domain,
            &connection.from.node,
            &connection.from.port,
            &nodes,
        );
        let to_type = input_type(&connection.to.node, &connection.to.port, &nodes);
        match (from_type, to_type) {
            (Ok(from), Ok(to)) if signal_compatible(from, to) => {}
            (Ok(from), Ok(to)) => located_error(
                diagnostics,
                "CRV044",
                format!(
                    "cannot connect {} ({}) to {} ({}) without an explicit conversion",
                    connection.from,
                    from.label(),
                    connection.to,
                    to.label()
                ),
                &connection.location,
            ),
            (Err(message), _) | (_, Err(message)) => {
                located_error(diagnostics, "CRV045", message, &connection.location);
                continue;
            }
        }
        let count = input_counts
            .entry((connection.to.node.as_str(), connection.to.port.as_str()))
            .or_default();
        *count += 1;
        let summing_limit = nodes
            .get(connection.to.node.as_str())
            .filter(|node| node.kind.is_summing() && connection.to.port == "inputs")
            .map(|node| match node.setting("max_inputs") {
                Some(SettingValue::Number(value)) if value.is_finite() => {
                    (*value as usize).clamp(1, MAX_STEREO_SUM_INPUTS)
                }
                _ => MAX_STEREO_SUM_INPUTS,
            });
        if summing_limit.is_none() && *count > 1 {
            located_error(
                diagnostics,
                "CRV046",
                format!(
                    "single-cardinality input {} has more than one Bus",
                    connection.to
                ),
                &connection.location,
            );
        } else if summing_limit.is_some_and(|limit| *count > limit) {
            located_error(
                diagnostics,
                "CRV061",
                format!(
                    "summing input {} exceeds its {}-Bus bound",
                    connection.to,
                    summing_limit.unwrap_or(MAX_STEREO_SUM_INPUTS)
                ),
                &connection.location,
            );
        }
        if !matches!(connection.from.node.as_str(), "note" | "voice_sum") {
            if !breaks_feedback(connection.to.node.as_str())
                && let Some(edges) = adjacency.get_mut(connection.from.node.as_str())
            {
                edges.push(connection.to.node.as_str());
            }
            if let Some(edges) = reverse.get_mut(connection.to.node.as_str()) {
                edges.push(connection.from.node.as_str());
            }
        }
    }

    if graph.domain == GraphDomain::Shared && voice_sum_connections != 1 {
        located_error(
            diagnostics,
            "CRV062",
            format!(
                "shared graph must contain exactly one Bus from `voice_sum.audio`; found {voice_sum_connections}"
            ),
            &graph.location,
        );
    }

    // Same-frame graph-Control routes are per-voice dependency edges: their source Node must
    // execute before every voice Node consuming the destination Parameter. Shared graphs omit
    // every per-voice route and therefore must not inherit these graph-local edges.
    if graph.domain == GraphDomain::Voice {
        for route in &instrument.modulation_routes {
            let source_nodes = std::iter::once(route.source.as_str())
                .chain(route.auxiliary.as_deref())
                .filter_map(|source_id| {
                    instrument
                        .modulation_sources
                        .iter()
                        .find(|source| source.id == source_id)
                })
                .filter_map(|source| match &source.kind {
                    crate::model::InstrumentModulationSourceKind::GraphPort { node, .. } => {
                        Some(node.as_str())
                    }
                    _ => None,
                });
            for source_node in source_nodes {
                for consumer in graph
                    .nodes
                    .iter()
                    .filter(|node| node.references_parameter(&route.target))
                {
                    if !consumer.kind.breaks_feedback()
                        && let Some(edges) = adjacency.get_mut(source_node)
                    {
                        edges.push(consumer.id.as_str());
                    }
                    if let Some(edges) = reverse.get_mut(consumer.id.as_str()) {
                        edges.push(source_node);
                    }
                }
            }
        }
    }

    for node in &graph.nodes {
        for required in required_inputs(node.kind) {
            if input_counts
                .get(&(node.id.as_str(), *required))
                .copied()
                .unwrap_or(0)
                == 0
            {
                located_error(
                    diagnostics,
                    "CRV047",
                    format!("node `{}` requires input `{required}`", node.id),
                    &node.location,
                );
            }
        }
    }
    validate_primitive_inputs(graph, &parameters, &input_counts, diagnostics);
    if let Some(cycle) = find_cycle(&adjacency) {
        located_error(
            diagnostics,
            "CRV048",
            format!(
                "graph contains a zero-latency routing cycle `{}`; close feedback loops through a `delay_line` input",
                cycle.join(" -> ")
            ),
            &graph.location,
        );
    }
    if let Some(output) = graph
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Output)
    {
        let reachable = reverse_reachable(output.id.as_str(), &reverse);
        for node in &graph.nodes {
            if !reachable.contains(node.id.as_str()) {
                located_error(
                    diagnostics,
                    "CRV049",
                    format!("node `{}` cannot contribute to the graph output", node.id),
                    &node.location,
                );
            }
        }
    }
}

fn validate_node_settings(
    node: &crate::model::GraphNode,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    diagnostics: &mut Diagnostics,
) {
    let schema = setting_schema(node.kind);
    let mut keys = BTreeSet::new();
    for setting in &node.settings {
        let key = setting.key.as_str();
        let value = &setting.value;
        if !keys.insert(key) {
            source_site_error(
                diagnostics,
                "CRV050",
                format!(
                    "duplicate `{key}` setting for {} node",
                    node_kind_name(node.kind)
                ),
                &setting.key_site,
            );
            continue;
        }
        let Some(expected) = schema
            .iter()
            .find(|spec| spec.key == key)
            .map(|spec| spec.kind)
        else {
            source_site_error(
                diagnostics,
                "CRV050",
                format!(
                    "unknown `{key}` setting for {} node",
                    node_kind_name(node.kind)
                ),
                &setting.key_site,
            );
            continue;
        };
        match (expected, value) {
            (SettingKind::Identifier, SettingValue::Text(text)) if valid_id(text) => {}
            (SettingKind::Identifier, _) => source_site_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be a stable identifier"),
                &setting.value_site,
            ),
            (SettingKind::Pitch, SettingValue::Text(text)) if parse_pitch(text).is_ok() => {}
            (SettingKind::Pitch, _) => source_site_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be a scientific pitch from C-1 to G9"),
                &setting.value_site,
            ),
            (SettingKind::Text(allowed), SettingValue::Text(text))
                if allowed.contains(&text.as_str()) => {}
            (SettingKind::Text(allowed), _) => source_site_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be one of {}", allowed.join(", ")),
                &setting.value_site,
            ),
            (SettingKind::Integer { minimum, maximum }, SettingValue::Number(number))
                if number.is_finite()
                    && number.fract() == 0.0
                    && *number >= f64::from(minimum)
                    && *number <= f64::from(maximum) => {}
            (SettingKind::Integer { minimum, maximum }, _) => source_site_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be an integer from {minimum} to {maximum}"),
                &setting.value_site,
            ),
            (SettingKind::LfoRate, SettingValue::Parameter(parameter_key)) => {
                match parameters.get(parameter_key.as_str()) {
                    Some(parameter) if parameter.unit == Unit::Hertz => {
                        validate_setting_domain(
                            node,
                            key,
                            parameter.value,
                            Some((parameter.minimum, parameter.maximum, &parameter.key)),
                            &setting.value_site,
                            diagnostics,
                        );
                    }
                    Some(parameter) => source_site_error(
                        diagnostics,
                        "CRV052",
                        format!(
                            "setting `{key}` expects hertz or synchronized musical time, but Parameter `{}` uses {}",
                            parameter.key, parameter.unit
                        ),
                        &setting.value_site,
                    ),
                    None => source_site_error(
                        diagnostics,
                        "CRV053",
                        format!("setting `{key}` references missing Parameter `{parameter_key}`"),
                        &setting.value_site,
                    ),
                }
            }
            (SettingKind::LfoRate, SettingValue::Text(text)) => {
                if let Ok(number) = Unit::Hertz.parse_value(text) {
                    validate_setting_domain(
                        node,
                        key,
                        number,
                        None,
                        &setting.value_site,
                        diagnostics,
                    );
                } else if parse_musical_time(text, Meter::default()).is_err() {
                    source_site_error(
                        diagnostics,
                        "CRV056",
                        "LFO rate requires finite hertz or synchronized musical time",
                        &setting.value_site,
                    );
                }
            }
            (SettingKind::LfoRate, _) => source_site_error(
                diagnostics,
                "CRV056",
                "LFO rate requires explicit hertz or synchronized musical time",
                &setting.value_site,
            ),
            (SettingKind::Duration, SettingValue::Parameter(parameter_key)) => {
                match parameters.get(parameter_key.as_str()) {
                    Some(parameter) if parameter.unit == Unit::Seconds => {
                        validate_setting_domain(
                            node,
                            key,
                            parameter.value,
                            Some((parameter.minimum, parameter.maximum, &parameter.key)),
                            &setting.value_site,
                            diagnostics,
                        );
                    }
                    Some(parameter) => source_site_error(
                        diagnostics,
                        "CRV052",
                        format!(
                            "setting `{key}` expects seconds or synchronized musical time, but Parameter `{}` uses {}",
                            parameter.key, parameter.unit
                        ),
                        &setting.value_site,
                    ),
                    None => source_site_error(
                        diagnostics,
                        "CRV053",
                        format!("setting `{key}` references missing Parameter `{parameter_key}`"),
                        &setting.value_site,
                    ),
                }
            }
            (SettingKind::Duration, SettingValue::Text(text)) => {
                if let Ok(number) = Unit::Seconds.parse_value(text) {
                    validate_setting_domain(
                        node,
                        key,
                        number,
                        None,
                        &setting.value_site,
                        diagnostics,
                    );
                } else if parse_musical_time(text, Meter::default()).is_err() {
                    source_site_error(
                        diagnostics,
                        "CRV056",
                        format!(
                            "setting `{key}` requires finite seconds or synchronized musical time"
                        ),
                        &setting.value_site,
                    );
                }
            }
            (SettingKind::Duration, _) => source_site_error(
                diagnostics,
                "CRV056",
                format!("setting `{key}` requires explicit seconds or synchronized musical time"),
                &setting.value_site,
            ),
            (SettingKind::Number(unit), SettingValue::Parameter(parameter_key)) => {
                match parameters.get(parameter_key.as_str()) {
                    Some(parameter) if parameter.unit == unit => {
                        validate_setting_domain(
                            node,
                            key,
                            parameter.value,
                            Some((parameter.minimum, parameter.maximum, &parameter.key)),
                            &setting.value_site,
                            diagnostics,
                        );
                    }
                    Some(parameter) => source_site_error(
                        diagnostics,
                        "CRV052",
                        format!(
                            "setting `{key}` expects {unit}, but Parameter `{}` uses {}",
                            parameter.key, parameter.unit
                        ),
                        &setting.value_site,
                    ),
                    None => source_site_error(
                        diagnostics,
                        "CRV053",
                        format!("setting `{key}` references missing Parameter `{parameter_key}`"),
                        &setting.value_site,
                    ),
                }
            }
            (SettingKind::Number(unit), SettingValue::Number(number)) => {
                if !number.is_finite() {
                    source_site_error(
                        diagnostics,
                        "CRV054",
                        "node setting must be finite",
                        &setting.value_site,
                    );
                } else if !matches!(unit, Unit::Ratio | Unit::Pan | Unit::Semitones) {
                    source_site_error(
                        diagnostics,
                        "CRV056",
                        format!("setting `{key}` requires an explicit finite {unit} unit suffix"),
                        &setting.value_site,
                    );
                } else {
                    validate_setting_domain(
                        node,
                        key,
                        *number,
                        None,
                        &setting.value_site,
                        diagnostics,
                    );
                }
            }
            (SettingKind::Number(unit), SettingValue::Text(text)) => match unit.parse_value(text) {
                Ok(number) => validate_setting_domain(
                    node,
                    key,
                    number,
                    None,
                    &setting.value_site,
                    diagnostics,
                ),
                Err(_) => source_site_error(
                    diagnostics,
                    "CRV056",
                    format!("setting `{key}` requires a finite {unit} value"),
                    &setting.value_site,
                ),
            },
        }
    }
    if node.kind == NodeKind::StereoFilter
        && matches!(node.setting("model"), Some(SettingValue::Text(model)) if model == "ladder_drive")
        && matches!(node.setting("mode"), Some(SettingValue::Text(mode)) if mode != "lowpass")
    {
        let site = node
            .settings
            .iter()
            .find(|setting| setting.key == "mode")
            .map(|setting| &setting.value_site);
        if let Some(site) = site {
            source_site_error(
                diagnostics,
                "CRV056",
                "ladder_drive currently supports only lowpass mode; use svf_clean for highpass, bandpass, or notch",
                site,
            );
        } else {
            located_error(
                diagnostics,
                "CRV056",
                "ladder_drive currently supports only lowpass mode",
                &node.location,
            );
        }
    }
}

/// Port and setting combinations of the generic primitives that per-key validation cannot see.
fn validate_primitive_inputs(
    graph: &Graph,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    input_counts: &BTreeMap<(&str, &str), usize>,
    diagnostics: &mut Diagnostics,
) {
    let text = |node: &crate::model::GraphNode, key: &str, default: &'static str| match node
        .setting(key)
    {
        Some(SettingValue::Text(value)) => value.clone(),
        _ => default.to_owned(),
    };
    let connected = |node: &crate::model::GraphNode, port: &str| {
        input_counts
            .get(&(node.id.as_str(), port))
            .copied()
            .unwrap_or(0)
            > 0
    };
    let mut error = |node: &crate::model::GraphNode, message: String| {
        located_error(diagnostics, "CRV220", message, &node.location);
    };
    for node in &graph.nodes {
        match node.kind {
            NodeKind::DelayLine => {
                if matches!(node.setting("max_time"), Some(SettingValue::Parameter(_))) {
                    error(
                        node,
                        format!(
                            "delay_line `{}` max_time must be a literal buffer size; Parameters cannot resize its allocation",
                            node.id
                        ),
                    );
                }
                let pitch_mode = text(node, "mode", "time") == "pitch";
                if pitch_mode && !connected(node, "pitch") {
                    error(
                        node,
                        format!(
                            "delay_line `{}` in pitch mode requires input `pitch`",
                            node.id
                        ),
                    );
                }
                if !pitch_mode && connected(node, "pitch") {
                    error(
                        node,
                        format!(
                            "delay_line `{}` reads `pitch` only in pitch mode; set `mode pitch`",
                            node.id
                        ),
                    );
                }
                for (key, used) in [("time", !pitch_mode), ("ratio", pitch_mode)] {
                    if !used && node.setting(key).is_some() {
                        error(
                            node,
                            format!(
                                "delay_line `{}` setting `{key}` applies only in {} mode",
                                node.id,
                                if pitch_mode { "time" } else { "pitch" }
                            ),
                        );
                    }
                }
                let seconds = |key: &str, default: f64| match node.setting(key) {
                    Some(SettingValue::Text(value)) => Unit::Seconds.parse_value(value).ok(),
                    Some(SettingValue::Parameter(key)) => parameters
                        .get(key.as_str())
                        .map(|parameter| parameter.maximum),
                    Some(SettingValue::Number(_)) => None,
                    None => Some(default),
                };
                if !pitch_mode
                    && let (Some(time), Some(max_time)) =
                        (seconds("time", 0.01), seconds("max_time", 0.1))
                    && time > max_time
                {
                    error(
                        node,
                        format!(
                            "delay_line `{}` time reaches {time}s, beyond its {max_time}s max_time buffer",
                            node.id
                        ),
                    );
                }
            }
            NodeKind::Math | NodeKind::StereoMath => {
                let operation = text(node, "operation", "add");
                let binary = matches!(
                    operation.as_str(),
                    "add" | "subtract" | "multiply" | "min" | "max"
                );
                if binary && !connected(node, "b") {
                    error(
                        node,
                        format!(
                            "math `{}` operation `{operation}` requires input `b`",
                            node.id
                        ),
                    );
                }
                if !binary && connected(node, "b") {
                    error(
                        node,
                        format!(
                            "math `{}` operation `{operation}` uses only input `a`",
                            node.id
                        ),
                    );
                }
                if node.setting("amount").is_some()
                    && !matches!(operation.as_str(), "offset" | "scale")
                {
                    error(
                        node,
                        format!(
                            "math `{}` setting `amount` applies only to offset and scale",
                            node.id
                        ),
                    );
                }
            }
            NodeKind::Shaper
                if node.setting("asymmetry").is_some()
                    && text(node, "curve", "tanh") != "asymmetric" =>
            {
                error(
                    node,
                    format!(
                        "shaper `{}` setting `asymmetry` applies only to the asymmetric curve",
                        node.id
                    ),
                );
            }
            _ => {}
        }
    }
}

fn validate_shaper_directives(node: &crate::model::GraphNode, diagnostics: &mut Diagnostics) {
    const MAX_POINTS: usize = 64;
    let table =
        matches!(node.setting("curve"), Some(SettingValue::Text(value)) if value == "table");
    if !table {
        if !node.directives.is_empty() {
            located_error(
                diagnostics,
                "CRV060",
                "only a table shaper may declare point directives",
                &node.location,
            );
        }
        return;
    }
    if !(2..=MAX_POINTS).contains(&node.directives.len()) {
        located_error(
            diagnostics,
            "CRV060",
            format!("table shaper requires 2..={MAX_POINTS} points"),
            &node.location,
        );
    }
    let mut ids = BTreeSet::new();
    let mut previous_input = None;
    for directive in &node.directives {
        let tokens = &directive.tokens;
        if !directive.children.is_empty()
            || tokens.len() != 6
            || tokens[0].value != "point"
            || tokens[2].value != "input"
            || tokens[4].value != "output"
        {
            located_error(
                diagnostics,
                "CRV060",
                "shaper point syntax is `point ID input VALUE output VALUE` with no children",
                &directive.location,
            );
            continue;
        }
        if !valid_id(&tokens[1].value) || !ids.insert(tokens[1].value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "shaper point ID is invalid or duplicated",
                &tokens[1].site,
            );
        }
        let bounded = |token: &crate::model::NodeDirectiveToken| {
            token
                .value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && value.abs() <= 16.0)
        };
        match (bounded(&tokens[3]), bounded(&tokens[5])) {
            (Some(input), Some(_)) if previous_input.is_none_or(|previous| input > previous) => {
                previous_input = Some(input);
            }
            (Some(_), Some(_)) => source_site_error(
                diagnostics,
                "CRV060",
                "shaper point inputs must strictly increase",
                &tokens[3].site,
            ),
            _ => source_site_error(
                diagnostics,
                "CRV060",
                "shaper point input and output must be finite within -16..=16",
                &tokens[3].site,
            ),
        }
    }
}

fn validate_envelope_mode(node: &crate::model::GraphNode, diagnostics: &mut Diagnostics) {
    let adsr = match node.setting("mode") {
        None => true,
        Some(SettingValue::Text(mode)) => mode == "adsr",
        Some(SettingValue::Number(_) | SettingValue::Parameter(_)) => false,
    };
    if adsr && let Some(site) = node.setting_site("hold") {
        source_site_error(
            diagnostics,
            "CRV195",
            "an adsr envelope cannot author a hold segment; use `mode ahdsr`",
            site,
        );
    }
}

fn validate_lfo_directives(node: &crate::model::GraphNode, diagnostics: &mut Diagnostics) {
    const MAX_CUSTOM_POINTS: usize = 64;
    let custom =
        matches!(node.setting("waveform"), Some(SettingValue::Text(value)) if value == "custom");
    if !custom && !node.directives.is_empty() {
        located_error(
            diagnostics,
            "CRV060",
            "only a custom LFO waveform may declare point directives",
            &node.location,
        );
        return;
    }
    if custom && !(2..=MAX_CUSTOM_POINTS).contains(&node.directives.len()) {
        located_error(
            diagnostics,
            "CRV060",
            format!("custom LFO requires 2..={MAX_CUSTOM_POINTS} ordered points"),
            &node.location,
        );
    }
    let mut ids = BTreeSet::new();
    let mut previous_phase = None;
    let mut first_phase = None;
    let mut last_phase = None;
    for directive in &node.directives {
        let tokens = &directive.tokens;
        if !directive.children.is_empty()
            || tokens.len() != 6
            || tokens[0].value != "point"
            || tokens[2].value != "phase"
            || tokens[4].value != "value"
        {
            located_error(
                diagnostics,
                "CRV060",
                "custom LFO point syntax is `point ID phase VALUE value VALUE` with no children",
                &directive.location,
            );
            continue;
        }
        if !valid_id(&tokens[1].value) || !ids.insert(tokens[1].value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "custom LFO point ID is invalid or duplicated",
                &tokens[1].site,
            );
        }
        let phase = tokens[3].value.parse::<f64>();
        let value = tokens[5].value.parse::<f64>();
        match (phase, value) {
            (Ok(phase), Ok(value))
                if phase.is_finite()
                    && value.is_finite()
                    && (0.0..=1.0).contains(&phase)
                    && (-1.0..=1.0).contains(&value)
                    && previous_phase.is_none_or(|previous| phase > previous) =>
            {
                first_phase.get_or_insert(phase);
                last_phase = Some(phase);
                previous_phase = Some(phase);
            }
            _ => source_site_error(
                diagnostics,
                "CRV060",
                "custom LFO points require strictly increasing phase 0..=1 and value -1..=1",
                &tokens[3].site,
            ),
        }
    }
    if custom
        && !node.directives.is_empty()
        && (first_phase != Some(0.0) || last_phase != Some(1.0))
    {
        located_error(
            diagnostics,
            "CRV060",
            "custom LFO points must begin at phase 0 and end at phase 1",
            &node.location,
        );
    }
}

fn validate_wavetable_directives(
    node: &crate::model::GraphNode,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    diagnostics: &mut Diagnostics,
) {
    if node.setting("voices").is_some()
        && let Some(site) = node.setting_site("unison")
    {
        source_site_error(
            diagnostics,
            "CRV050",
            "wavetable oscillator cannot author both `voices` and `unison`",
            site,
        );
    }
    let mut warp_count = usize::from(match node.setting("warp") {
        None => false,
        Some(SettingValue::Text(value)) => value != "none",
        Some(_) => true,
    });
    let mut ids = BTreeSet::new();
    for directive in &node.directives {
        if !directive.children.is_empty() || directive.tokens.len() != 5 {
            located_error(
                diagnostics,
                "CRV060",
                "wavetable stage syntax is `warp ID KIND amount VALUE` with no nested directives",
                &directive.location,
            );
            continue;
        }
        let tokens = &directive.tokens;
        if tokens[0].value != "warp" {
            source_site_error(
                diagnostics,
                "CRV060",
                "wavetable structured directives must be `warp` stages",
                &tokens[0].site,
            );
            continue;
        }
        warp_count = warp_count.saturating_add(1);
        if !valid_id(&tokens[1].value) || !ids.insert(tokens[1].value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "wavetable warp ID is invalid or duplicated",
                &tokens[1].site,
            );
        }
        if !matches!(
            tokens[2].value.as_str(),
            "pulse_width" | "bend" | "remap" | "phase_remap"
        ) {
            source_site_error(
                diagnostics,
                "CRV060",
                "wavetable warp must be pulse_width, bend, remap, or phase_remap",
                &tokens[2].site,
            );
        }
        if tokens[3].value != "amount" {
            source_site_error(
                diagnostics,
                "CRV060",
                "wavetable warp requires `amount VALUE`",
                &tokens[3].site,
            );
        }
        validate_analog_directive_amount(
            &tokens[4],
            parameters,
            NumericDomain::bounded(-1.0, 1.0),
            diagnostics,
        );
    }
    if warp_count > 2 {
        located_error(
            diagnostics,
            "CRV060",
            "wavetable oscillator supports at most two ordered phase-warp stages",
            &node.location,
        );
    }
}

fn validate_sample_source_directives(
    node: &crate::model::GraphNode,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    diagnostics: &mut Diagnostics,
) {
    let mut ids = BTreeSet::new();
    for directive in &node.directives {
        let tokens = &directive.tokens;
        if !directive.children.is_empty() || tokens.len() != 4 || tokens[0].value != "slice" {
            located_error(
                diagnostics,
                "CRV060",
                "sample_source directives use `slice ID START_FRAME END_FRAME` with no children",
                &directive.location,
            );
            continue;
        }
        if !valid_id(&tokens[1].value) || !ids.insert(tokens[1].value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "sample slice ID is invalid or duplicated",
                &tokens[1].site,
            );
        }
        let start = tokens[2].value.parse::<u64>();
        let end = tokens[3].value.parse::<u64>();
        if !matches!((start, end), (Ok(start), Ok(end)) if start < end) {
            source_site_error(
                diagnostics,
                "CRV060",
                "sample slice must be a positive half-open unsigned frame range",
                &tokens[2].site,
            );
        }
    }
    if node.directives.len() > MAX_SAMPLE_SLICES {
        located_error(
            diagnostics,
            "CRV060",
            format!(
                "sample_source supports at most {} slices",
                MAX_SAMPLE_SLICES
            ),
            &node.location,
        );
    }
    if let Some(SettingValue::Text(selected)) = node.setting("selected_slice")
        && !ids.contains(selected.as_str())
        && let Some(site) = node.setting_site("selected_slice")
    {
        source_site_error(
            diagnostics,
            "CRV060",
            format!("selected sample slice `{selected}` is not declared"),
            site,
        );
    }
    let scalar_bounds = |key: &str, default: f64| match node.setting(key) {
        None => Some((default, default)),
        Some(SettingValue::Number(value)) => Some((*value, *value)),
        Some(SettingValue::Text(value)) => Unit::Ratio
            .parse_value(value)
            .ok()
            .map(|value| (value, value)),
        Some(SettingValue::Parameter(parameter)) => parameters
            .get(parameter.as_str())
            .map(|parameter| (parameter.minimum, parameter.maximum)),
    };
    if let (Some((_, start_maximum)), Some((end_minimum, _))) =
        (scalar_bounds("start", 0.0), scalar_bounds("end", 1.0))
        && start_maximum >= end_minimum
    {
        located_error(
            diagnostics,
            "CRV060",
            "sample start domain must remain strictly below the end domain",
            &node.location,
        );
    }
    let loop_enabled = match node.setting("loop_mode") {
        None => false,
        Some(SettingValue::Text(mode)) => mode != "off",
        Some(_) => true,
    };
    if loop_enabled
        && let (Some((_, start_maximum)), Some((end_minimum, _))) = (
            scalar_bounds("loop_start", 0.0),
            scalar_bounds("loop_end", 1.0),
        )
        && start_maximum >= end_minimum
    {
        located_error(
            diagnostics,
            "CRV060",
            "sample loop-start domain must remain strictly below the loop-end domain",
            &node.location,
        );
    }
    if matches!(node.setting("playback"), Some(SettingValue::Text(mode)) if mode == "one_shot")
        && loop_enabled
    {
        located_error(
            diagnostics,
            "CRV060",
            "one-shot sample_source playback cannot enable a loop",
            &node.location,
        );
    }
}

/// Longest multisample zone loop crossfade, matching `sample_source`.
const MAX_ZONE_LOOP_CROSSFADE: u64 = 8_192;

fn validate_multisample_directives(node: &crate::model::GraphNode, diagnostics: &mut Diagnostics) {
    if node.directives.is_empty() || node.directives.len() > MAX_MULTISAMPLE_ZONES {
        located_error(
            diagnostics,
            "CRV060",
            format!("multisample requires 1 to {} zones", MAX_MULTISAMPLE_ZONES),
            &node.location,
        );
    }
    let mut ids = BTreeSet::new();
    let mut round_robin_indexes = BTreeSet::new();
    for directive in &node.directives {
        let Some(zone) = directive.zone_clauses() else {
            located_error(
                diagnostics,
                "CRV060",
                MULTISAMPLE_ZONE_SYNTAX,
                &directive.location,
            );
            continue;
        };
        if !valid_id(&zone.id.value) || !ids.insert(zone.id.value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "multisample zone ID is invalid or duplicated",
                &zone.id.site,
            );
        }
        let identities = [
            Some(zone.asset),
            zone.layer,
            zone.round_robin.map(|[group, _]| group),
        ];
        for token in identities.into_iter().flatten() {
            if !valid_id(&token.value) {
                source_site_error(
                    diagnostics,
                    "CRV060",
                    "zone asset, layer, and round-robin identities must be stable IDs",
                    &token.site,
                );
            }
        }
        if let Some(articulation) = zone.articulation
            && articulation.value != "default"
        {
            source_site_error(
                diagnostics,
                "CRV060",
                "the first multisample slice supports only articulation `default`",
                &articulation.site,
            );
        }
        let low_key = parse_pitch(&zone.keys[0].value);
        let high_key = parse_pitch(&zone.keys[1].value);
        let root_key = parse_pitch(&zone.root.value);
        if !matches!((low_key, high_key, root_key), (Ok(low), Ok(high), Ok(_)) if low <= high) {
            source_site_error(
                diagnostics,
                "CRV060",
                "zone keys/root require scientific pitches and LOW must not exceed HIGH",
                &zone.keys[0].site,
            );
        }
        if let Some([low, high]) = zone.velocity {
            let low_velocity = Unit::Ratio.parse_value(&low.value);
            let high_velocity = Unit::Ratio.parse_value(&high.value);
            if !matches!((low_velocity, high_velocity), (Ok(low), Ok(high)) if (0.0..=1.0).contains(&low) && (low..=1.0).contains(&high))
            {
                source_site_error(
                    diagnostics,
                    "CRV060",
                    "zone velocity range must be ordered within 0..=1",
                    &low.site,
                );
            }
        }
        if let Some([group, index]) = zone.round_robin {
            match index.value.parse::<u16>() {
                Ok(index)
                    if round_robin_indexes.insert((
                        zone.layer_id(),
                        group.value.as_str(),
                        index,
                    )) => {}
                _ => source_site_error(
                    diagnostics,
                    "CRV060",
                    "round-robin index must be an unsigned value unique within its layer/group",
                    &index.site,
                ),
            }
        }
        let region = zone
            .region
            .map(|[start, end]| (start, start.value.parse::<u64>(), end.value.parse::<u64>()));
        if let Some((start_token, start, end)) = &region
            && !matches!((start, end), (Ok(start), Ok(end)) if start < end)
        {
            source_site_error(
                diagnostics,
                "CRV060",
                "zone region must be a positive half-open unsigned frame range",
                &start_token.site,
            );
        }
        let bounds = match &region {
            Some((_, Ok(start), Ok(end))) => Some((*start, *end)),
            Some(_) => None,
            // The asset length bounds a loop in a whole-asset zone; it is
            // checked against the media inventory.
            None => Some((0, u64::MAX)),
        };
        if let (Some([start_token, end_token, crossfade_token]), Some((start, end))) =
            (zone.loop_region, bounds)
        {
            let valid = matches!(
                (
                    start_token.value.parse::<u64>(),
                    end_token.value.parse::<u64>(),
                    crossfade_token.value.parse::<u64>()
                ),
                (Ok(loop_start), Ok(loop_end), Ok(crossfade))
                    if start <= loop_start
                        && loop_start < loop_end
                        && loop_end <= end
                        && crossfade <= MAX_ZONE_LOOP_CROSSFADE
                        && crossfade.saturating_mul(2) <= loop_end - loop_start
            );
            if !valid {
                source_site_error(
                    diagnostics,
                    "CRV060",
                    format!(
                        "zone loop must be a non-empty frame range inside the zone region with a crossfade of at most half the loop and {MAX_ZONE_LOOP_CROSSFADE} frames"
                    ),
                    &start_token.site,
                );
            }
        }
        if let Some(gain) = zone.gain
            && !matches!(Unit::Decibels.parse_value(&gain.value), Ok(gain) if (-96.0..=24.0).contains(&gain))
        {
            source_site_error(
                diagnostics,
                "CRV060",
                "zone gain must be -96db..=24db",
                &gain.site,
            );
        }
        if let Some(pan) = zone.pan
            && !matches!(Unit::Pan.parse_value(&pan.value), Ok(pan) if (-1.0..=1.0).contains(&pan))
        {
            source_site_error(diagnostics, "CRV060", "zone pan must be -1..=1", &pan.site);
        }
    }
}

fn validate_spectral_settings(
    node: &crate::model::GraphNode,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    diagnostics: &mut Diagnostics,
) {
    if !node.directives.is_empty() {
        located_error(
            diagnostics,
            "CRV200",
            "spectral source accepts only flat key/value settings",
            &node.location,
        );
    }
    if !matches!(node.setting("asset"), Some(SettingValue::Text(asset)) if valid_id(asset)) {
        located_error(
            diagnostics,
            "CRV200",
            "spectral source requires one stable Project asset ID",
            &node.location,
        );
    }
    if !matches!(node.setting("algorithm"), Some(SettingValue::Text(algorithm)) if algorithm == crate::engine::spectral::SPECTRAL_ANALYSIS_ALGORITHM)
    {
        located_error(
            diagnostics,
            "CRV200",
            format!(
                "spectral source requires algorithm `{}`",
                crate::engine::spectral::SPECTRAL_ANALYSIS_ALGORITHM
            ),
            &node.location,
        );
    }
    let scalar_bounds = |key: &str, default: f64| match node.setting(key) {
        None => Some((default, default)),
        Some(SettingValue::Number(value)) if value.is_finite() => Some((*value, *value)),
        Some(SettingValue::Text(value)) => Unit::Hertz
            .parse_value(value)
            .ok()
            .map(|value| (value, value)),
        Some(SettingValue::Parameter(parameter)) => parameters
            .get(parameter.as_str())
            .map(|parameter| (parameter.minimum, parameter.maximum)),
        Some(_) => None,
    };
    if let (Some((_, low_maximum)), Some((high_minimum, _))) =
        (scalar_bounds("low", 20.0), scalar_bounds("high", 20_000.0))
        && low_maximum >= high_minimum
    {
        located_error(
            diagnostics,
            "CRV200",
            "spectral low-frequency domain must remain strictly below the high-frequency domain",
            &node.location,
        );
    }
}

fn validate_analog_directives(
    node: &crate::model::GraphNode,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    diagnostics: &mut Diagnostics,
) {
    if node.setting("voices").is_some()
        && let Some(site) = node.setting_site("unison")
    {
        source_site_error(
            diagnostics,
            "CRV050",
            "analog oscillator cannot author both `voices` and `unison`",
            site,
        );
    }
    let mut warp_count = usize::from(match node.setting("warp") {
        None => false,
        Some(SettingValue::Text(value)) => value != "none",
        Some(_) => true,
    });
    let mut distortion_count = usize::from(match node.setting("distortion") {
        None => false,
        Some(SettingValue::Text(value)) => value != "none",
        Some(_) => true,
    });
    let mut ids = BTreeSet::new();
    for directive in &node.directives {
        if !directive.children.is_empty() || directive.tokens.len() != 5 {
            located_error(
                diagnostics,
                "CRV060",
                "analog oscillator stage syntax is `warp|distortion ID KIND amount VALUE` with no nested directives",
                &directive.location,
            );
            continue;
        }
        let tokens = &directive.tokens;
        if !valid_id(&tokens[1].value) || !ids.insert(tokens[1].value.as_str()) {
            source_site_error(
                diagnostics,
                "CRV060",
                "analog oscillator stage ID is invalid or duplicated",
                &tokens[1].site,
            );
        }
        if tokens[3].value != "amount" {
            source_site_error(
                diagnostics,
                "CRV060",
                "analog oscillator stage requires `amount VALUE`",
                &tokens[3].site,
            );
        }
        let domain = match tokens[0].value.as_str() {
            "warp" => {
                warp_count = warp_count.saturating_add(1);
                if !matches!(
                    tokens[2].value.as_str(),
                    "pulse_width" | "bend" | "remap" | "phase_remap"
                ) {
                    source_site_error(
                        diagnostics,
                        "CRV060",
                        "analog oscillator warp must be pulse_width, bend, remap, or phase_remap",
                        &tokens[2].site,
                    );
                }
                NumericDomain::bounded(-1.0, 1.0)
            }
            "distortion" => {
                distortion_count = distortion_count.saturating_add(1);
                if !matches!(tokens[2].value.as_str(), "soft_clip" | "tanh") {
                    source_site_error(
                        diagnostics,
                        "CRV060",
                        "analog oscillator distortion must be soft_clip or tanh",
                        &tokens[2].site,
                    );
                }
                NumericDomain::bounded(0.0, 1.0)
            }
            _ => {
                source_site_error(
                    diagnostics,
                    "CRV060",
                    "analog oscillator structured directive must be `warp` or `distortion`",
                    &tokens[0].site,
                );
                continue;
            }
        };
        validate_analog_directive_amount(&tokens[4], parameters, domain, diagnostics);
    }
    if warp_count > 2 {
        located_error(
            diagnostics,
            "CRV060",
            "analog oscillator supports at most two ordered phase-warp stages",
            &node.location,
        );
    }
    if distortion_count > 1 {
        located_error(
            diagnostics,
            "CRV060",
            "analog oscillator supports at most one post-read distortion stage",
            &node.location,
        );
    }
}

fn validate_analog_directive_amount(
    token: &crate::model::NodeDirectiveToken,
    parameters: &BTreeMap<&str, &crate::model::Parameter>,
    domain: NumericDomain,
    diagnostics: &mut Diagnostics,
) {
    if let Some(parameter_key) = token.value.strip_prefix('$') {
        match parameters.get(parameter_key) {
            Some(parameter)
                if parameter.unit == Unit::Ratio
                    && domain.contains(parameter.minimum)
                    && domain.contains(parameter.maximum) => {}
            Some(parameter) if parameter.unit != Unit::Ratio => source_site_error(
                diagnostics,
                "CRV052",
                format!(
                    "analog oscillator stage amount expects ratio, but Parameter `{}` uses {}",
                    parameter.key, parameter.unit
                ),
                &token.site,
            ),
            Some(parameter) => source_site_error(
                diagnostics,
                "CRV057",
                format!(
                    "Parameter `{}` domain {}..={} exceeds stage amount range {}",
                    parameter.key,
                    parameter.minimum,
                    parameter.maximum,
                    domain.label()
                ),
                &token.site,
            ),
            None => source_site_error(
                diagnostics,
                "CRV053",
                format!("stage amount references missing Parameter `{parameter_key}`"),
                &token.site,
            ),
        }
    } else {
        match Unit::Ratio.parse_value(&token.value) {
            Ok(value) if domain.contains(value) => {}
            _ => source_site_error(
                diagnostics,
                "CRV055",
                format!(
                    "analog oscillator stage amount must be a finite ratio in {}",
                    domain.label()
                ),
                &token.site,
            ),
        }
    }
}

#[derive(Clone, Copy)]
struct NumericDomain {
    minimum: f64,
    maximum: Option<f64>,
}

impl NumericDomain {
    const fn bounded(minimum: f64, maximum: f64) -> Self {
        Self {
            minimum,
            maximum: Some(maximum),
        }
    }

    fn contains(self, value: f64) -> bool {
        value >= self.minimum && self.maximum.is_none_or(|maximum| value <= maximum)
    }

    fn label(self) -> String {
        self.maximum.map_or_else(
            || format!("{} or greater", self.minimum),
            |maximum| format!("{}..={maximum}", self.minimum),
        )
    }
}

fn setting_domain(kind: NodeKind, key: &str) -> Option<NumericDomain> {
    match (kind, key) {
        (NodeKind::Exciter, "contact") => Some(NumericDomain::bounded(0.00002, 0.02)),
        (NodeKind::Exciter, "hardness") | (NodeKind::ModalBank, "key_track") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Exciter, "rate") => Some(NumericDomain::bounded(1.0, 10000.0)),
        (NodeKind::Exciter, "duration") => Some(NumericDomain::bounded(0.005, 2.0)),
        (NodeKind::Exciter, "attack") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::ModalBank, "rise") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::ModalBank, "decay_scale") => Some(NumericDomain::bounded(0.1, 4.0)),
        (NodeKind::ModalBank, "brightness") => Some(NumericDomain::bounded(-2.0, 2.0)),
        (NodeKind::Exciter | NodeKind::ModalBank, "level") => {
            Some(NumericDomain::bounded(0.0, 4.0))
        }
        (
            NodeKind::Oscillator
            | NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral
            | NodeKind::Noise
            | NodeKind::Amplifier,
            "level",
        )
        | (NodeKind::Mixer | NodeKind::StereoMixer, "a_level" | "b_level") => {
            Some(NumericDomain::bounded(0.0, 4.0))
        }
        (NodeKind::Oscillator, "detune") => Some(NumericDomain::bounded(-96.0, 96.0)),
        (
            NodeKind::Oscillator
            | NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable,
            "glide",
        ) => Some(NumericDomain::bounded(0.0, 5.0)),
        (NodeKind::Unison | NodeKind::AnalogOscillator | NodeKind::Wavetable, "detune") => {
            Some(NumericDomain::bounded(0.0, 2.0))
        }
        (NodeKind::Unison | NodeKind::AnalogOscillator | NodeKind::Wavetable, "drift") => {
            Some(NumericDomain::bounded(0.0, 0.5))
        }
        (NodeKind::Unison, "mod_depth") => Some(NumericDomain::bounded(-24.0, 24.0)),
        (
            NodeKind::Unison | NodeKind::AnalogOscillator | NodeKind::Wavetable,
            "width" | "velocity_amount",
        ) => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::AnalogOscillator, "blend" | "am_depth")
        | (NodeKind::Wavetable, "blend" | "table_position") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::AnalogOscillator | NodeKind::Wavetable, "pan") => {
            Some(NumericDomain::bounded(-1.0, 1.0))
        }
        (NodeKind::AnalogOscillator | NodeKind::Wavetable, "coarse" | "fm_depth") => {
            Some(NumericDomain::bounded(-48.0, 48.0))
        }
        (NodeKind::AnalogOscillator | NodeKind::Wavetable, "fine") => {
            Some(NumericDomain::bounded(-1.0, 1.0))
        }
        (NodeKind::AnalogOscillator, "pm_depth" | "phase" | "random_phase" | "drive")
        | (NodeKind::Wavetable, "pm_depth" | "phase" | "random_phase") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::AnalogOscillator | NodeKind::Wavetable, "warp_amount") => {
            Some(NumericDomain::bounded(-1.0, 1.0))
        }
        (
            NodeKind::SampleSource | NodeKind::Multisample,
            "start" | "end" | "loop_start" | "loop_end" | "velocity_amount",
        ) => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::SampleSource | NodeKind::Multisample, "rate") => {
            Some(NumericDomain::bounded(0.125, 8.0))
        }
        (NodeKind::SampleSource | NodeKind::Multisample, "tune") => {
            Some(NumericDomain::bounded(-48.0, 48.0))
        }
        (NodeKind::SampleSource | NodeKind::Multisample, "pan") => {
            Some(NumericDomain::bounded(-1.0, 1.0))
        }
        (NodeKind::Granular, "position" | "spray" | "direction_probability" | "stereo_spread") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Granular, "scan") => Some(NumericDomain::bounded(-2.0, 2.0)),
        (NodeKind::Granular, "size") => Some(NumericDomain::bounded(0.005, 1.0)),
        (NodeKind::Granular, "density") => Some(NumericDomain::bounded(0.1, 1_000.0)),
        (NodeKind::Granular, "tune") => Some(NumericDomain::bounded(-48.0, 48.0)),
        (NodeKind::Spectral, "scan" | "phase_randomization") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Spectral, "rate") => Some(NumericDomain::bounded(-4.0, 4.0)),
        (NodeKind::Spectral, "tune") => Some(NumericDomain::bounded(-48.0, 48.0)),
        (NodeKind::Spectral, "low" | "high") => Some(NumericDomain::bounded(20.0, 21_600.0)),
        (NodeKind::Envelope, "attack" | "hold" | "decay") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Envelope, "sustain") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::Envelope, "attack_curve" | "decay_curve" | "release_curve") => {
            Some(NumericDomain::bounded(-1.0, 1.0))
        }
        (NodeKind::Envelope, "release") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "cutoff" | "key_reference") => {
            Some(NumericDomain::bounded(10.0, 21_600.0))
        }
        (NodeKind::Lowpass | NodeKind::StereoFilter, "mod_depth" | "velocity_depth") => {
            Some(NumericDomain::bounded(-21_600.0, 21_600.0))
        }
        (NodeKind::Lowpass | NodeKind::StereoFilter, "resonance") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Lowpass | NodeKind::StereoFilter, "key_track") => {
            Some(NumericDomain::bounded(0.0, 2.0))
        }
        (NodeKind::StereoFilter, "drive" | "mix") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::StereoFilter, "pan") => Some(NumericDomain::bounded(-1.0, 1.0)),
        (NodeKind::Saturation, "drive") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Pan, "position") => Some(NumericDomain::bounded(-1.0, 1.0)),
        (NodeKind::Delay, "time") => Some(NumericDomain::bounded(0.001, 2.0)),
        (NodeKind::Delay, "feedback") => Some(NumericDomain::bounded(0.0, 0.95)),
        (NodeKind::Delay | NodeKind::Reverb, "mix") | (NodeKind::Reverb, "room") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Reverb, "damping") => Some(NumericDomain::bounded(0.0, 0.95)),
        (NodeKind::Lfo, "rate") => Some(NumericDomain::bounded(0.01, 50.0)),
        (NodeKind::Lfo, "depth" | "phase" | "random_phase") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Lfo, "swing") => Some(NumericDomain::bounded(-1.0, 1.0)),
        (NodeKind::DelayLine, "time") => Some(NumericDomain::bounded(0.0, 2.0)),
        (NodeKind::DelayLine, "max_time") => Some(NumericDomain::bounded(0.001, 2.0)),
        (NodeKind::DelayLine, "ratio") => Some(NumericDomain::bounded(0.01, 16.0)),
        (NodeKind::DelayLine, "mod_depth") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::DelayLine, "tune_compensation") => Some(NumericDomain::bounded(0.0, 16.0)),
        (NodeKind::Math | NodeKind::StereoMath, "amount") => {
            Some(NumericDomain::bounded(-64.0, 64.0))
        }
        (NodeKind::Shaper, "drive") => Some(NumericDomain::bounded(0.0, 64.0)),
        (NodeKind::Shaper, "bias") => Some(NumericDomain::bounded(-16.0, 16.0)),
        (NodeKind::Shaper, "asymmetry") | (NodeKind::LinearPhaseLowpass, "amount") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::DcBlock, "cutoff") => Some(NumericDomain::bounded(1.0, 500.0)),
        _ => None,
    }
}

fn validate_setting_domain(
    node: &crate::model::GraphNode,
    key: &str,
    value: f64,
    parameter_domain: Option<(f64, f64, &str)>,
    site: &crate::model::ValueSite,
    diagnostics: &mut Diagnostics,
) {
    let Some(domain) = setting_domain(node.kind, key) else {
        return;
    };
    if !domain.contains(value) {
        source_site_error(
            diagnostics,
            "CRV055",
            format!(
                "setting `{key}` value {value} is outside its supported {} range",
                domain.label()
            ),
            site,
        );
    }
    if let Some((minimum, maximum, parameter)) = parameter_domain
        && (!domain.contains(minimum) || !domain.contains(maximum))
    {
        source_site_error(
            diagnostics,
            "CRV057",
            format!(
                "Parameter `{parameter}` domain {minimum}..={maximum} exceeds setting `{key}` range {}",
                domain.label()
            ),
            site,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalType {
    AudioMono,
    AudioStereo,
    Hertz,
    Gate,
    Unipolar,
    Control,
}

impl SignalType {
    const fn label(self) -> &'static str {
        match self {
            Self::AudioMono => "mono Audio",
            Self::AudioStereo => "stereo Audio",
            Self::Hertz => "hertz Control",
            Self::Gate => "gate Control",
            Self::Unipolar => "unipolar Control",
            Self::Control => "uni- or bipolar Control",
        }
    }
}

fn output_type(
    domain: GraphDomain,
    node: &str,
    port: &str,
    nodes: &BTreeMap<&str, &crate::model::GraphNode>,
) -> Result<SignalType, String> {
    if node == "note" {
        if domain != GraphDomain::Voice {
            return Err("shared graph cannot read the per-voice `note` source".to_owned());
        }
        return match port {
            "pitch" => Ok(SignalType::Hertz),
            "gate" => Ok(SignalType::Gate),
            "velocity" => Ok(SignalType::Unipolar),
            _ => Err(format!("note source has no `{port}` output")),
        };
    }
    if node == "voice_sum" {
        if domain != GraphDomain::Shared {
            return Err("voice graph cannot read the post-voice `voice_sum` source".to_owned());
        }
        return match port {
            "audio" => Ok(SignalType::AudioStereo),
            _ => Err(format!("voice_sum source has no `{port}` output")),
        };
    }
    let graph_node = nodes
        .get(node)
        .ok_or_else(|| format!("connection references missing node `{node}`"))?;
    node_output(graph_node.kind, port).ok_or_else(|| {
        format!(
            "{} node `{node}` has no `{port}` output",
            node_kind_name(graph_node.kind)
        )
    })
}

fn input_type(
    node: &str,
    port: &str,
    nodes: &BTreeMap<&str, &crate::model::GraphNode>,
) -> Result<SignalType, String> {
    if node == "note" {
        return Err("the reserved `note` source has no inputs".to_owned());
    }
    let graph_node = nodes
        .get(node)
        .ok_or_else(|| format!("connection references missing node `{node}`"))?;
    node_input(graph_node.kind, port).ok_or_else(|| {
        format!(
            "{} node `{node}` has no `{port}` input",
            node_kind_name(graph_node.kind)
        )
    })
}

fn node_output(kind: NodeKind, port: &str) -> Option<SignalType> {
    match (kind, port) {
        (NodeKind::Exciter | NodeKind::ModalBank, "audio") => Some(SignalType::AudioMono),
        (
            NodeKind::Oscillator
            | NodeKind::Noise
            | NodeKind::Mixer
            | NodeKind::Sum
            | NodeKind::Lowpass
            | NodeKind::Amplifier
            | NodeKind::Saturation
            | NodeKind::StereoToMono,
            "audio",
        ) => Some(SignalType::AudioMono),
        (
            NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral
            | NodeKind::StereoFilter
            | NodeKind::Pan
            | NodeKind::Delay
            | NodeKind::Reverb
            | NodeKind::MonoToStereo
            | NodeKind::StereoMixer
            | NodeKind::StereoSum,
            "audio",
        ) => Some(SignalType::AudioStereo),
        (NodeKind::Envelope, "control") => Some(SignalType::Unipolar),
        (NodeKind::Lfo | NodeKind::ToControl, "control") => Some(SignalType::Control),
        (
            NodeKind::DelayLine
            | NodeKind::Math
            | NodeKind::Shaper
            | NodeKind::ToAudio
            | NodeKind::DcBlock
            | NodeKind::LinearPhaseLowpass,
            "audio",
        ) => Some(SignalType::AudioMono),
        (NodeKind::StereoMath, "audio") => Some(SignalType::AudioStereo),
        _ => None,
    }
}

fn node_input(kind: NodeKind, port: &str) -> Option<SignalType> {
    match (kind, port) {
        (NodeKind::Exciter | NodeKind::ModalBank, "gate") => Some(SignalType::Gate),
        (NodeKind::Exciter, "velocity") => Some(SignalType::Unipolar),
        (NodeKind::ModalBank, "pitch") => Some(SignalType::Hertz),
        (NodeKind::ModalBank, "audio") => Some(SignalType::AudioMono),
        (
            NodeKind::Oscillator
            | NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral,
            "pitch",
        ) => Some(SignalType::Hertz),
        (
            NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample,
            "gain" | "velocity",
        ) => Some(SignalType::Unipolar),
        (NodeKind::Unison, "mod") => Some(SignalType::Control),
        (NodeKind::AnalogOscillator, "fm" | "pm" | "sync" | "am")
        | (NodeKind::Wavetable, "fm" | "pm" | "sync") => Some(SignalType::AudioMono),
        (
            NodeKind::Envelope
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular,
            "gate",
        ) => Some(SignalType::Gate),
        (NodeKind::Mixer, "a" | "b") => Some(SignalType::AudioMono),
        (NodeKind::StereoMixer, "a" | "b") => Some(SignalType::AudioStereo),
        (NodeKind::StereoSum, "inputs") => Some(SignalType::AudioStereo),
        (NodeKind::Sum, "inputs") => Some(SignalType::AudioMono),
        (
            NodeKind::Lowpass
            | NodeKind::Amplifier
            | NodeKind::Saturation
            | NodeKind::Pan
            | NodeKind::MonoToStereo,
            "audio",
        ) => Some(SignalType::AudioMono),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "key") => Some(SignalType::Hertz),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "mod") => Some(SignalType::Control),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "velocity") => Some(SignalType::Unipolar),
        (NodeKind::Amplifier, "gain") => Some(SignalType::Unipolar),
        (
            NodeKind::Delay
            | NodeKind::Reverb
            | NodeKind::Output
            | NodeKind::StereoToMono
            | NodeKind::StereoFilter,
            "audio",
        ) => Some(SignalType::AudioStereo),
        (
            NodeKind::DelayLine
            | NodeKind::Shaper
            | NodeKind::ToControl
            | NodeKind::DcBlock
            | NodeKind::LinearPhaseLowpass,
            "audio",
        )
        | (NodeKind::DelayLine, "time_mod")
        | (NodeKind::Math, "a" | "b") => Some(SignalType::AudioMono),
        (NodeKind::DelayLine, "pitch") => Some(SignalType::Hertz),
        (NodeKind::StereoMath, "a" | "b") => Some(SignalType::AudioStereo),
        (NodeKind::ToAudio, "control") => Some(SignalType::Control),
        _ => None,
    }
}

const fn required_inputs(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Exciter => &["gate", "velocity"],
        NodeKind::ModalBank => &["gate", "pitch", "audio"],
        NodeKind::Oscillator
        | NodeKind::Unison
        | NodeKind::AnalogOscillator
        | NodeKind::Wavetable => &["pitch"],
        NodeKind::SampleSource => &["pitch", "gate"],
        NodeKind::Multisample => &["pitch", "gate", "velocity"],
        NodeKind::Granular => &["pitch", "gate"],
        NodeKind::Spectral => &["pitch"],
        NodeKind::Noise | NodeKind::Lfo => &[],
        NodeKind::Envelope => &["gate"],
        NodeKind::Mixer | NodeKind::StereoMixer => &["a"],
        NodeKind::StereoSum | NodeKind::Sum => &["inputs"],
        NodeKind::Lowpass
        | NodeKind::Saturation
        | NodeKind::Pan
        | NodeKind::MonoToStereo
        | NodeKind::StereoToMono => &["audio"],
        NodeKind::Amplifier => &["audio", "gain"],
        NodeKind::Delay | NodeKind::Reverb | NodeKind::Output | NodeKind::StereoFilter => {
            &["audio"]
        }
        NodeKind::DelayLine
        | NodeKind::Shaper
        | NodeKind::ToControl
        | NodeKind::DcBlock
        | NodeKind::LinearPhaseLowpass => &["audio"],
        NodeKind::Math | NodeKind::StereoMath => &["a"],
        NodeKind::ToAudio => &["control"],
    }
}

const fn signal_compatible(from: SignalType, to: SignalType) -> bool {
    from as u8 == to as u8 || matches!((from, to), (SignalType::Unipolar, SignalType::Control))
}

#[derive(Clone, Copy)]
struct SettingSpec {
    key: &'static str,
    kind: SettingKind,
}

#[derive(Clone, Copy)]
enum SettingKind {
    Number(Unit),
    Duration,
    LfoRate,
    Identifier,
    Pitch,
    Integer { minimum: u16, maximum: u16 },
    Text(&'static [&'static str]),
}

const fn setting_schema(kind: NodeKind) -> &'static [SettingSpec] {
    use SettingKind::{Duration, Identifier, Integer, LfoRate, Number, Pitch, Text};
    match kind {
        NodeKind::Exciter => &[
            SettingSpec {
                key: "kind",
                kind: Text(&["impact", "collisions", "texture"]),
            },
            SettingSpec {
                key: "contact",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "attack",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "hardness",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "rate",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "duration",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::ModalBank => &[
            SettingSpec {
                key: "rise",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "profile",
                kind: Identifier,
            },
            SettingSpec {
                key: "max_modes",
                kind: Integer {
                    minimum: 1,
                    maximum: 256,
                },
            },
            SettingSpec {
                key: "decay_scale",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "brightness",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "key_track",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Oscillator => &[
            SettingSpec {
                key: "waveform",
                kind: Text(&["sine", "saw", "square", "triangle"]),
            },
            SettingSpec {
                key: "detune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "glide",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Unison => &[
            SettingSpec {
                key: "waveform",
                kind: Text(&["sine", "saw", "square", "triangle"]),
            },
            SettingSpec {
                key: "voices",
                kind: Integer {
                    minimum: 1,
                    maximum: 8,
                },
            },
            SettingSpec {
                key: "detune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "width",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "drift",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "glide",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "mod_depth",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "velocity_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::AnalogOscillator => &[
            SettingSpec {
                key: "waveform",
                kind: Text(&["sine", "saw", "square", "pulse", "triangle"]),
            },
            SettingSpec {
                key: "voices",
                kind: Integer {
                    minimum: 1,
                    maximum: 8,
                },
            },
            SettingSpec {
                key: "unison",
                kind: Integer {
                    minimum: 1,
                    maximum: 8,
                },
            },
            SettingSpec {
                key: "coarse",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "fine",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "detune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "blend",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "width",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "drift",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "glide",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "fm_depth",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "pm_depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "am_mode",
                kind: Text(&["am", "ring"]),
            },
            SettingSpec {
                key: "am_depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "random_phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "warp",
                kind: Text(&["none", "pulse_width", "bend", "remap"]),
            },
            SettingSpec {
                key: "warp_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "distortion",
                kind: Text(&["none", "soft_clip", "tanh"]),
            },
            SettingSpec {
                key: "drive",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "velocity_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "pan",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Wavetable => &[
            SettingSpec {
                key: "asset",
                kind: Identifier,
            },
            SettingSpec {
                key: "voices",
                kind: Integer {
                    minimum: 1,
                    maximum: 8,
                },
            },
            SettingSpec {
                key: "unison",
                kind: Integer {
                    minimum: 1,
                    maximum: 8,
                },
            },
            SettingSpec {
                key: "coarse",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "fine",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "table_position",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "random_phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "detune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "blend",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "width",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "drift",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "glide",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "fm_depth",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "pm_depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "warp",
                kind: Text(&["none", "pulse_width", "bend", "remap"]),
            },
            SettingSpec {
                key: "warp_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "velocity_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "pan",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::SampleSource => &[
            SettingSpec {
                key: "asset",
                kind: Identifier,
            },
            SettingSpec {
                key: "selected_slice",
                kind: Identifier,
            },
            SettingSpec {
                key: "start",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "end",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "loop_mode",
                kind: Text(&["off", "forward", "ping_pong"]),
            },
            SettingSpec {
                key: "loop_start",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "loop_end",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "loop_crossfade",
                kind: Integer {
                    minimum: 0,
                    maximum: 8_192,
                },
            },
            SettingSpec {
                key: "direction",
                kind: Text(&["forward", "reverse"]),
            },
            SettingSpec {
                key: "playback",
                kind: Text(&["gated", "one_shot"]),
            },
            SettingSpec {
                key: "root",
                kind: Pitch,
            },
            SettingSpec {
                key: "tune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "rate",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "velocity_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "pan",
                kind: Number(Unit::Pan),
            },
        ],
        NodeKind::Multisample => &[
            SettingSpec {
                key: "playback",
                kind: Text(&["gated", "one_shot"]),
            },
            SettingSpec {
                key: "tune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "rate",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "velocity_amount",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "pan",
                kind: Number(Unit::Pan),
            },
        ],
        NodeKind::Granular => &[
            SettingSpec {
                key: "asset",
                kind: Identifier,
            },
            SettingSpec {
                key: "max_grains",
                kind: Integer {
                    minimum: 1,
                    maximum: 64,
                },
            },
            SettingSpec {
                key: "position",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "scan",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "size",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "density",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "root",
                kind: Pitch,
            },
            SettingSpec {
                key: "tune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "spray",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "window",
                kind: Text(&["hann", "triangle", "tukey"]),
            },
            SettingSpec {
                key: "edge",
                kind: Text(&["wrap", "reflect", "clamp"]),
            },
            SettingSpec {
                key: "direction_probability",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "stereo_spread",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "freeze",
                kind: Number(Unit::Boolean),
            },
            SettingSpec {
                key: "seed",
                kind: Integer {
                    minimum: 0,
                    maximum: u16::MAX,
                },
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Noise => &[SettingSpec {
            key: "level",
            kind: Number(Unit::Ratio),
        }],
        NodeKind::Envelope => &[
            SettingSpec {
                key: "mode",
                kind: Text(&["adsr", "ahdsr"]),
            },
            SettingSpec {
                key: "attack",
                kind: Duration,
            },
            SettingSpec {
                key: "hold",
                kind: Duration,
            },
            SettingSpec {
                key: "decay",
                kind: Duration,
            },
            SettingSpec {
                key: "sustain",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "release",
                kind: Duration,
            },
            SettingSpec {
                key: "attack_curve",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "decay_curve",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "release_curve",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "retrigger",
                kind: Text(&["always", "legato"]),
            },
        ],
        NodeKind::Mixer | NodeKind::StereoMixer => &[
            SettingSpec {
                key: "a_level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "b_level",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::MonoToStereo | NodeKind::StereoToMono => &[],
        NodeKind::StereoSum | NodeKind::Sum => &[SettingSpec {
            key: "max_inputs",
            kind: Integer {
                minimum: 1,
                maximum: MAX_STEREO_SUM_INPUTS as u16,
            },
        }],
        NodeKind::Lowpass => &[
            SettingSpec {
                key: "mode",
                kind: Text(&["lowpass", "highpass", "bandpass", "notch"]),
            },
            SettingSpec {
                key: "cutoff",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "resonance",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "mod_depth",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "velocity_depth",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "key_track",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "key_reference",
                kind: Number(Unit::Hertz),
            },
        ],
        NodeKind::StereoFilter => &[
            SettingSpec {
                key: "model",
                kind: Text(&["svf_clean", "ladder_drive"]),
            },
            SettingSpec {
                key: "mode",
                kind: Text(&["lowpass", "highpass", "bandpass", "notch"]),
            },
            SettingSpec {
                key: "cutoff",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "resonance",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "drive",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "mix",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "pan",
                kind: Number(Unit::Pan),
            },
            SettingSpec {
                key: "mod_depth",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "velocity_depth",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "key_track",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "key_reference",
                kind: Number(Unit::Hertz),
            },
        ],
        NodeKind::Amplifier => &[SettingSpec {
            key: "level",
            kind: Number(Unit::Ratio),
        }],
        NodeKind::Saturation => &[SettingSpec {
            key: "drive",
            kind: Number(Unit::Ratio),
        }],
        NodeKind::Pan => &[SettingSpec {
            key: "position",
            kind: Number(Unit::Pan),
        }],
        NodeKind::Delay => &[
            SettingSpec {
                key: "time",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "feedback",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "mix",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Reverb => &[
            SettingSpec {
                key: "room",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "damping",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "mix",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Output => &[],
        NodeKind::Lfo => &[
            SettingSpec {
                key: "rate",
                kind: LfoRate,
            },
            SettingSpec {
                key: "depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "random_phase",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "swing",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "waveform",
                kind: Text(&["sine", "triangle", "saw", "square", "sample_hold", "custom"]),
            },
            SettingSpec {
                key: "mode",
                kind: Text(&["loop", "one_shot"]),
            },
            SettingSpec {
                key: "direction",
                kind: Text(&["forward", "reverse", "ping_pong"]),
            },
            SettingSpec {
                key: "polarity",
                kind: Text(&["unipolar", "bipolar"]),
            },
            SettingSpec {
                key: "retrigger",
                kind: Text(&["note", "transport"]),
            },
        ],
        NodeKind::DelayLine => &[
            SettingSpec {
                key: "mode",
                kind: Text(&["time", "pitch"]),
            },
            SettingSpec {
                key: "time",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "ratio",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "max_time",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "mod_depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "interpolation",
                kind: Text(&["linear", "lagrange3", "allpass"]),
            },
            SettingSpec {
                key: "tune_compensation",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Math | NodeKind::StereoMath => &[
            SettingSpec {
                key: "operation",
                kind: Text(&[
                    "add", "subtract", "multiply", "min", "max", "abs", "negate", "offset", "scale",
                ]),
            },
            SettingSpec {
                key: "amount",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::Shaper => &[
            SettingSpec {
                key: "curve",
                kind: Text(&[
                    "tanh",
                    "hard_clip",
                    "soft_clip",
                    "one_sided",
                    "asymmetric",
                    "table",
                ]),
            },
            SettingSpec {
                key: "drive",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "bias",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "asymmetry",
                kind: Number(Unit::Ratio),
            },
        ],
        NodeKind::ToAudio | NodeKind::ToControl => &[],
        NodeKind::DcBlock => &[SettingSpec {
            key: "cutoff",
            kind: Number(Unit::Hertz),
        }],
        NodeKind::LinearPhaseLowpass => &[SettingSpec {
            key: "amount",
            kind: Number(Unit::Ratio),
        }],
        NodeKind::Spectral => &[
            SettingSpec {
                key: "asset",
                kind: Identifier,
            },
            SettingSpec {
                key: "algorithm",
                kind: Text(&[crate::engine::spectral::SPECTRAL_ANALYSIS_ALGORITHM]),
            },
            SettingSpec {
                key: "max_peaks",
                kind: Integer {
                    minimum: 1,
                    maximum: MAX_SPECTRAL_PEAKS as u16,
                },
            },
            SettingSpec {
                key: "scan",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "rate",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "root",
                kind: Pitch,
            },
            SettingSpec {
                key: "tune",
                kind: Number(Unit::Semitones),
            },
            SettingSpec {
                key: "low",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "high",
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "transients",
                kind: Text(&["preserve", "reset", "suppress"]),
            },
            SettingSpec {
                key: "phase_randomization",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "freeze",
                kind: Number(Unit::Boolean),
            },
            SettingSpec {
                key: "seed",
                kind: Integer {
                    minimum: 0,
                    maximum: u16::MAX,
                },
            },
            SettingSpec {
                key: "level",
                kind: Number(Unit::Ratio),
            },
        ],
    }
}

const fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Exciter => "exciter",
        NodeKind::ModalBank => "modal_bank",
        NodeKind::Oscillator => "oscillator",
        NodeKind::Unison => "unison",
        NodeKind::Noise => "noise",
        NodeKind::Envelope => "envelope",
        NodeKind::Mixer => "mixer",
        NodeKind::Lowpass => "lowpass",
        NodeKind::StereoFilter => "stereo_filter",
        NodeKind::Amplifier => "amplifier",
        NodeKind::Saturation => "saturation",
        NodeKind::Pan => "pan",
        NodeKind::Delay => "delay",
        NodeKind::Reverb => "reverb",
        NodeKind::Output => "output",
        NodeKind::Lfo => "lfo",
        NodeKind::MonoToStereo => "mono_to_stereo",
        NodeKind::StereoToMono => "stereo_to_mono",
        NodeKind::StereoMixer => "stereo_mixer",
        NodeKind::StereoSum => "stereo_sum",
        NodeKind::Sum => "sum",
        NodeKind::AnalogOscillator => "analog_oscillator",
        NodeKind::Wavetable => "wavetable",
        NodeKind::SampleSource => "sample_source",
        NodeKind::Multisample => "multisample",
        NodeKind::Granular => "granular",
        NodeKind::Spectral => "spectral",
        NodeKind::DelayLine => "delay_line",
        NodeKind::Math => "math",
        NodeKind::StereoMath => "stereo_math",
        NodeKind::Shaper => "shaper",
        NodeKind::ToAudio => "to_audio",
        NodeKind::ToControl => "to_control",
        NodeKind::DcBlock => "dc_block",
        NodeKind::LinearPhaseLowpass => "linear_phase_lowpass",
    }
}

/// Returns one same-frame dependency cycle, starting and ending at its smallest Node ID.
fn find_cycle<'a>(adjacency: &BTreeMap<&'a str, Vec<&'a str>>) -> Option<Vec<&'a str>> {
    let mut indegree: BTreeMap<&str, usize> = adjacency.keys().map(|node| (*node, 0)).collect();
    let mut predecessors: BTreeMap<&str, Vec<&str>> =
        adjacency.keys().map(|node| (*node, Vec::new())).collect();
    for (source, destinations) in adjacency {
        for destination in destinations {
            if let Some(count) = indegree.get_mut(destination) {
                *count += 1;
                predecessors.get_mut(destination)?.push(source);
            }
        }
    }
    let mut ready: VecDeque<_> = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(*node))
        .collect();
    while let Some(node) = ready.pop_front() {
        indegree.remove(node);
        if let Some(destinations) = adjacency.get(node) {
            for destination in destinations {
                if let Some(count) = indegree.get_mut(destination) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(destination);
                    }
                }
            }
        }
    }
    // Every remaining Node has a remaining predecessor, including downstream sinks.
    // Walking backwards must therefore revisit a Node on a cycle.
    let start = *indegree.keys().next()?;
    let mut path = vec![start];
    let mut node = start;
    loop {
        let next = predecessors
            .get(node)?
            .iter()
            .copied()
            .filter(|next| indegree.contains_key(next))
            .min()?;
        if let Some(position) = path.iter().position(|seen| *seen == next) {
            let mut cycle = path.split_off(position);
            cycle.reverse();
            let smallest = cycle
                .iter()
                .enumerate()
                .min_by_key(|(_, id)| **id)
                .map_or(0, |(index, _)| index);
            cycle.rotate_left(smallest);
            cycle.push(cycle[0]);
            return Some(cycle);
        }
        path.push(next);
        node = next;
    }
}

fn reverse_reachable<'a>(
    output: &'a str,
    reverse: &BTreeMap<&'a str, Vec<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![output];
    while let Some(node) = pending.pop() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(sources) = reverse.get(node) {
            pending.extend(sources.iter().copied());
        }
    }
    reachable
}
