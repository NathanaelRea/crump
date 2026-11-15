//! Whole-Project semantic and graph validation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, Severity, SourceLocation},
    media::{AssetInventory, valid_asset_path, valid_sha256},
    model::{
        InstrumentKind, MediaAssetUsage, NodeDirective, NodeKind, PatternEvent,
        PresentationItemKind, Project, SOURCE_VERSION_V1, SOURCE_VERSION_V2,
    },
};

mod controls;
mod instruments;
mod patterns;
mod routing;
mod tracks;

use controls::{validate_automations, validate_modulations};
pub(crate) use instruments::validate_graph_for_compile;
pub use instruments::validate_instrument_module;
use instruments::{validate_instruments, validate_runtime_graph_voices};
use patterns::validate_patterns;
use routing::validate_mix_graph;
use tracks::validate_tracks;

use crate::routing::AuthoredTopology;

/// Maximum authored graph nodes in one voice.
pub const MAX_GRAPH_NODES: u16 = 64;
/// Maximum graph voices across the Project.
pub const MAX_GRAPH_VOICES: u32 = 512;
/// Maximum Buses accepted by one explicit stereo summing input.
pub const MAX_STEREO_SUM_INPUTS: usize = 64;
/// Minimum power-of-two wavetable frame length.
pub const MIN_WAVETABLE_FRAME_LENGTH: u32 = 32;
/// Maximum power-of-two wavetable frame length.
pub const MAX_WAVETABLE_FRAME_LENGTH: u32 = 8_192;
/// Maximum authored frames in one wavetable asset.
pub const MAX_WAVETABLE_FRAMES: u16 = 256;
/// Maximum authored samples interpreted by one wavetable asset.
pub const MAX_WAVETABLE_SAMPLES: u64 = 2_097_152;
/// Maximum named slice regions on one graph sample source.
pub const MAX_SAMPLE_SLICES: usize = 256;
/// Maximum authored zones on one graph multisample source.
pub const MAX_MULTISAMPLE_ZONES: usize = 256;
/// Maximum sinusoidal peaks retained and resynthesized per spectral frame.
pub const MAX_SPECTRAL_PEAKS: usize = 128;
/// Maximum spectral source Nodes admitted in one voice graph.
pub const MAX_SPECTRAL_SOURCES_PER_VOICE: usize = 2;
/// Maximum explicit render tail.
pub const MAX_RENDER_TAIL_SECONDS: f64 = 30.0;
/// First-release supported render/playback sample rate.
pub const SUPPORTED_SAMPLE_RATE: u32 = 48_000;
/// Maximum Song Automation lanes admitted to the callback-safe control runtime.
pub const MAX_AUTOMATION_LANES: usize = 128;
/// Maximum points in one Song Automation lane.
pub const MAX_AUTOMATION_POINTS_PER_LANE: usize = 1_024;
/// Maximum points across every Song Automation lane.
pub const MAX_AUTOMATION_POINTS: usize = 8_192;
/// Maximum dense effective-value slots evaluated on every authored sample frame.
pub const MAX_CONTROL_TARGETS: usize = 1_024;
/// Maximum authored Instrument Modulation sources evaluated per sample.
pub const MAX_INSTRUMENT_MODULATION_SOURCES: usize = 64;
/// Maximum points in one `key_curve` Modulation source.
pub const MAX_KEY_CURVE_POINTS: usize = 64;
/// Maximum normalized macro Parameters exposed by one Instrument.
pub const MAX_INSTRUMENT_MACROS: usize = 8;
/// Maximum reusable Instrument-owned LFO source programs.
pub const MAX_INSTRUMENT_LFOS: usize = 16;
/// Maximum authored generated-view sections per Instrument.
pub const MAX_PRESENTATION_SECTIONS: usize = 16;
/// Maximum source-backed presentation references per Instrument.
pub const MAX_PRESENTATION_ITEMS: usize = 256;
/// Maximum searchable tags carried by one Instrument presentation.
pub const MAX_INSTRUMENT_TAGS: usize = 16;
/// Maximum authored Modulation routes evaluated per sample.
pub const MAX_MODULATION_ROUTES: usize = 256;
/// Maximum route-local arithmetic operations per Modulation route.
pub const MAX_MODULATION_OPERATORS_PER_ROUTE: usize = 16;
/// Maximum route arithmetic instructions evaluated per sample.
pub const MAX_MODULATION_OPERATORS: usize = 2_048;

/// Validates a complete Project candidate for publication.
#[must_use]
pub fn validate_project(project: &Project) -> Diagnostics {
    validate_project_with_topology(project).0
}

/// Validates while retaining the one canonical authored topology for runtime lowering.
pub(crate) fn validate_project_with_topology(project: &Project) -> (Diagnostics, AuthoredTopology) {
    let mut diagnostics = Diagnostics::new();
    validate_project_header(project, &mut diagnostics);
    validate_acoustic_profiles(project, &mut diagnostics);
    validate_media_assets(project, &mut diagnostics);
    validate_graph_assets(project, &mut diagnostics);
    validate_presentation_assets(project, &mut diagnostics);
    let topology = AuthoredTopology::compile(project);
    validate_instruments(&project.song.instruments, &mut diagnostics);
    validate_patterns(project, &mut diagnostics);
    validate_tracks(project, &topology, &mut diagnostics);
    validate_mix_graph(project, &topology, &mut diagnostics);
    validate_runtime_graph_voices(project, &mut diagnostics);
    validate_automations(project, &mut diagnostics);
    validate_modulations(project, &mut diagnostics);
    if !diagnostics.has_errors()
        && let Err(error) = crate::engine::schedule::compile_schedules(project)
    {
        diagnostics.push(
            Diagnostic::error(
                "CRV091",
                format!("runtime resource bound exceeded: {error}"),
            )
            .with_help("reduce Tracks, dense simultaneous Events, or loop-expanded Clip length"),
        );
    }
    if !diagnostics.has_errors() {
        match crate::engine::mix_graph::CompiledMixGraph::resource_estimate_for_validation(
            project,
            &topology,
        ) {
            Ok(estimate) if !estimate.admitted => diagnostics.push(
                Diagnostic::error(
                    "CRV092",
                    format!(
                        "compiled mix runtime exceeds a named budget: {} processors (fixed telemetry capacity {}), {} bytes at publication (budget {}), {} work units/frame (budget {})",
                        estimate.processors,
                        crate::engine::mix_graph::MAX_PROCESSOR_INSTANCES,
                        estimate.publication_peak_bytes,
                        estimate.publication_budget_bytes,
                        estimate.work_units_per_frame,
                        estimate.work_budget_per_frame
                    ),
                )
                .with_help(
                    "reduce processor state, channel count, graph voices, or callback-time work",
                ),
            ),
            Err(error) => diagnostics.push(
                Diagnostic::error("CRV092", format!("compiled mix graph rejected candidate: {error}"))
                    .with_help("inspect routing, processor contracts, and runtime resource bounds"),
            ),
            Ok(_) => {}
        }
    }
    (diagnostics, topology)
}

pub(crate) fn validate_acoustic_profiles(project: &Project, diagnostics: &mut Diagnostics) {
    let mut profiles = BTreeMap::new();
    for profile in &project.acoustic_profiles {
        if !valid_id(&profile.id) || profiles.insert(profile.id.as_str(), profile).is_some() {
            diagnostics.push(
                Diagnostic::error(
                    "CRV210",
                    "acoustic profile identities must be valid and unique",
                )
                .at(profile.location.clone()),
            );
        }
        if let Err(error) = profile.body.validate() {
            diagnostics.push(Diagnostic::error("CRV210", error).at(profile.location.clone()));
        }
    }
    if project.acoustic_profiles.len() > 256 {
        diagnostics.push(Diagnostic::error(
            "CRV210",
            "at most 256 acoustic profiles are admitted",
        ));
    }
    for node in project
        .song
        .instruments
        .iter()
        .flat_map(|i| i.graph.iter().chain(i.shared_graph.iter()))
        .flat_map(|g| &g.nodes)
        .filter(|n| n.kind == NodeKind::ModalBank)
    {
        let Some(crate::model::SettingValue::Text(id)) = node.setting("profile") else {
            diagnostics.push(
                Diagnostic::error("CRV210", "modal_bank requires a named profile")
                    .at(node.location.clone()),
            );
            continue;
        };
        match profiles.get(id.as_str()) {
            None => diagnostics.push(
                Diagnostic::error("CRV210", format!("missing acoustic profile `{id}`"))
                    .at(node.location.clone()),
            ),
            Some(profile) => {
                let max = match node.setting("max_modes") {
                    Some(crate::model::SettingValue::Number(n)) => *n,
                    _ => 64.0,
                };
                if profile.body.modes.len() as f64 > max {
                    diagnostics.push(
                        Diagnostic::error("CRV210", "acoustic profile exceeds max_modes")
                            .at(node.location.clone()),
                    );
                }
            }
        }
    }
}

/// Validates sample source regions against verified exact-byte WAV metadata.
#[must_use]
pub fn validate_project_assets(project: &Project, inventory: &AssetInventory) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    for declaration in &project.assets {
        if let Some(asset) = inventory.get(&declaration.id)
            && let Err(message) = crate::media::validate_finite_wav_samples(asset)
        {
            located_error(
                &mut diagnostics,
                "CRV199",
                format!(
                    "media asset `{}` contains invalid sample data: {message}",
                    declaration.id
                ),
                &declaration.location,
            );
        }
        if let Some(wavetable) = &declaration.wavetable {
            match inventory.get(&declaration.id) {
                None => located_error(
                    &mut diagnostics,
                    "CRV197",
                    format!(
                        "wavetable asset `{}` is not loaded from its exact declared bytes",
                        declaration.id
                    ),
                    &declaration.location,
                ),
                Some(asset) if asset.metadata().channels != 1 => located_error(
                    &mut diagnostics,
                    "CRV197",
                    format!(
                        "wavetable asset `{}` must be a mono WAV; found {} channels",
                        declaration.id,
                        asset.metadata().channels
                    ),
                    &declaration.location,
                ),
                Some(asset) => {
                    let expected = u64::from(wavetable.frame_length)
                        .saturating_mul(u64::from(wavetable.frames));
                    if asset.metadata().frames != expected {
                        located_error(
                            &mut diagnostics,
                            "CRV197",
                            format!(
                                "wavetable asset `{}` declares {} frames × {} samples = {expected}, but the WAV contains {} frames",
                                declaration.id,
                                wavetable.frames,
                                wavetable.frame_length,
                                asset.metadata().frames
                            ),
                            &declaration.location,
                        );
                    }
                }
            }
        }
        if declaration.generated.is_some()
            && let Some(asset) = inventory.get(&declaration.id)
            && asset.metadata().sample_rate != project.sample_rate
        {
            located_error(
                &mut diagnostics,
                "CRV147",
                format!(
                    "generated asset `{}` sample rate {} does not match Project sample rate {}",
                    declaration.id,
                    asset.metadata().sample_rate,
                    project.sample_rate
                ),
                &declaration.location,
            );
        }
    }
    for instrument in &project.song.instruments {
        for node in instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .flat_map(|graph| &graph.nodes)
        {
            match node.kind {
                NodeKind::SampleSource => {
                    let Some(crate::model::SettingValue::Text(asset_id)) = node.setting("asset")
                    else {
                        continue;
                    };
                    let Some(asset) = inventory.get(asset_id) else {
                        continue;
                    };
                    for directive in &node.directives {
                        if directive.tokens.len() == 4
                            && directive.tokens[0].value == "slice"
                            && let Ok(end) = directive.tokens[3].value.parse::<u64>()
                            && end > asset.metadata().frames
                        {
                            source_site_error(
                                &mut diagnostics,
                                "CRV198",
                                format!(
                                    "sample slice `{}` ends at frame {end}, beyond asset `{asset_id}` length {}",
                                    directive.tokens[1].value,
                                    asset.metadata().frames
                                ),
                                &directive.tokens[3].site,
                            );
                        }
                    }
                }
                NodeKind::Multisample => {
                    for zone in node
                        .directives
                        .iter()
                        .filter_map(NodeDirective::zone_clauses)
                    {
                        let Some(asset) = inventory.get(&zone.asset.value) else {
                            continue;
                        };
                        let frames = asset.metadata().frames;
                        // An explicit region must fit the asset; a whole-asset
                        // zone's loop must end within it.
                        let end = zone
                            .region
                            .map(|[_, end]| end)
                            .or(zone.loop_region.map(|[_, end, _]| end));
                        if let Some(end) = end
                            && let Ok(frame) = end.value.parse::<u64>()
                            && frame > frames
                        {
                            source_site_error(
                                &mut diagnostics,
                                "CRV198",
                                format!(
                                    "multisample zone `{}` ends at frame {frame}, beyond asset `{}` length {frames}",
                                    zone.id.value, zone.asset.value
                                ),
                                &end.site,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for instrument in &project.song.instruments {
        let Some(sampler) = &instrument.sampler else {
            continue;
        };
        for mapping in &sampler.mappings {
            match inventory.get(&mapping.asset) {
                None => located_error(
                    &mut diagnostics,
                    "CRV143",
                    format!(
                        "sample mapping `{}` asset `{}` is not loaded",
                        mapping.id, mapping.asset
                    ),
                    &mapping.location,
                ),
                Some(asset) if mapping.end_frame > asset.metadata().frames => source_site_error(
                    &mut diagnostics,
                    "CRV144",
                    format!(
                        "sample mapping `{}` ends at frame {}, beyond asset `{}` length {}",
                        mapping.id,
                        mapping.end_frame,
                        mapping.asset,
                        asset.metadata().frames
                    ),
                    &mapping.end_site,
                ),
                Some(_) => {}
            }
        }
    }
    let instruments = project
        .song
        .instruments
        .iter()
        .map(|instrument| (instrument.id.as_str(), instrument))
        .collect::<BTreeMap<_, _>>();
    let patterns = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect::<BTreeMap<_, _>>();
    for track in &project.song.tracks {
        let Some(sampler) = instruments
            .get(track.instrument.as_str())
            .and_then(|instrument| instrument.sampler.as_ref())
        else {
            continue;
        };
        let mappings = sampler
            .mappings
            .iter()
            .map(|mapping| (mapping.id.as_str(), mapping))
            .collect::<BTreeMap<_, _>>();
        for clip in &track.clips {
            let Some(pattern) = patterns.get(clip.pattern.as_str()) else {
                continue;
            };
            for event in &pattern.events {
                let PatternEvent::Sample {
                    mapping,
                    start_frame,
                    end_frame,
                    loop_region,
                    ..
                } = event
                else {
                    continue;
                };
                let Some(authored_mapping) = mappings.get(mapping.as_str()) else {
                    continue;
                };
                if let Some(asset) = inventory.get(&authored_mapping.asset)
                    && *end_frame > asset.metadata().frames
                {
                    source_site_error(
                        &mut diagnostics,
                        "CRV145",
                        format!(
                            "sample occurrence ends at frame {end_frame}, beyond asset `{}` length {}",
                            authored_mapping.asset,
                            asset.metadata().frames
                        ),
                        event.site(),
                    );
                }
                if let Some(effective_loop) = loop_region.or(authored_mapping.loop_region)
                    && (effective_loop.start_frame < *start_frame
                        || effective_loop.end_frame > *end_frame)
                {
                    source_site_error(
                        &mut diagnostics,
                        "CRV146",
                        "the effective sample loop lies outside this occurrence's source region",
                        event.site(),
                    );
                }
            }
        }
    }
    if !diagnostics.has_errors() {
        let topology = AuthoredTopology::compile(project);
        if let (Ok(sample_bytes), Ok(mut estimate)) = (
            crate::engine::sample::CompiledSampleLibrary::estimated_allocation_bytes_for_project(
                project,
                inventory,
                project.sample_rate,
            ),
            crate::engine::mix_graph::CompiledMixGraph::resource_estimate_for_validation(
                project, &topology,
            ),
        ) {
            estimate.include_additional_instrument_program_bytes(sample_bytes);
            if !estimate.admitted {
                diagnostics.push(
                    Diagnostic::error(
                        "CRV092",
                        format!(
                            "compiled exact-asset runtime exceeds a named budget: {} bytes at publication (budget {})",
                            estimate.publication_peak_bytes, estimate.publication_budget_bytes
                        ),
                    )
                    .with_help(
                        "reduce referenced sample duration, graph voices, processor state, or Track count",
                    ),
                );
            }
        }
    }
    diagnostics
}

/// Runs objective Musical Sketch checks beyond ordinary publication validity.
#[must_use]
pub fn check_musical_sketch(project: &Project) -> CheckReport {
    let diagnostics = validate_project(project);
    let duration_seconds = project.song.end.as_f64() * 60.0 / project.song.tempo_bpm;
    let has_graph = project
        .song
        .instruments
        .iter()
        .any(|instrument| instrument.kind == InstrumentKind::Graph);
    let has_drums = project
        .song
        .instruments
        .iter()
        .any(|instrument| instrument.kind == InstrumentKind::StockDrums);
    let patterns: BTreeMap<_, _> = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect();
    let track_has_event = |track: &crate::model::Track, pitched: bool| {
        track.clips.iter().any(|clip| {
            patterns.get(clip.pattern.as_str()).is_some_and(|pattern| {
                pattern.events.iter().any(|event| {
                    event.at() < clip.duration
                        && if pitched {
                            matches!(event, PatternEvent::Note { .. })
                        } else {
                            matches!(event, PatternEvent::Hit { .. })
                        }
                })
            })
        })
    };
    let graph_track = project.song.tracks.iter().any(|track| {
        project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            .is_some_and(|instrument| {
                instrument.kind == InstrumentKind::Graph
                    && instrument.polyphony > 1
                    && track_has_event(track, true)
            })
    });
    let drum_track = project.song.tracks.iter().any(|track| {
        project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            .is_some_and(|instrument| {
                instrument.kind == InstrumentKind::StockDrums && track_has_event(track, false)
            })
    });
    let section_variation = project.song.tracks.iter().any(|track| {
        let patterns: BTreeSet<_> = track.clips.iter().map(|clip| &clip.pattern).collect();
        patterns.len() > 1
    });
    let required_nodes: BTreeSet<NodeKind> = [
        NodeKind::Lowpass,
        NodeKind::Saturation,
        NodeKind::Delay,
        NodeKind::Reverb,
    ]
    .into_iter()
    .collect();
    let active_graph_instruments: BTreeSet<&str> = project
        .song
        .tracks
        .iter()
        .filter(|track| track_has_event(track, true))
        .filter_map(|track| {
            project
                .song
                .instruments
                .iter()
                .find(|instrument| instrument.id == track.instrument)
        })
        .filter(|instrument| instrument.kind == InstrumentKind::Graph)
        .map(|instrument| instrument.id.as_str())
        .collect();
    let present_nodes: BTreeSet<NodeKind> = project
        .song
        .instruments
        .iter()
        .filter(|instrument| active_graph_instruments.contains(instrument.id.as_str()))
        .filter_map(|instrument| instrument.graph.as_ref())
        .flat_map(|graph| graph.nodes.iter().map(|node| node.kind))
        .collect();
    let creative_processing = required_nodes.is_subset(&present_nodes);

    let mut checks = vec![
        CheckResult::new(
            "valid_project",
            !diagnostics.has_errors(),
            "Project parses and satisfies publication semantics",
        ),
        CheckResult::new(
            "duration_30_to_60_seconds",
            (30.0..=60.0).contains(&duration_seconds),
            format!("arrangement duration is {duration_seconds:.3} seconds"),
        ),
        CheckResult::new(
            "graph_instrument",
            has_graph && graph_track,
            "a Track uses a polyphonic graph-authored pitched instrument",
        ),
        CheckResult::new(
            "stock_percussion",
            has_drums && drum_track,
            "a Track uses stock synthesized percussion",
        ),
        CheckResult::new(
            "section_variation",
            section_variation,
            "at least one Track places more than one Pattern",
        ),
        CheckResult::new(
            "creative_processing",
            creative_processing,
            "the graph uses low-pass filtering, saturation, delay, and reverb",
        ),
        CheckResult::new(
            "no_media_assets",
            project.assets.is_empty(),
            "the Project declares no binary media assets",
        ),
    ];
    checks.sort_by(|lhs, rhs| lhs.id.cmp(&rhs.id));
    CheckReport {
        passed: checks.iter().all(|check| check.passed),
        checks,
        diagnostics: diagnostics.into_vec(),
    }
}

/// Machine-readable objective check report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    /// True only when every check passes.
    pub passed: bool,
    /// Named checks in stable order.
    pub checks: Vec<CheckResult>,
    /// Underlying publication diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// One objective release-fixture check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Stable check key.
    pub id: String,
    /// Pass/fail result.
    pub passed: bool,
    /// Human-readable evidence.
    pub detail: String,
}

impl CheckResult {
    fn new(id: impl Into<String>, passed: bool, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            passed,
            detail: detail.into(),
        }
    }
}

fn validate_graph_assets(project: &Project, diagnostics: &mut Diagnostics) {
    let assets = project
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset))
        .collect::<BTreeMap<_, _>>();
    for instrument in &project.song.instruments {
        for node in instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .flat_map(|graph| &graph.nodes)
        {
            match node.kind {
                NodeKind::Wavetable
                | NodeKind::SampleSource
                | NodeKind::Granular
                | NodeKind::Spectral => {
                    let Some(setting) = node.settings.iter().find(|setting| setting.key == "asset")
                    else {
                        located_error(
                            diagnostics,
                            "CRV196",
                            format!(
                                "{} node `{}` requires one Project-owned asset setting",
                                match node.kind {
                                    NodeKind::Wavetable => "wavetable",
                                    NodeKind::Granular => "granular",
                                    NodeKind::Spectral => "spectral",
                                    _ => "sample_source",
                                },
                                node.id
                            ),
                            &node.location,
                        );
                        continue;
                    };
                    let crate::model::SettingValue::Text(asset_id) = &setting.value else {
                        continue;
                    };
                    match assets.get(asset_id.as_str()) {
                        None => source_site_error(
                            diagnostics,
                            "CRV196",
                            format!(
                                "{} node `{}` references missing Project asset `{asset_id}`",
                                match node.kind {
                                    NodeKind::Wavetable => "wavetable",
                                    NodeKind::Granular => "granular",
                                    NodeKind::Spectral => "spectral",
                                    _ => "sample_source",
                                },
                                node.id
                            ),
                            &setting.value_site,
                        ),
                        Some(asset)
                            if node.kind == NodeKind::Wavetable
                                && (!asset.usages.contains(&MediaAssetUsage::Wavetable)
                                    || asset.wavetable.is_none()) =>
                        {
                            source_site_error(
                                diagnostics,
                                "CRV196",
                                format!(
                                    "asset `{asset_id}` is not declared with complete wavetable metadata"
                                ),
                                &setting.value_site,
                            );
                        }
                        Some(asset)
                            if node.kind == NodeKind::SampleSource
                                && !asset.usages.contains(&MediaAssetUsage::Sample) =>
                        {
                            source_site_error(
                                diagnostics,
                                "CRV196",
                                format!("asset `{asset_id}` is not declared for sample usage"),
                                &setting.value_site,
                            );
                        }
                        Some(asset)
                            if node.kind == NodeKind::Granular
                                && !asset.usages.contains(&MediaAssetUsage::Granular) =>
                        {
                            source_site_error(
                                diagnostics,
                                "CRV196",
                                format!("asset `{asset_id}` is not declared for granular usage"),
                                &setting.value_site,
                            );
                        }
                        Some(asset)
                            if node.kind == NodeKind::Spectral
                                && !asset.usages.contains(&MediaAssetUsage::Spectral) =>
                        {
                            source_site_error(
                                diagnostics,
                                "CRV196",
                                format!("asset `{asset_id}` is not declared for spectral usage"),
                                &setting.value_site,
                            );
                        }
                        Some(_) => {}
                    }
                }
                NodeKind::Multisample => {
                    for zone in node
                        .directives
                        .iter()
                        .filter_map(NodeDirective::zone_clauses)
                    {
                        let asset_id = zone.asset;
                        match assets.get(asset_id.value.as_str()) {
                            None => source_site_error(
                                diagnostics,
                                "CRV196",
                                format!(
                                    "multisample zone references missing Project asset `{}`",
                                    asset_id.value
                                ),
                                &asset_id.site,
                            ),
                            Some(asset)
                                if !asset.usages.contains(&MediaAssetUsage::Multisample) =>
                            {
                                source_site_error(
                                    diagnostics,
                                    "CRV196",
                                    format!(
                                        "asset `{}` is not declared for multisample usage",
                                        asset_id.value
                                    ),
                                    &asset_id.site,
                                );
                            }
                            Some(_) => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn validate_presentation_assets(project: &Project, diagnostics: &mut Diagnostics) {
    let assets = project
        .assets
        .iter()
        .map(|asset| asset.id.as_str())
        .collect::<BTreeSet<_>>();
    for instrument in &project.song.instruments {
        if project.source_version < SOURCE_VERSION_V2
            && !instrument.presentation.sections.is_empty()
        {
            located_error(
                diagnostics,
                "CRV194",
                "Instrument presentation metadata requires source version 2",
                &instrument.location,
            );
        }
        for item in instrument
            .presentation
            .sections
            .iter()
            .flat_map(|section| &section.items)
        {
            if item.kind == PresentationItemKind::Asset && !assets.contains(item.id.as_str()) {
                source_site_error(
                    diagnostics,
                    "CRV194",
                    format!(
                        "Instrument `{}` presentation references missing Project asset `{}`",
                        instrument.id, item.id
                    ),
                    &item.id_site,
                );
            }
        }
    }
}

fn validate_project_header(project: &Project, diagnostics: &mut Diagnostics) {
    if !matches!(
        project.source_version,
        SOURCE_VERSION_V1 | SOURCE_VERSION_V2
    ) {
        diagnostics.push(Diagnostic::error(
            "CRV189",
            format!(
                "Project source version {} is unsupported; expected {SOURCE_VERSION_V1} or {SOURCE_VERSION_V2}",
                project.source_version
            ),
        ));
    }
    if project.source_version == SOURCE_VERSION_V1 {
        for instrument in &project.song.instruments {
            if instrument
                .parameters
                .iter()
                .any(|parameter| parameter.macro_source)
                || !instrument.modulation_sources.is_empty()
                || !instrument.modulation_routes.is_empty()
                || instrument.shared_graph.is_some()
                || !instrument.voicing.is_legacy_default()
            {
                located_error(
                    diagnostics,
                    "CRV190",
                    "Instrument-owned Modulation, macro Parameters, shared graphs, and non-default voicing require source version 2",
                    &instrument.location,
                );
            }
        }
    }
    if !valid_id(&project.id) {
        diagnostics.push(Diagnostic::error(
            "CRV001",
            "Project id must start with an ASCII letter and contain only letters, digits, `_`, or `-`",
        ));
    }
    if project.sample_rate != SUPPORTED_SAMPLE_RATE {
        diagnostics.push(
            Diagnostic::error(
                "CRV002",
                format!("sample_rate must be {SUPPORTED_SAMPLE_RATE} for the first public release"),
            )
            .with_help("change the Project's `sample_rate` directive to 48000"),
        );
    }
    if !project.render_tail_seconds.is_finite()
        || !(0.0..=MAX_RENDER_TAIL_SECONDS).contains(&project.render_tail_seconds)
    {
        diagnostics.push(Diagnostic::error(
            "CRV003",
            format!(
                "render_tail must be finite and between 0 and {MAX_RENDER_TAIL_SECONDS} seconds"
            ),
        ));
    }
    if !project.song.tempo_bpm.is_finite() || !(20.0..=300.0).contains(&project.song.tempo_bpm) {
        diagnostics.push(Diagnostic::error(
            "CRV004",
            "tempo must be between 20 and 300 BPM",
        ));
    }
    if !project.song.end.is_positive() {
        diagnostics.push(Diagnostic::error("CRV005", "Song end must be positive"));
    }
    let duration_seconds = project.song.end.as_f64() * 60.0 / project.song.tempo_bpm;
    if duration_seconds > 1_800.0 {
        diagnostics.push(Diagnostic::error(
            "CRV006",
            "Song exceeds the bounded first-release maximum of 30 minutes",
        ));
    }
}

fn validate_media_assets(project: &Project, diagnostics: &mut Diagnostics) {
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for asset in &project.assets {
        if !valid_id(&asset.id) || !ids.insert(asset.id.as_str()) {
            located_error(
                diagnostics,
                "CRV140",
                format!("media asset id `{}` is invalid or duplicated", asset.id),
                &asset.location,
            );
        }
        if !valid_asset_path(&asset.path) || !paths.insert(asset.path.as_path()) {
            located_error(
                diagnostics,
                "CRV141",
                format!(
                    "media asset path `{}` is unsafe, non-WAV, or duplicated",
                    asset.path.display()
                ),
                &asset.location,
            );
        }
        if !valid_sha256(&asset.sha256) {
            located_error(
                diagnostics,
                "CRV142",
                format!(
                    "media asset `{}` SHA-256 must be 64 lowercase hexadecimal characters",
                    asset.id
                ),
                &asset.location,
            );
        }
        if project.source_version < SOURCE_VERSION_V2
            && (!asset.usages.is_empty() || asset.wavetable.is_some())
        {
            located_error(
                diagnostics,
                "CRV196",
                "asset synthesis metadata requires source version 2",
                &asset.location,
            );
        }
        let unique_usages = asset.usages.iter().copied().collect::<BTreeSet<_>>();
        if unique_usages.len() != asset.usages.len() {
            located_error(
                diagnostics,
                "CRV196",
                format!("media asset `{}` repeats a synthesis usage", asset.id),
                &asset.location,
            );
        }
        let declares_wavetable = asset.usages.contains(&MediaAssetUsage::Wavetable);
        if declares_wavetable != asset.wavetable.is_some() {
            located_error(
                diagnostics,
                "CRV196",
                format!(
                    "media asset `{}` must pair `usage wavetable` with complete wavetable metadata",
                    asset.id
                ),
                &asset.location,
            );
        }
        if let Some(wavetable) = &asset.wavetable {
            if !wavetable.frame_length.is_power_of_two()
                || !(MIN_WAVETABLE_FRAME_LENGTH..=MAX_WAVETABLE_FRAME_LENGTH)
                    .contains(&wavetable.frame_length)
            {
                source_site_error(
                    diagnostics,
                    "CRV196",
                    format!(
                        "wavetable frame_length must be a power of two from {MIN_WAVETABLE_FRAME_LENGTH} to {MAX_WAVETABLE_FRAME_LENGTH}"
                    ),
                    &wavetable.frame_length_site,
                );
            }
            if !(1..=MAX_WAVETABLE_FRAMES).contains(&wavetable.frames) {
                source_site_error(
                    diagnostics,
                    "CRV196",
                    format!("wavetable frames must be from 1 to {MAX_WAVETABLE_FRAMES}"),
                    &wavetable.frames_site,
                );
            }
            let samples =
                u64::from(wavetable.frame_length).saturating_mul(u64::from(wavetable.frames));
            if samples > MAX_WAVETABLE_SAMPLES {
                located_error(
                    diagnostics,
                    "CRV196",
                    format!(
                        "wavetable asset `{}` declares {samples} samples; maximum is {MAX_WAVETABLE_SAMPLES}",
                        asset.id
                    ),
                    &asset.location,
                );
            }
        }
    }
}

fn valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn valid_key(value: &str) -> bool {
    valid_id(value)
}

fn located_error(
    diagnostics: &mut Diagnostics,
    code: &str,
    message: impl Into<String>,
    location: &SourceLocation,
) {
    diagnostics.push(Diagnostic::error(code, message).at(location.clone()));
}

fn source_site_error(
    diagnostics: &mut Diagnostics,
    code: &str,
    message: impl Into<String>,
    site: &crate::model::ValueSite,
) {
    diagnostics.push(Diagnostic::error(code, message).at(SourceLocation {
        file: site.file.clone(),
        start: site.start,
        end: site.end,
        line: 1,
        column: 1,
    }));
}

/// Returns only blocking diagnostics from a set.
pub fn errors(diagnostics: &Diagnostics) -> impl Iterator<Item = &Diagnostic> {
    diagnostics
        .as_slice()
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
}

#[cfg(test)]
mod tests;
