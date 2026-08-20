//! Whole-Project semantic and graph validation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, Severity, SourceLocation},
    model::{
        ClipMode, Graph, Instrument, InstrumentKind, NodeKind, PatternEvent, Project, SettingValue,
        Unit,
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

/// Validates a complete Project candidate for publication.
#[must_use]
pub fn validate_project(project: &Project) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    validate_project_header(project, &mut diagnostics);
    validate_instruments(&project.song.instruments, &mut diagnostics);
    validate_patterns(project, &mut diagnostics);
    validate_tracks(project, &mut diagnostics);
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
            project.imports.iter().all(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "crump")
            }),
            "all imported artifacts are readable Crump source modules",
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
    let mut total_graph_voices = 0_u32;
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
        if instrument.kind == InstrumentKind::Graph {
            total_graph_voices = total_graph_voices.saturating_add(u32::from(instrument.polyphony));
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
            (InstrumentKind::StockDrums, None) => {}
        }
    }
    if total_graph_voices > MAX_GRAPH_VOICES {
        diagnostics.push(Diagnostic::error(
            "CRV015",
            format!(
                "Project declares {total_graph_voices} graph voices; maximum is {MAX_GRAPH_VOICES}"
            ),
        ));
    }
    if instruments.is_empty() {
        diagnostics.push(Diagnostic::error(
            "CRV016",
            "Song must declare at least one instrument",
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
        (NodeKind::Oscillator | NodeKind::Noise | NodeKind::Amplifier, "level")
        | (NodeKind::Mixer, "a_level" | "b_level") => Some(NumericDomain::bounded(0.0, 4.0)),
        (NodeKind::Oscillator, "detune") => Some(NumericDomain::bounded(-96.0, 96.0)),
        (NodeKind::Envelope, "attack" | "decay") => Some(NumericDomain::at_least(0.0)),
        (NodeKind::Envelope, "sustain") => Some(NumericDomain::bounded(0.0, 1.0)),
        (NodeKind::Envelope, "release") => Some(NumericDomain::bounded(0.0, 8.0)),
        (NodeKind::Lowpass, "cutoff") => Some(NumericDomain::bounded(10.0, 21_600.0)),
        (NodeKind::Lowpass, "resonance") => Some(NumericDomain::bounded(0.0, 1.0)),
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
    let mut track_ids = BTreeSet::new();
    let mut clip_ids = BTreeSet::new();
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
        if !track.level_db.is_finite() || !(-96.0..=12.0).contains(&track.level_db) {
            source_site_error(
                diagnostics,
                "CRV072",
                "Track level must be between -96db and 12db",
                &track.level_site,
            );
        }
        if !track.pan.is_finite() || !(-1.0..=1.0).contains(&track.pan) {
            source_site_error(
                diagnostics,
                "CRV073",
                "Track pan must be between -1 and 1",
                &track.pan_site,
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
            if let (Some(instrument), Some(pattern)) = (instrument, pattern) {
                let has_notes = pattern
                    .events
                    .iter()
                    .any(|event| matches!(event, PatternEvent::Note { .. }));
                let has_hits = pattern
                    .events
                    .iter()
                    .any(|event| matches!(event, PatternEvent::Hit { .. }));
                if instrument.kind == InstrumentKind::Graph && has_hits {
                    located_error(
                        diagnostics,
                        "CRV079",
                        "graph instrument Track cannot consume percussion hit Events",
                        &clip.location,
                    );
                }
                if instrument.kind == InstrumentKind::StockDrums && has_notes {
                    located_error(
                        diagnostics,
                        "CRV080",
                        "stock_drums Track cannot consume pitched note Events",
                        &clip.location,
                    );
                }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalType {
    AudioMono,
    AudioStereo,
    Hertz,
    Gate,
    Unipolar,
    Bipolar,
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
        (NodeKind::Pan | NodeKind::Delay | NodeKind::Reverb, "audio") => {
            Some(SignalType::AudioStereo)
        }
        (NodeKind::Envelope, "control") => Some(SignalType::Unipolar),
        (NodeKind::Lfo, "control") => Some(SignalType::Bipolar),
        _ => None,
    }
}

fn node_input(kind: NodeKind, port: &str) -> Option<SignalType> {
    match (kind, port) {
        (NodeKind::Oscillator, "pitch") => Some(SignalType::Hertz),
        (NodeKind::Envelope, "gate") => Some(SignalType::Gate),
        (NodeKind::Mixer, "a" | "b") => Some(SignalType::AudioMono),
        (
            NodeKind::Lowpass | NodeKind::Amplifier | NodeKind::Saturation | NodeKind::Pan,
            "audio",
        ) => Some(SignalType::AudioMono),
        (NodeKind::Lowpass, "mod") => Some(SignalType::Bipolar),
        (NodeKind::Amplifier, "gain") => Some(SignalType::Unipolar),
        (NodeKind::Delay | NodeKind::Reverb | NodeKind::Output, "audio") => {
            Some(SignalType::AudioStereo)
        }
        _ => None,
    }
}

const fn required_inputs(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Oscillator => &["pitch"],
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
}

#[derive(Clone, Copy)]
struct SettingSpec {
    key: &'static str,
    kind: SettingKind,
}

#[derive(Clone, Copy)]
enum SettingKind {
    Number(Unit),
    Text(&'static [&'static str]),
}

const fn setting_schema(kind: NodeKind) -> &'static [SettingSpec] {
    use SettingKind::{Number, Text};
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

    use crate::dsl::parse_project;

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
