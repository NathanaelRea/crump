//! Whole-Project semantic and graph validation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, Severity, SourceLocation},
    media::{AssetInventory, valid_asset_path, valid_sha256},
    model::{
        AutomationInterpolation, ChannelStrip, ClipMode, Graph, Instrument, InstrumentKind,
        ModulationOperator, ModulationRate, ModulationScope, NodeKind, OutputTarget, Panner,
        ParameterTarget, Pattern, PatternEvent, Processor, ProcessorKind, Project, SettingValue,
        Unit, processor_setting_contract, processor_structural_setting_contract,
    },
};

/// Maximum authored graph nodes in one voice.
pub const MAX_GRAPH_NODES: u16 = 64;
/// Maximum graph voices across the Project.
pub const MAX_GRAPH_VOICES: u32 = 256;
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
pub const MAX_CONTROL_TARGETS: usize = 512;
/// Maximum authored Modulation routes evaluated per sample.
pub const MAX_MODULATION_ROUTES: usize = 256;
/// Maximum route-local arithmetic operations per Modulation route.
pub const MAX_MODULATION_OPERATORS_PER_ROUTE: usize = 16;
/// Maximum route arithmetic instructions evaluated per sample.
pub const MAX_MODULATION_OPERATORS: usize = 2_048;

/// Validates a complete Project candidate for publication.
#[must_use]
pub fn validate_project(project: &Project) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    validate_project_header(project, &mut diagnostics);
    validate_media_assets(project, &mut diagnostics);
    validate_instruments(&project.song.instruments, &mut diagnostics);
    validate_patterns(project, &mut diagnostics);
    validate_tracks(project, &mut diagnostics);
    validate_mix_graph(project, &mut diagnostics);
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
        match crate::engine::mix_graph::CompiledMixGraph::resource_estimate_for_validation(project) {
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
    diagnostics
}

/// Validates sample source regions against verified exact-byte WAV metadata.
#[must_use]
pub fn validate_project_assets(project: &Project, inventory: &AssetInventory) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
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
                    mapping, end_frame, ..
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

fn validate_project_header(project: &Project, diagnostics: &mut Diagnostics) {
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
    }
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

fn validate_instruments(instruments: &[Instrument], diagnostics: &mut Diagnostics) {
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
        validate_parameters(instrument, diagnostics);
        validate_faceplate(instrument, diagnostics);
        match (instrument.kind, &instrument.graph) {
            (InstrumentKind::Graph, Some(graph)) => validate_graph(instrument, graph, diagnostics),
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
    }
    if instruments.is_empty() {
        diagnostics.push(Diagnostic::error(
            "CRV016",
            "Song must declare at least one instrument",
        ));
    }
}

fn validate_runtime_graph_voices(project: &Project, diagnostics: &mut Diagnostics) {
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
                    graph.nodes.iter().any(|node| {
                        node.settings.values().any(|setting| {
                            matches!(setting, SettingValue::Parameter(key) if key == &control.parameter)
                        })
                    })
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
        validate_node_settings(node, &parameters, diagnostics);
    }
    if outputs != 1 {
        located_error(
            diagnostics,
            "CRV043",
            format!("graph must contain exactly one output node; found {outputs}"),
            &graph.location,
        );
    }

    let mut occupied_inputs: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut adjacency: BTreeMap<&str, Vec<&str>> =
        nodes.keys().map(|id| (*id, Vec::new())).collect();
    let mut reverse: BTreeMap<&str, Vec<&str>> = nodes.keys().map(|id| (*id, Vec::new())).collect();
    for connection in &graph.connections {
        let from_type = output_type(&connection.from.node, &connection.from.port, &nodes);
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
        if !occupied_inputs.insert((connection.to.node.as_str(), connection.to.port.as_str())) {
            located_error(
                diagnostics,
                "CRV046",
                format!("input {} has more than one Bus", connection.to),
                &connection.location,
            );
        }
        if connection.from.node != "note" {
            if let Some(edges) = adjacency.get_mut(connection.from.node.as_str()) {
                edges.push(connection.to.node.as_str());
            }
            if let Some(edges) = reverse.get_mut(connection.to.node.as_str()) {
                edges.push(connection.from.node.as_str());
            }
        }
    }

    for node in &graph.nodes {
        for required in required_inputs(node.kind) {
            if !occupied_inputs.contains(&(node.id.as_str(), *required)) {
                located_error(
                    diagnostics,
                    "CRV047",
                    format!("node `{}` requires input `{required}`", node.id),
                    &node.location,
                );
            }
        }
    }
    if contains_cycle(&adjacency) {
        located_error(
            diagnostics,
            "CRV048",
            "graph contains a routing cycle",
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
    for (key, value) in &node.settings {
        let Some(expected) = schema
            .iter()
            .find(|spec| spec.key == key)
            .map(|spec| spec.kind)
        else {
            located_error(
                diagnostics,
                "CRV050",
                format!(
                    "unknown `{key}` setting for {} node",
                    node_kind_name(node.kind)
                ),
                &node.location,
            );
            continue;
        };
        match (expected, value) {
            (SettingKind::Text(allowed), SettingValue::Text(text))
                if allowed.contains(&text.as_str()) => {}
            (SettingKind::Text(allowed), _) => located_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be one of {}", allowed.join(", ")),
                &node.location,
            ),
            (SettingKind::Integer { minimum, maximum }, SettingValue::Number(number))
                if number.is_finite()
                    && number.fract() == 0.0
                    && *number >= f64::from(minimum)
                    && *number <= f64::from(maximum) => {}
            (SettingKind::Integer { minimum, maximum }, _) => located_error(
                diagnostics,
                "CRV051",
                format!("setting `{key}` must be an integer from {minimum} to {maximum}"),
                &node.location,
            ),
            (SettingKind::Number(unit), SettingValue::Parameter(parameter_key)) => {
                match parameters.get(parameter_key.as_str()) {
                    Some(parameter) if parameter.unit == unit => {
                        validate_setting_domain(
                            node,
                            key,
                            parameter.value,
                            Some((parameter.minimum, parameter.maximum, &parameter.key)),
                            diagnostics,
                        );
                    }
                    Some(parameter) => located_error(
                        diagnostics,
                        "CRV052",
                        format!(
                            "setting `{key}` expects {unit}, but Parameter `{}` uses {}",
                            parameter.key, parameter.unit
                        ),
                        &node.location,
                    ),
                    None => located_error(
                        diagnostics,
                        "CRV053",
                        format!("setting `{key}` references missing Parameter `{parameter_key}`"),
                        &node.location,
                    ),
                }
            }
            (SettingKind::Number(unit), SettingValue::Number(number)) => {
                if !number.is_finite() {
                    located_error(
                        diagnostics,
                        "CRV054",
                        "node setting must be finite",
                        &node.location,
                    );
                } else if !matches!(unit, Unit::Ratio | Unit::Pan | Unit::Semitones) {
                    located_error(
                        diagnostics,
                        "CRV056",
                        format!("setting `{key}` requires an explicit finite {unit} unit suffix"),
                        &node.location,
                    );
                } else {
                    validate_setting_domain(node, key, *number, None, diagnostics);
                }
            }
            (SettingKind::Number(unit), SettingValue::Text(text)) => match unit.parse_value(text) {
                Ok(number) => validate_setting_domain(node, key, number, None, diagnostics),
                Err(_) => located_error(
                    diagnostics,
                    "CRV056",
                    format!("setting `{key}` requires a finite {unit} value"),
                    &node.location,
                ),
            },
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

    const fn at_least(minimum: f64) -> Self {
        Self {
            minimum,
            maximum: None,
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
        (
            NodeKind::Oscillator | NodeKind::Unison | NodeKind::Noise | NodeKind::Amplifier,
            "level",
        )
        | (NodeKind::Mixer, "a_level" | "b_level") => Some(NumericDomain::bounded(0.0, 4.0)),
        (NodeKind::Oscillator, "detune") => Some(NumericDomain::bounded(-96.0, 96.0)),
        (NodeKind::Oscillator | NodeKind::Unison, "glide") => {
            Some(NumericDomain::bounded(0.0, 5.0))
        }
        (NodeKind::Unison, "detune") => Some(NumericDomain::bounded(0.0, 2.0)),
        (NodeKind::Unison, "drift") => Some(NumericDomain::bounded(0.0, 0.5)),
        (NodeKind::Unison, "mod_depth") => Some(NumericDomain::bounded(-24.0, 24.0)),
        (NodeKind::Unison, "width" | "velocity_amount") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::Envelope, "attack" | "decay") => Some(NumericDomain::at_least(0.0)),
        (NodeKind::Envelope, "sustain") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::Envelope, "release") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Lowpass, "cutoff" | "key_reference") => {
            Some(NumericDomain::bounded(10.0, 21_600.0))
        }
        (NodeKind::Lowpass, "mod_depth" | "velocity_depth") => {
            Some(NumericDomain::bounded(-21_600.0, 21_600.0))
        }
        (NodeKind::Lowpass, "resonance") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::Lowpass, "key_track") => Some(NumericDomain::bounded(0.0, 2.0)),
        (NodeKind::Saturation, "drive") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Pan, "position") => Some(NumericDomain::bounded(-1.0, 1.0)),
        (NodeKind::Delay, "time") => Some(NumericDomain::bounded(0.001, 2.0)),
        (NodeKind::Delay, "feedback") => Some(NumericDomain::bounded(0.0, 0.95)),
        (NodeKind::Delay | NodeKind::Reverb, "mix") | (NodeKind::Reverb, "room") => {
            Some(NumericDomain::bounded(0.0, 1.0))
        }
        (NodeKind::Reverb, "damping") => Some(NumericDomain::bounded(0.0, 0.95)),
        (NodeKind::Lfo, "rate") => Some(NumericDomain::bounded(0.01, 50.0)),
        (NodeKind::Lfo, "depth") => Some(NumericDomain::bounded(0.0, 1.0)),
        _ => None,
    }
}

fn validate_setting_domain(
    node: &crate::model::GraphNode,
    key: &str,
    value: f64,
    parameter_domain: Option<(f64, f64, &str)>,
    diagnostics: &mut Diagnostics,
) {
    let Some(domain) = setting_domain(node.kind, key) else {
        return;
    };
    if !domain.contains(value) {
        located_error(
            diagnostics,
            "CRV055",
            format!(
                "setting `{key}` value {value} is outside its supported {} range",
                domain.label()
            ),
            &node.location,
        );
    }
    if let Some((minimum, maximum, parameter)) = parameter_domain
        && (!domain.contains(minimum) || !domain.contains(maximum))
    {
        located_error(
            diagnostics,
            "CRV057",
            format!(
                "Parameter `{parameter}` domain {minimum}..={maximum} exceeds setting `{key}` range {}",
                domain.label()
            ),
            &node.location,
        );
    }
}

fn validate_patterns(project: &Project, diagnostics: &mut Diagnostics) {
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

fn validate_automations(project: &Project, diagnostics: &mut Diagnostics) {
    let control_targets = project
        .song
        .instruments
        .iter()
        .map(|instrument| instrument.parameters.len())
        .sum::<usize>()
        .saturating_add(project.mix_parameter_specs().len());
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

fn validate_modulations(project: &Project, diagnostics: &mut Diagnostics) {
    if project.song.modulations.len() > MAX_MODULATION_ROUTES {
        diagnostics.push(Diagnostic::error(
            "CRV119",
            format!(
                "Song declares {} Modulation routes; maximum is {MAX_MODULATION_ROUTES}",
                project.song.modulations.len()
            ),
        ));
    }
    let total_operators = project
        .song
        .modulations
        .iter()
        .map(|route| route.operators.len())
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
}

fn automation_target_is_runtime_safe(project: &Project, target: &ParameterTarget) -> bool {
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

fn validate_tracks(project: &Project, diagnostics: &mut Diagnostics) {
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

    let mut track_ids = BTreeSet::new();
    let mut clip_ids = BTreeSet::new();
    let mut instrument_clips = Vec::new();
    for track in &project.song.tracks {
        if !valid_id(&track.id) || !track_ids.insert(track.id.as_str()) {
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

fn validate_mix_graph(project: &Project, diagnostics: &mut Diagnostics) {
    let mut channel_locations: BTreeMap<String, &SourceLocation> = BTreeMap::new();
    for track in &project.song.tracks {
        channel_locations.insert(format!("track/{}", track.id), &track.location);
    }
    let mut group_ids = BTreeSet::new();
    for group in &project.song.groups {
        if !valid_id(&group.id) || !group_ids.insert(group.id.as_str()) {
            located_error(
                diagnostics,
                "CRV100",
                format!("Group id `{}` is invalid or duplicated", group.id),
                &group.location,
            );
        }
        channel_locations.insert(format!("group/{}", group.id), &group.location);
    }
    let mut return_ids = BTreeSet::new();
    for channel in &project.song.returns {
        if !valid_id(&channel.id) || !return_ids.insert(channel.id.as_str()) {
            located_error(
                diagnostics,
                "CRV101",
                format!("Return id `{}` is invalid or duplicated", channel.id),
                &channel.location,
            );
        }
        channel_locations.insert(format!("return/{}", channel.id), &channel.location);
    }
    channel_locations.insert("master".to_owned(), &project.song.location);

    let mut processor_ids = BTreeSet::new();
    let mut processor_owners = BTreeMap::new();
    for instrument in &project.song.instruments {
        for processor in &instrument.post_processors {
            validate_processor(processor, project, &mut processor_ids, diagnostics);
        }
    }
    for track in &project.song.tracks {
        let owner = format!("track/{}", track.id);
        validate_channel_strip(
            &owner,
            "Track",
            &track.channel,
            &track.location,
            project,
            &channel_locations,
            &mut processor_ids,
            &mut processor_owners,
            diagnostics,
        );
    }
    for group in &project.song.groups {
        let owner = format!("group/{}", group.id);
        validate_channel_strip(
            &owner,
            "Group",
            &group.channel,
            &group.location,
            project,
            &channel_locations,
            &mut processor_ids,
            &mut processor_owners,
            diagnostics,
        );
    }
    for channel in &project.song.returns {
        let owner = format!("return/{}", channel.id);
        validate_channel_strip(
            &owner,
            "Return",
            &channel.channel,
            &channel.location,
            project,
            &channel_locations,
            &mut processor_ids,
            &mut processor_owners,
            diagnostics,
        );
    }
    if let Some(master) = &project.song.master {
        if !master.level_db.is_finite() || !(-96.0..=12.0).contains(&master.level_db) {
            source_site_error(
                diagnostics,
                "CRV102",
                "Master level must be between -96db and 12db",
                &master.level_site,
            );
        }
        for processor in &master.inserts {
            validate_processor(processor, project, &mut processor_ids, diagnostics);
            processor_owners.insert(processor.id.as_str(), "master".to_owned());
        }
    }

    let mut send_ids = BTreeSet::new();
    let mut routes: BTreeMap<String, Vec<String>> = channel_locations
        .keys()
        .cloned()
        .map(|identity| (identity, Vec::new()))
        .collect();
    for (owner, channel, role, location) in project
        .song
        .tracks
        .iter()
        .map(|track| {
            (
                format!("track/{}", track.id),
                &track.channel,
                "Track",
                &track.location,
            )
        })
        .chain(project.song.groups.iter().map(|group| {
            (
                format!("group/{}", group.id),
                &group.channel,
                "Group",
                &group.location,
            )
        }))
        .chain(project.song.returns.iter().map(|channel| {
            (
                format!("return/{}", channel.id),
                &channel.channel,
                "Return",
                &channel.location,
            )
        }))
    {
        let destination = channel.output.to_string();
        if !channel_locations.contains_key(&destination) {
            located_error(
                diagnostics,
                "CRV103",
                format!("{role} `{owner}` routes to missing `{destination}`"),
                location,
            );
        } else {
            routes.entry(owner.clone()).or_default().push(destination);
        }
        if role != "Return" && matches!(channel.output, OutputTarget::Return(_)) {
            located_error(
                diagnostics,
                "CRV104",
                format!("{role} `{owner}` cannot use a Return as its main output"),
                location,
            );
        }
        for send in &channel.sends {
            if !valid_id(&send.id) || !send_ids.insert(send.id.as_str()) {
                located_error(
                    diagnostics,
                    "CRV105",
                    format!("Send id `{}` is invalid or duplicated", send.id),
                    &send.location,
                );
            }
            let destination = format!("return/{}", send.target);
            if !channel_locations.contains_key(&destination) {
                located_error(
                    diagnostics,
                    "CRV106",
                    format!(
                        "Send `{}` targets missing Return `{}`",
                        send.id, send.target
                    ),
                    &send.location,
                );
            } else {
                routes.entry(owner.clone()).or_default().push(destination);
            }
            if !send.level_db.is_finite() || !(-96.0..=12.0).contains(&send.level_db) {
                source_site_error(
                    diagnostics,
                    "CRV107",
                    "Send level must be between -96db and 12db",
                    &send.level_site,
                );
            }
        }
    }

    let mut sidechain_ids = BTreeSet::new();
    let mut sidechain_destinations = BTreeSet::new();
    for sidechain in &project.song.sidechains {
        if !valid_id(&sidechain.id) || !sidechain_ids.insert(sidechain.id.as_str()) {
            located_error(
                diagnostics,
                "CRV108",
                format!(
                    "sidechain route id `{}` is invalid or duplicated",
                    sidechain.id
                ),
                &sidechain.location,
            );
        }
        let source = sidechain.from.to_string();
        if !channel_locations.contains_key(&source) {
            located_error(
                diagnostics,
                "CRV109",
                format!(
                    "sidechain `{}` references missing source `{source}`",
                    sidechain.id
                ),
                &sidechain.location,
            );
        }
        let Some(owner) = processor_owners.get(sidechain.processor.as_str()) else {
            located_error(
                diagnostics,
                "CRV110",
                format!(
                    "sidechain `{}` references a missing or non-channel processor `{}`",
                    sidechain.id, sidechain.processor
                ),
                &sidechain.location,
            );
            continue;
        };
        let processor =
            channel_processors(project).find(|processor| processor.id == sidechain.processor);
        if !processor.is_some_and(|processor| {
            processor
                .sidechain_inputs
                .iter()
                .any(|input| input == &sidechain.input)
        }) {
            located_error(
                diagnostics,
                "CRV111",
                format!(
                    "sidechain `{}` targets undeclared input `{}` on processor `{}`",
                    sidechain.id, sidechain.input, sidechain.processor
                ),
                &sidechain.location,
            );
        }
        if !sidechain_destinations.insert((sidechain.processor.as_str(), sidechain.input.as_str()))
        {
            located_error(
                diagnostics,
                "CRV112",
                format!(
                    "processor `{}` input `{}` has more than one sidechain route",
                    sidechain.processor, sidechain.input
                ),
                &sidechain.location,
            );
        }
        routes.entry(source).or_default().push(owner.clone());
    }
    for processor in project
        .song
        .instruments
        .iter()
        .flat_map(|instrument| &instrument.post_processors)
        .chain(channel_processors(project))
    {
        for input in &processor.sidechain_inputs {
            if !sidechain_destinations.contains(&(processor.id.as_str(), input.as_str())) {
                located_error(
                    diagnostics,
                    "CRV135",
                    format!(
                        "processor `{}` sidechain input `{input}` has no route",
                        processor.id
                    ),
                    &processor.location,
                );
            }
        }
    }
    for destinations in routes.values_mut() {
        destinations.sort();
        destinations.dedup();
    }
    if mix_routes_have_cycle(&routes) {
        diagnostics.push(
            Diagnostic::error(
                "CRV113",
                "channel, Send, or sidechain routing contains a cycle",
            )
            .at(project.song.location.clone())
            .with_help(
                "route every main, Send, and auxiliary sidechain path acyclically toward Master",
            ),
        );
    }

    let mut locator_ids = BTreeSet::new();
    for locator in &project.song.locators {
        if !valid_id(&locator.id) || !locator_ids.insert(locator.id.as_str()) {
            located_error(
                diagnostics,
                "CRV114",
                format!("Locator id `{}` is invalid or duplicated", locator.id),
                &locator.location,
            );
        }
        if locator.at < crate::time::MusicalTime::ZERO || locator.at > project.song.end {
            source_site_error(
                diagnostics,
                "CRV115",
                "Locator time must be within the Song arrangement",
                &locator.at_site,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_channel_strip<'a>(
    owner: &str,
    role: &str,
    channel: &'a ChannelStrip,
    location: &SourceLocation,
    project: &Project,
    channel_locations: &BTreeMap<String, &SourceLocation>,
    processor_ids: &mut BTreeSet<&'a str>,
    processor_owners: &mut BTreeMap<&'a str, String>,
    diagnostics: &mut Diagnostics,
) {
    if !channel.level_db.is_finite() || !(-96.0..=12.0).contains(&channel.level_db) {
        source_site_error(
            diagnostics,
            "CRV116",
            format!("{role} level must be between -96db and 12db"),
            &channel.level_site,
        );
    }
    let positions = match &channel.panner {
        Panner::MonoPan { position, site } | Panner::Balance { position, site } => {
            vec![(*position, site)]
        }
        Panner::StereoPan {
            left,
            right,
            left_site,
            right_site,
        } => vec![(*left, left_site), (*right, right_site)],
    };
    for (position, site) in positions {
        if !position.is_finite() || !(-1.0..=1.0).contains(&position) {
            source_site_error(
                diagnostics,
                "CRV117",
                format!("{role} panner positions must be between -1 and 1"),
                site,
            );
        }
    }
    if !channel_locations.contains_key(&channel.output.to_string()) {
        located_error(
            diagnostics,
            "CRV118",
            format!("{role} `{owner}` has an unresolved main output"),
            location,
        );
    }
    for processor in &channel.inserts {
        validate_processor(processor, project, processor_ids, diagnostics);
        processor_owners.insert(processor.id.as_str(), owner.to_owned());
    }
}

fn validate_processor<'a>(
    processor: &'a Processor,
    project: &Project,
    processor_ids: &mut BTreeSet<&'a str>,
    diagnostics: &mut Diagnostics,
) {
    if !valid_id(&processor.id) || !processor_ids.insert(processor.id.as_str()) {
        located_error(
            diagnostics,
            "CRV119",
            format!("processor id `{}` is invalid or duplicated", processor.id),
            &processor.location,
        );
    }
    if u64::from(processor.latency_frames) > u64::from(project.sample_rate).saturating_mul(10) {
        located_error(
            diagnostics,
            "CRV120",
            format!(
                "processor `{}` declares more than ten seconds of latency",
                processor.id
            ),
            &processor.location,
        );
    }
    let mut keys = BTreeSet::new();
    for setting in &processor.settings {
        if !valid_key(&setting.key) || !keys.insert(setting.key.as_str()) {
            located_error(
                diagnostics,
                "CRV121",
                format!(
                    "processor `{}` setting key `{}` is invalid or duplicated",
                    processor.id, setting.key
                ),
                &setting.location,
            );
            continue;
        }
        if let Some((unit, minimum, maximum)) =
            processor_setting_contract(processor.kind, &setting.key)
                .or_else(|| processor_structural_setting_contract(processor.kind, &setting.key))
        {
            match unit.parse_value(&setting.value) {
                Ok(value) if value.is_finite() && (minimum..=maximum).contains(&value) => {}
                Ok(_) => source_site_error(
                    diagnostics,
                    "CRV122",
                    format!(
                        "processor `{}` setting `{}` must be within {}..={} {:?}",
                        processor.id, setting.key, minimum, maximum, unit
                    ),
                    &setting.value_site,
                ),
                Err(error) => source_site_error(
                    diagnostics,
                    "CRV123",
                    format!(
                        "processor `{}` setting `{}` has an invalid physical literal: {error}",
                        processor.id, setting.key
                    ),
                    &setting.value_site,
                ),
            }
        } else if !validate_discrete_processor_setting(processor.kind, &setting.key, &setting.value)
        {
            source_site_error(
                diagnostics,
                "CRV124",
                format!(
                    "processor `{}` kind {:?} does not accept `{} {}`",
                    processor.id, processor.kind, setting.key, setting.value
                ),
                &setting.value_site,
            );
        }
    }
    if processor.kind == ProcessorKind::Delay {
        let tempo_sync = processor
            .settings
            .iter()
            .find(|setting| setting.key == "tempo_sync")
            .is_some_and(|setting| setting.value == "true");
        if tempo_sync
            && let Some(time) = processor
                .settings
                .iter()
                .find(|setting| setting.key == "time")
        {
            source_site_error(
                diagnostics,
                "CRV136",
                format!(
                    "delay `{}` cannot author `time` while tempo sync is enabled",
                    processor.id
                ),
                &time.value_site,
            );
        }
        if !tempo_sync
            && let Some(division) = processor
                .settings
                .iter()
                .find(|setting| setting.key == "division")
        {
            source_site_error(
                diagnostics,
                "CRV136",
                format!(
                    "delay `{}` cannot author `division` unless tempo sync is enabled",
                    processor.id
                ),
                &division.value_site,
            );
        }
    }
    if processor.kind != ProcessorKind::Compressor && !processor.sidechain_inputs.is_empty() {
        located_error(
            diagnostics,
            "CRV133",
            format!(
                "processor `{}` kind {:?} does not consume sidechain Audio",
                processor.id, processor.kind
            ),
            &processor.location,
        );
    }
    if processor.kind == ProcessorKind::Compressor && processor.sidechain_inputs.len() > 1 {
        located_error(
            diagnostics,
            "CRV134",
            format!(
                "compressor `{}` accepts at most one named sidechain input",
                processor.id
            ),
            &processor.location,
        );
    }
    let mut inputs = BTreeSet::new();
    for input in &processor.sidechain_inputs {
        if !valid_key(input) || !inputs.insert(input.as_str()) {
            located_error(
                diagnostics,
                "CRV125",
                format!(
                    "processor `{}` sidechain input `{input}` is invalid or duplicated",
                    processor.id
                ),
                &processor.location,
            );
        }
    }
}

fn validate_discrete_processor_setting(kind: ProcessorKind, key: &str, value: &str) -> bool {
    match (kind, key, value) {
        (ProcessorKind::Utility, "polarity", "normal" | "invert")
        | (ProcessorKind::Utility, "mono", "true" | "false")
        | (ProcessorKind::Compressor, "detector", "peak" | "rms")
        | (ProcessorKind::Delay, "tempo_sync", "true" | "false")
        | (ProcessorKind::Delay, "mode", "stereo" | "ping_pong")
        | (ProcessorKind::Saturation, "mode", "soft_clip" | "tanh" | "tape") => true,
        (ProcessorKind::Delay, "division", value) => {
            crate::model::delay_division_beats(value).is_some()
        }
        (ProcessorKind::Latency, "frames", value) => value.parse::<u32>().is_ok(),
        _ => false,
    }
}

fn channel_processors(project: &Project) -> impl Iterator<Item = &Processor> {
    project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.channel.inserts)
        .chain(
            project
                .song
                .groups
                .iter()
                .flat_map(|group| &group.channel.inserts),
        )
        .chain(
            project
                .song
                .returns
                .iter()
                .flat_map(|channel| &channel.channel.inserts),
        )
        .chain(
            project
                .song
                .master
                .iter()
                .flat_map(|master| &master.inserts),
        )
}

fn mix_routes_have_cycle(routes: &BTreeMap<String, Vec<String>>) -> bool {
    fn visit(
        node: &str,
        routes: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        if visited.contains(node) {
            return false;
        }
        if !visiting.insert(node.to_owned()) {
            return true;
        }
        if routes.get(node).is_some_and(|destinations| {
            destinations
                .iter()
                .any(|destination| visit(destination, routes, visiting, visited))
        }) {
            return true;
        }
        visiting.remove(node);
        visited.insert(node.to_owned());
        false
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    routes
        .keys()
        .any(|node| visit(node, routes, &mut visiting, &mut visited))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalType {
    AudioMono,
    AudioStereo,
    Hertz,
    Gate,
    Unipolar,
    Bipolar,
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
            Self::Bipolar => "bipolar Control",
            Self::Control => "uni- or bipolar Control",
        }
    }
}

fn output_type(
    node: &str,
    port: &str,
    nodes: &BTreeMap<&str, &crate::model::GraphNode>,
) -> Result<SignalType, String> {
    if node == "note" {
        return match port {
            "pitch" => Ok(SignalType::Hertz),
            "gate" => Ok(SignalType::Gate),
            "velocity" => Ok(SignalType::Unipolar),
            _ => Err(format!("note source has no `{port}` output")),
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
        (
            NodeKind::Oscillator
            | NodeKind::Noise
            | NodeKind::Mixer
            | NodeKind::Lowpass
            | NodeKind::Amplifier
            | NodeKind::Saturation,
            "audio",
        ) => Some(SignalType::AudioMono),
        (NodeKind::Unison | NodeKind::Pan | NodeKind::Delay | NodeKind::Reverb, "audio") => {
            Some(SignalType::AudioStereo)
        }
        (NodeKind::Envelope, "control") => Some(SignalType::Unipolar),
        (NodeKind::Lfo, "control") => Some(SignalType::Bipolar),
        _ => None,
    }
}

fn node_input(kind: NodeKind, port: &str) -> Option<SignalType> {
    match (kind, port) {
        (NodeKind::Oscillator | NodeKind::Unison, "pitch") => Some(SignalType::Hertz),
        (NodeKind::Unison, "gain" | "velocity") => Some(SignalType::Unipolar),
        (NodeKind::Unison, "mod") => Some(SignalType::Control),
        (NodeKind::Envelope, "gate") => Some(SignalType::Gate),
        (NodeKind::Mixer, "a" | "b") => Some(SignalType::AudioMono),
        (
            NodeKind::Lowpass | NodeKind::Amplifier | NodeKind::Saturation | NodeKind::Pan,
            "audio",
        ) => Some(SignalType::AudioMono),
        (NodeKind::Lowpass, "key") => Some(SignalType::Hertz),
        (NodeKind::Lowpass, "mod") => Some(SignalType::Control),
        (NodeKind::Lowpass, "velocity") => Some(SignalType::Unipolar),
        (NodeKind::Amplifier, "gain") => Some(SignalType::Unipolar),
        (NodeKind::Delay | NodeKind::Reverb | NodeKind::Output, "audio") => {
            Some(SignalType::AudioStereo)
        }
        _ => None,
    }
}

const fn required_inputs(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Oscillator | NodeKind::Unison => &["pitch"],
        NodeKind::Noise | NodeKind::Lfo => &[],
        NodeKind::Envelope => &["gate"],
        NodeKind::Mixer => &["a"],
        NodeKind::Lowpass | NodeKind::Saturation | NodeKind::Pan => &["audio"],
        NodeKind::Amplifier => &["audio", "gain"],
        NodeKind::Delay | NodeKind::Reverb | NodeKind::Output => &["audio"],
    }
}

const fn signal_compatible(from: SignalType, to: SignalType) -> bool {
    from as u8 == to as u8
        || matches!(
            (from, to),
            (
                SignalType::Unipolar | SignalType::Bipolar,
                SignalType::Control
            )
        )
}

#[derive(Clone, Copy)]
struct SettingSpec {
    key: &'static str,
    kind: SettingKind,
}

#[derive(Clone, Copy)]
enum SettingKind {
    Number(Unit),
    Integer { minimum: u16, maximum: u16 },
    Text(&'static [&'static str]),
}

const fn setting_schema(kind: NodeKind) -> &'static [SettingSpec] {
    use SettingKind::{Integer, Number, Text};
    match kind {
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
        NodeKind::Noise => &[SettingSpec {
            key: "level",
            kind: Number(Unit::Ratio),
        }],
        NodeKind::Envelope => &[
            SettingSpec {
                key: "attack",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "decay",
                kind: Number(Unit::Seconds),
            },
            SettingSpec {
                key: "sustain",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "release",
                kind: Number(Unit::Seconds),
            },
        ],
        NodeKind::Mixer => &[
            SettingSpec {
                key: "a_level",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "b_level",
                kind: Number(Unit::Ratio),
            },
        ],
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
                kind: Number(Unit::Hertz),
            },
            SettingSpec {
                key: "depth",
                kind: Number(Unit::Ratio),
            },
            SettingSpec {
                key: "waveform",
                kind: Text(&["sine", "triangle"]),
            },
        ],
    }
}

const fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Oscillator => "oscillator",
        NodeKind::Unison => "unison",
        NodeKind::Noise => "noise",
        NodeKind::Envelope => "envelope",
        NodeKind::Mixer => "mixer",
        NodeKind::Lowpass => "lowpass",
        NodeKind::Amplifier => "amplifier",
        NodeKind::Saturation => "saturation",
        NodeKind::Pan => "pan",
        NodeKind::Delay => "delay",
        NodeKind::Reverb => "reverb",
        NodeKind::Output => "output",
        NodeKind::Lfo => "lfo",
    }
}

fn contains_cycle(adjacency: &BTreeMap<&str, Vec<&str>>) -> bool {
    let mut indegree: BTreeMap<&str, usize> = adjacency.keys().map(|node| (*node, 0)).collect();
    for destinations in adjacency.values() {
        for destination in destinations {
            if let Some(count) = indegree.get_mut(destination) {
                *count += 1;
            }
        }
    }
    let mut ready: VecDeque<_> = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(*node))
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop_front() {
        visited += 1;
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
    visited != adjacency.len()
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
mod tests {
    use std::path::Path;

    use crate::{dsl::parse_project, model::ChannelRef};

    use super::*;

    const VALID: &str = r#"crump 1
project "Validation" {
  id validation
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 4
      parameter tone "Tone" hz 100hz 10000hz 2000hz 2500hz
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
      graph 8 {
        node osc oscillator waveform saw
        node env envelope attack 10ms decay 100ms sustain 0.7 release 300ms
        node filter lowpass cutoff $tone resonance 0.1
        node amp amplifier
        node pan pan position 0
        node out output
        connect note.pitch -> osc.pitch
        connect note.gate -> env.gate
        connect osc.audio -> filter.audio
        connect filter.audio -> amp.audio
        connect env.control -> amp.gain
        connect amp.audio -> pan.audio
        connect pan.audio -> out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }
    track lead-track "Lead" instrument lead level -6db pan 0 {
      clip lead-clip pattern notes start 0bar duration 1bar mode once
    }
  }
}
"#;

    fn automation_project() -> Project {
        let source = VALID
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz automatable",
            )
            .replace(
                "    track lead-track \"Lead\" instrument lead level -6db pan 0 {",
                r#"    automation tone-sweep target instrument/lead/tone {
      point start 0bar 1000hz linear
      point open 2beat 8000hz power(2)
    }
    track lead-track "Lead" instrument lead level -6db pan 0 {"#,
            );
        let document = parse_project(Path::new("project.crump"), &source);
        assert!(!document.diagnostics.has_errors());
        document.project.expect("Automation Project")
    }

    #[test]
    fn accepts_valid_graph_and_timeline() {
        let document = parse_project(Path::new("project.crump"), VALID);
        assert!(!document.diagnostics.has_errors());
        let diagnostics = validate_project(document.project.as_ref().unwrap());
        for diagnostic in diagnostics.as_slice() {
            eprintln!("{diagnostic}");
        }
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn musical_sketch_role_requires_an_in_range_event() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let before = check_musical_sketch(&project);
        assert!(
            before
                .checks
                .iter()
                .find(|check| check.id == "graph_instrument")
                .expect("graph check")
                .passed
        );

        project.song.patterns[0].events.clear();
        let after = check_musical_sketch(&project);
        assert!(
            !after
                .checks
                .iter()
                .find(|check| check.id == "graph_instrument")
                .expect("graph check")
                .passed
        );
    }

    #[test]
    fn musical_sketch_rejects_declared_media_assets() {
        let source = VALID.replace(
            "  song {",
            concat!(
                "  asset voice \"Voice\" wav \"media/voice.wav\" sha256 \"",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "\"\n  song {"
            ),
        );
        let document = parse_project(Path::new("project.crump"), &source);
        assert!(!document.diagnostics.has_errors());
        let report = check_musical_sketch(document.project.as_ref().expect("Project"));
        assert!(
            !report
                .checks
                .iter()
                .find(|check| check.id == "no_media_assets")
                .expect("media check")
                .passed
        );
    }

    #[test]
    fn unused_effects_graph_does_not_satisfy_creative_processing() {
        let document = parse_project(
            Path::new("project.crump"),
            include_str!("../fixtures/reference/project.crump"),
        );
        let mut project = document.project.expect("Reference Project");
        project
            .song
            .tracks
            .iter_mut()
            .find(|track| track.id == "lead")
            .expect("lead Track")
            .instrument = "bass".to_owned();

        let report = check_musical_sketch(&project);
        assert!(
            report
                .checks
                .iter()
                .find(|check| check.id == "valid_project")
                .expect("validity check")
                .passed
        );
        assert!(
            report
                .checks
                .iter()
                .find(|check| check.id == "graph_instrument")
                .expect("graph check")
                .passed
        );
        assert!(
            !report
                .checks
                .iter()
                .find(|check| check.id == "creative_processing")
                .expect("processing check")
                .passed
        );
    }

    #[test]
    fn validates_automation_identity_time_domain_and_runtime_safety() {
        let project = automation_project();
        assert!(!validate_project(&project).has_errors());

        let mut invalid = project.clone();
        invalid.song.automations[0].points[0].at = crate::time::MusicalTime::new(1, 1).unwrap();
        invalid.song.automations[0].points[1].value = "20000hz".to_owned();
        let mut duplicate = invalid.song.automations[0].clone();
        duplicate.id = "duplicate-target".to_owned();
        invalid.song.automations.push(duplicate);
        let diagnostics = validate_project(&invalid);
        for code in ["CRV103", "CRV109", "CRV115"] {
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing {code}: {:?}",
                diagnostics.as_slice()
            );
        }

        let mut not_automatable = project.clone();
        not_automatable.song.instruments[0].parameters[0].automatable = false;
        assert!(
            validate_project(&not_automatable)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV105")
        );

        let mut allocation_changing = project;
        let parameter = &mut allocation_changing.song.instruments[0].parameters[0];
        parameter.unit = Unit::Seconds;
        parameter.minimum = 0.001;
        parameter.maximum = 2.0;
        parameter.default = 0.2;
        parameter.value = 0.2;
        let filter = allocation_changing.song.instruments[0]
            .graph
            .as_mut()
            .expect("graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "filter")
            .expect("filter");
        filter.kind = NodeKind::Delay;
        filter.settings = BTreeMap::from([
            (
                "time".to_owned(),
                SettingValue::Parameter("tone".to_owned()),
            ),
            ("feedback".to_owned(), SettingValue::Number(0.2)),
            ("mix".to_owned(), SettingValue::Number(0.5)),
        ]);
        allocation_changing.song.automations[0].points[0].value = "100ms".to_owned();
        allocation_changing.song.automations[0].points[1].value = "1s".to_owned();
        assert!(
            validate_project(&allocation_changing)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV106")
        );
    }

    #[test]
    fn validates_variation_lineage_pitch_and_parameter_lock_conflicts() {
        let source = VALID
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz automatable",
            )
            .replace(
                r#"    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }"#,
                r#"    pattern notes 1bar {
      lock bright at 0beat parameter tone value 8000hz
      note 0beat C4 1beat 0.8
    }
    pattern answer 1bar from notes {
      rotate 1beat
      transpose 12st
      velocity 0.8
      reverse false
      probability 1
      seed 77
    }"#,
            )
            .replace(
                "clip lead-clip pattern notes",
                "clip lead-clip pattern answer",
            );
        let document = parse_project(Path::new("project.crump"), &source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics.as_slice()
        );
        let valid = document.project.expect("Variation and lock Project");
        assert!(
            !validate_project(&valid).has_errors(),
            "valid Variation and lock Project"
        );

        let mut cumulative_pitch = valid.clone();
        let mut chained = cumulative_pitch
            .song
            .patterns
            .iter()
            .find(|pattern| pattern.id == "answer")
            .expect("answer Pattern")
            .clone();
        chained.id = "answer-two".to_owned();
        let variation = chained.variation.as_mut().expect("Variation lineage");
        variation.source = "answer".to_owned();
        variation.transpose_semitones = 56;
        cumulative_pitch.song.patterns.push(chained);
        cumulative_pitch.song.tracks[0].clips[0].pattern = "answer-two".to_owned();
        assert!(
            validate_project(&cumulative_pitch)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV088")
        );

        let mut cyclic = valid.clone();
        cyclic
            .song
            .patterns
            .iter_mut()
            .find(|pattern| pattern.id == "answer")
            .expect("answer Pattern")
            .variation
            .as_mut()
            .expect("Variation lineage")
            .source = "answer".to_owned();
        assert!(
            validate_project(&cyclic)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV160")
        );

        let mut overlapping = valid.clone();
        let mut duplicate = overlapping.song.tracks[0].clone();
        duplicate.id = "lead-track-two".to_owned();
        duplicate.name = "Lead Two".to_owned();
        duplicate.clips[0].id = "lead-clip-two".to_owned();
        overlapping.song.tracks.push(duplicate);
        assert!(
            validate_project(&overlapping)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV169")
        );

        let mut out_of_domain = valid;
        out_of_domain
            .song
            .patterns
            .iter_mut()
            .find(|pattern| pattern.id == "notes")
            .expect("source Pattern")
            .locks[0]
            .value = "20000hz".to_owned();
        assert!(
            validate_project(&out_of_domain)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV167")
        );
    }

    #[test]
    fn rejects_out_of_domain_parameter() {
        let invalid = VALID.replace("2500hz", "25000hz");
        let document = parse_project(Path::new("project.crump"), &invalid);
        let diagnostics = validate_project(document.project.as_ref().unwrap());
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV024")
        );
    }

    #[test]
    fn rejects_runtime_only_graph_setting_failures_before_publication() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let graph = project.song.instruments[0].graph.as_mut().expect("graph");
        let envelope = graph
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Envelope)
            .expect("envelope");
        envelope
            .settings
            .insert("release".to_owned(), SettingValue::Text("9s".to_owned()));
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV055")
        );
    }

    #[test]
    fn rejects_missing_physical_unit_suffixes() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let graph = project.song.instruments[0].graph.as_mut().expect("graph");
        let envelope = graph
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Envelope)
            .expect("envelope");
        envelope
            .settings
            .insert("attack".to_owned(), SettingValue::Number(0.01));
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV056")
        );
    }

    #[test]
    fn rejects_parameter_domains_that_exceed_mapped_setting_domain() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let instrument = &mut project.song.instruments[0];
        let parameter = &mut instrument.parameters[0];
        parameter.unit = Unit::Seconds;
        parameter.minimum = 0.0;
        parameter.maximum = 20.0;
        parameter.default = 0.3;
        parameter.value = 0.3;
        let envelope = instrument
            .graph
            .as_mut()
            .expect("graph")
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Envelope)
            .expect("envelope");
        envelope.settings.insert(
            "release".to_owned(),
            SettingValue::Parameter("tone".to_owned()),
        );
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV057")
        );
    }

    #[test]
    fn rejects_faceplate_controls_without_a_sound_mapping() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let graph = project.song.instruments[0].graph.as_mut().expect("graph");
        let filter = graph
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Lowpass)
            .expect("filter");
        filter
            .settings
            .insert("cutoff".to_owned(), SettingValue::Text("2000hz".to_owned()));
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV034")
        );
    }

    #[test]
    fn rejects_stock_drum_parameters_without_native_mappings() {
        let document = parse_project(
            Path::new("project.crump"),
            include_str!("../tests/fixtures/minimal.crump"),
        );
        let mut project = document.project.expect("Project");
        let drums = project
            .song
            .instruments
            .iter_mut()
            .find(|instrument| instrument.kind == InstrumentKind::StockDrums)
            .expect("drums");
        drums.parameters[0].key = "unused".to_owned();
        drums.parameters[0].maximum = 4.0;
        drums.faceplate[0].parameter = "unused".to_owned();
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV025")
        );
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV026")
        );
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV034")
        );
    }

    #[test]
    fn standalone_module_requires_exactly_one_instrument() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let instrument = document
            .project
            .expect("Project")
            .song
            .instruments
            .remove(0);
        let diagnostics = validate_instrument_module(&[instrument.clone(), instrument]);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV017")
        );
    }

    #[test]
    fn graph_voice_bound_counts_each_runtime_track_instance() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        project.song.instruments[0].polyphony = 64;
        let template = project.song.tracks[0].clone();
        project.song.tracks.clear();
        for index in 0..4 {
            let mut track = template.clone();
            track.id = format!("lead-{index}");
            track.name = format!("Lead {index}");
            track.clips[0].id = format!("lead-clip-{index}");
            project.song.tracks.push(track);
        }
        assert!(
            validate_project(&project)
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code != "CRV015"),
            "exactly {MAX_GRAPH_VOICES} runtime voices must be admitted"
        );

        let mut extra = template;
        extra.id = "lead-overflow".to_owned();
        extra.name = "Lead Overflow".to_owned();
        extra.clips[0].id = "lead-clip-overflow".to_owned();
        project.song.tracks.push(extra);
        assert!(
            validate_project(&project)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV015")
        );
    }

    #[test]
    fn rejects_unbounded_callback_time_parameter_targets() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let template = project.song.instruments[0].parameters[0].clone();
        for index in 0..510 {
            let mut parameter = template.clone();
            parameter.key = format!("extra-{index}");
            parameter.name = format!("Extra {index}");
            project.song.instruments[0].parameters.push(parameter);
        }
        assert!(
            validate_project(&project)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV118")
        );
    }

    #[test]
    fn routing_cycles_and_sidechain_contracts_have_focused_diagnostics() {
        let document = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        );
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");

        let mut send_cycle = project.clone();
        send_cycle.song.returns[0].channel.output = OutputTarget::Group("mix_bus".to_owned());
        let diagnostics = validate_project(&send_cycle);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| { diagnostic.code == "CRV113" && diagnostic.location.is_some() })
        );

        let mut sidechain_cycle = project.clone();
        sidechain_cycle.song.sidechains[0].from = ChannelRef::Group("mix_bus".to_owned());
        assert!(
            validate_project(&sidechain_cycle)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV113")
        );

        let mut missing_port = project.clone();
        missing_port.song.sidechains[0].input = "missing".to_owned();
        assert!(
            validate_project(&missing_port)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV111" && diagnostic.location.is_some())
        );

        let mut duplicate_destination = project;
        let mut duplicate = duplicate_destination.song.sidechains[0].clone();
        duplicate.id = "duplicate_detector".to_owned();
        duplicate_destination.song.sidechains.push(duplicate);
        assert!(
            validate_project(&duplicate_destination)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV112" && diagnostic.location.is_some())
        );
    }

    #[test]
    fn processor_contracts_reject_unimplemented_or_unconsumed_configuration() {
        let document = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        );
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");

        let mut disconnected_sidechain = project.clone();
        disconnected_sidechain.song.sidechains.clear();
        assert!(
            validate_project(&disconnected_sidechain)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV135" && diagnostic.location.is_some())
        );

        let mut wrong_sidechain_kind = project.clone();
        wrong_sidechain_kind.song.returns[0].channel.inserts[0]
            .sidechain_inputs
            .push("detector".to_owned());
        assert!(
            validate_project(&wrong_sidechain_kind)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV133" && diagnostic.location.is_some())
        );

        let mut too_many_compressor_inputs = project.clone();
        too_many_compressor_inputs.song.tracks[1].channel.inserts[0]
            .sidechain_inputs
            .push("alternate".to_owned());
        assert!(
            validate_project(&too_many_compressor_inputs)
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV134" && diagnostic.location.is_some())
        );

        let supported_sync = include_str!("../tests/fixtures/production_mix.crump").replace(
            "    master level -1db {",
            "    master level -1db {\n      processor sync_probe delay {\n        setting tempo_sync true\n        setting division 1/8d\n        setting mode ping_pong\n      }",
        );
        let parsed = parse_project(Path::new("supported-sync.crump"), &supported_sync);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(!validate_project(&parsed.project.expect("Project")).has_errors());

        let conflicting_sync = supported_sync.replace(
            "        setting tempo_sync true",
            "        setting tempo_sync true\n        setting time 0.2s",
        );
        let parsed = parse_project(Path::new("conflicting-sync.crump"), &conflicting_sync);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(
            validate_project(&parsed.project.expect("Project"))
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV136" && diagnostic.location.is_some())
        );

        let orphaned_division = supported_sync.replace(
            "        setting tempo_sync true\n",
            "        setting tempo_sync false\n",
        );
        let parsed = parse_project(Path::new("orphaned-division.crump"), &orphaned_division);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(
            validate_project(&parsed.project.expect("Project"))
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV136" && diagnostic.location.is_some())
        );

        let invalid_division =
            supported_sync.replace("setting division 1/8d", "setting division 1/7");
        let parsed = parse_project(Path::new("invalid-division.crump"), &invalid_division);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(
            validate_project(&parsed.project.expect("Project"))
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV124" && diagnostic.location.is_some())
        );

        let excessive_lookahead = include_str!("../tests/fixtures/production_mix.crump")
            .replace("setting lookahead 0.005s", "setting lookahead 0.5s");
        let parsed = parse_project(Path::new("lookahead.crump"), &excessive_lookahead);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(
            validate_project(&parsed.project.expect("Project"))
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV122" && diagnostic.location.is_some())
        );
    }

    #[test]
    fn rejects_fixed_processor_telemetry_capacity_before_runtime_allocation() {
        let document = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        );
        let mut project = document.project.expect("Project");
        let template = project.song.tracks[0].channel.inserts[0].clone();
        for index in 0..crate::engine::mix_graph::MAX_PROCESSOR_INSTANCES {
            let mut processor = template.clone();
            processor.id = format!("extra_latency_{index}");
            project.song.tracks[0].channel.inserts.push(processor);
        }
        let diagnostic = validate_project(&project)
            .as_slice()
            .iter()
            .find(|diagnostic| diagnostic.code == "CRV092")
            .cloned()
            .expect("processor telemetry capacity diagnostic");
        assert!(diagnostic.message.contains("fixed telemetry capacity"));
    }

    #[test]
    fn repeated_instrument_post_chains_count_as_runtime_processor_instances() {
        let document = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        );
        let mut project = document.project.expect("Project");
        let mut track_template = project.song.tracks[0].clone();
        track_template.channel.inserts.clear();
        track_template.channel.sends.clear();
        track_template.channel.output = crate::model::OutputTarget::Master;
        project.song.tracks.clear();
        for index in 0..crate::engine::schedule::MAX_RUNTIME_TRACKS {
            let mut track = track_template.clone();
            track.id = format!("runtime_track_{index}");
            for (clip_index, clip) in track.clips.iter_mut().enumerate() {
                clip.id = format!("runtime_clip_{index}_{clip_index}");
            }
            project.song.tracks.push(track);
        }
        project.song.groups.clear();
        project.song.returns.clear();
        project.song.sidechains.clear();
        project
            .song
            .master
            .as_mut()
            .expect("Master")
            .inserts
            .clear();
        let instrument = project
            .song
            .instruments
            .iter_mut()
            .find(|instrument| instrument.id == track_template.instrument)
            .expect("Instrument");
        let processor_template = instrument.post_processors[0].clone();
        instrument.post_processors = (0..17)
            .map(|index| {
                let mut processor = processor_template.clone();
                processor.id = format!("runtime_post_{index}");
                processor
            })
            .collect();
        let post_processor_count = instrument.post_processors.len();

        let estimate = crate::engine::mix_graph::RuntimeResourceEstimate::for_project(&project, 0)
            .expect("resource estimate");
        assert_eq!(
            estimate.processors,
            crate::engine::schedule::MAX_RUNTIME_TRACKS * post_processor_count
        );
        assert!(!estimate.admitted);
        assert!(
            validate_project(&project)
                .as_slice()
                .iter()
                .any(|diagnostic| {
                    diagnostic.code == "CRV092"
                        && diagnostic.message.contains("fixed telemetry capacity")
                })
        );
    }

    #[test]
    fn graph_voice_effect_state_survives_compiled_resource_recalculation() {
        let source = include_str!("../tests/fixtures/minimal.crump")
            .replace("polyphony 4", "polyphony 64")
            .replace(
                "        node output output",
                "        node long_delay delay time 2s feedback 0 mix 0\n        node output output",
            )
            .replace(
                "        connect position.audio -> output.audio",
                "        connect position.audio -> long_delay.audio\n        connect long_delay.audio -> output.audio",
            )
            .replace(
                "    track drums \"Drums\" instrument drums level -7db pan 0.1 {",
                "    track lead-b \"Lead B\" instrument lead level -9db pan -0.1 {\n      clip melody-b pattern melody start 0bar duration 1bar mode once\n    }\n    track lead-c \"Lead C\" instrument lead level -9db pan -0.1 {\n      clip melody-c pattern melody start 0bar duration 1bar mode once\n    }\n    track lead-d \"Lead D\" instrument lead level -9db pan -0.1 {\n      clip melody-d pattern melody start 0bar duration 1bar mode once\n    }\n    track drums \"Drums\" instrument drums level -7db pan 0.1 {",
            );
        let document = parse_project(Path::new("graph-budget.crump"), &source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        let estimate = crate::engine::mix_graph::RuntimeResourceEstimate::for_project(&project, 0)
            .expect("resource estimate");
        let delay_payload = 256_u64 * 2 * 48_000 * std::mem::size_of::<[f32; 2]>() as u64;
        assert!(estimate.graph_voice_state_bytes >= delay_payload);
        assert!(!estimate.admitted);
        let diagnostics = validate_project(&project);
        let diagnostic = diagnostics
            .as_slice()
            .iter()
            .find(|diagnostic| diagnostic.code == "CRV092")
            .cloned()
            .unwrap_or_else(|| panic!("graph voice state budget diagnostic: {diagnostics:?}"));
        assert!(diagnostic.message.contains("bytes at publication"));
    }

    #[test]
    fn rejects_named_mix_runtime_budget_before_allocation() {
        let processors = (0..90)
            .map(|index| {
                format!(
                    "      processor delay_{index} delay {{\n        setting time 1s\n      }}\n"
                )
            })
            .collect::<String>();
        let source = VALID.replace(
            "    track lead-track \"Lead\" instrument lead level -6db pan 0 {\n",
            &format!(
                "    track lead-track \"Lead\" instrument lead level -6db pan 0 {{\n{processors}"
            ),
        );
        let document = parse_project(Path::new("budget.crump"), &source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics
        );
        let diagnostics = validate_project(&document.project.expect("Project"));
        let diagnostic = diagnostics
            .as_slice()
            .iter()
            .find(|diagnostic| diagnostic.code == "CRV092")
            .expect("named runtime budget diagnostic");
        assert!(diagnostic.message.contains("bytes at publication"));
        assert!(diagnostic.message.contains("budget"));
    }

    #[test]
    fn rejects_runtime_track_bound_before_publication() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let template = project.song.tracks[0].clone();
        for index in 1..=32 {
            let mut track = template.clone();
            track.id = format!("lead-track-{index}");
            track.clips[0].id = format!("lead-clip-{index}");
            project.song.tracks.push(track);
        }
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV091")
        );
    }

    #[test]
    fn rejects_more_than_bounded_events_at_one_frame() {
        let document = parse_project(Path::new("project.crump"), VALID);
        let mut project = document.project.expect("Project");
        let event = project.song.patterns[0].events[0].clone();
        project.song.patterns[0].events = vec![event; 257];
        let diagnostics = validate_project(&project);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV091")
        );
    }
}
