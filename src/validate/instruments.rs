//! Instrument definitions and Composable DSP Graph validation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{Graph, Instrument, InstrumentKind, NodeKind, Project, SettingValue, Unit},
};

use super::{
    MAX_GRAPH_NODES, MAX_GRAPH_VOICES, located_error, source_site_error, valid_id, valid_key,
};

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
