use super::*;

/// Explicit panner mode compiled for one non-Master channel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PannerMode {
    /// Collapse to mono and place with equal power.
    MonoPan,
    /// Attenuate stereo sides without crossfeed.
    Balance,
    /// Place the two input sides independently.
    StereoPan,
}

/// One inspectable compiled channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledChannelInspection {
    /// Dense callback-time execution index.
    pub index: usize,
    /// Stable source identity (`track/ID`, `group/ID`, `return/ID`, or `master`).
    pub identity: String,
    /// Domain role.
    pub role: ChannelRole,
    /// Explicit panner mode for non-Master channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panner: Option<PannerMode>,
    /// Source-backed mute state.
    pub mute: bool,
    /// Ordered processor identities, including Instrument post-sum processors.
    pub processors: Vec<String>,
    /// Compensation applied to a direct Track source before shared processing.
    pub source_compensation_frames: u32,
    /// Deterministic latency at the channel output.
    pub output_latency_frames: u32,
}

/// Kind of one compiled Audio route.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteRole {
    /// Exactly one channel main output.
    Main,
    /// Durably identified parallel branch to a Return.
    Send,
    /// Named auxiliary Audio input on a processor.
    Sidechain,
}

/// One inspectable stable route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledRouteInspection {
    /// Stable route identity used for deterministic fan-in ordering.
    pub id: String,
    /// Route semantic role.
    pub role: RouteRole,
    /// Source channel identity.
    pub from: String,
    /// Destination channel or processor input identity.
    pub to: String,
    /// Explicit source tap.
    pub tap: TapPoint,
    /// Channel-count-aware Audio layout.
    pub layout: String,
    /// Compensation inserted on this Bus.
    pub compensation_frames: u32,
}

/// One resolved processor setting in physical or discrete form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledProcessorSettingInspection {
    /// Processor-local stable setting key.
    pub key: String,
    /// Canonical unit for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,
    /// Resolved physical value for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Resolved choice for a discrete setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// Inclusive lower domain for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Inclusive upper domain for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    /// Whether this value has an exact authored source literal.
    pub authored: bool,
    /// Whether the authored value also has a stable continuous Parameter target.
    pub parameter_target: bool,
}

/// One inspectable processor instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledProcessorInspection {
    /// Dense fixed-capacity runtime telemetry index.
    pub index: usize,
    /// Durable processor identity.
    pub id: String,
    /// Native implementation.
    pub kind: ProcessorKind,
    /// Owning Instrument or channel identity.
    pub owner: String,
    /// Durable source-backed bypass state.
    pub authored_bypass: bool,
    /// Bypassed processors continue processing so tails and detector state survive re-enable.
    pub state_policy: ProcessorStatePolicy,
    /// Every effective runtime setting, including explicit defaults.
    pub settings: Vec<CompiledProcessorSettingInspection>,
    /// Named auxiliary stereo inputs.
    pub sidechain_inputs: Vec<String>,
    /// Explicit generic latency authored on the processor declaration.
    pub authored_latency_frames: u32,
    /// Latency introduced by the native algorithm or kind-specific configuration.
    pub intrinsic_latency_frames: u32,
    /// Deterministic total output latency.
    pub output_latency_frames: u32,
    /// Nonlinear evaluations per input frame; one means base-rate processing.
    pub nonlinear_substeps: u8,
    /// Filtered oversampling ratio; one means no oversampling kernel.
    pub oversampling_factor: u8,
    /// Linear-phase FIR taps in the shared oversampling kernel; zero means none.
    pub oversampling_filter_taps: u16,
    /// Fixed detector headroom used for reconstructed output peaks, in decibels.
    pub true_peak_safety_margin_db: f64,
    /// Preallocated mutable processor-state bytes used by admission.
    pub state_bytes: u64,
    /// Abstract callback work units per rendered frame used by admission.
    pub work_units_per_frame: u64,
}

/// Mutable-state policy while a processor's output follows its aligned dry path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorStatePolicy {
    /// The wet processor continues receiving Audio while bypassed.
    Continue,
}

/// One complete source-to-Master signal path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalPathInspection {
    /// Stable timeline or branch source.
    pub source: String,
    /// Ordered processing identities ending at Master.
    pub stages: Vec<String>,
    /// Total deterministic path latency after convergence compensation.
    pub total_latency_frames: u32,
}

/// Serializable inspection of the compiled processing graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MixGraphInspection {
    /// Stable deterministic execution channels.
    pub channels: Vec<CompiledChannelInspection>,
    /// Stable deterministic Audio routes.
    pub routes: Vec<CompiledRouteInspection>,
    /// Ordered native processors.
    pub processors: Vec<CompiledProcessorInspection>,
    /// Complete signal paths from timeline and branch sources.
    pub signal_paths: Vec<SignalPathInspection>,
    /// Resource admission estimate used before runtime allocation.
    pub resources: RuntimeResourceEstimate,
}

pub(super) fn resolved_processor_settings(
    processor: &crate::model::Processor,
    tempo_bpm: f64,
) -> Result<Vec<CompiledProcessorSettingInspection>, EngineError> {
    let settings = resolve_settings(processor, tempo_bpm)
        .map_err(|error| EngineError::Compile(error.compile_message(processor)))?;
    Ok(settings
        .iter()
        .map(|setting| {
            let numeric = setting.descriptor.contract.numeric();
            let (unit, minimum, maximum) =
                numeric.map_or((None, None, None), |(unit, minimum, maximum, _)| {
                    (
                        Some(unit),
                        (!setting.derived).then_some(minimum),
                        (!setting.derived).then_some(maximum),
                    )
                });
            let (value, choice) = match &setting.value {
                ResolvedSettingValue::Numeric(value) => (Some(*value), None),
                ResolvedSettingValue::Choice(choice) => (None, Some(choice.clone())),
                ResolvedSettingValue::Unsigned(value) => (None, Some(value.to_string())),
            };
            CompiledProcessorSettingInspection {
                key: setting.descriptor.key.to_owned(),
                unit,
                value,
                choice,
                minimum,
                maximum,
                authored: setting.authored,
                parameter_target: setting.parameter_target(),
            }
        })
        .collect())
}

pub(super) fn compile_inspection(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    mut processors: Vec<CompiledProcessorInspection>,
    master_channel: usize,
    resources: RuntimeResourceEstimate,
) -> MixGraphInspection {
    let mut channel_inspection = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| CompiledChannelInspection {
            index,
            identity: channel.identity.clone(),
            role: channel.role,
            panner: channel.panner.map(CompiledPanner::mode),
            mute: channel.mute.initial() >= 0.5,
            processors: channel
                .source_processors
                .iter()
                .chain(&channel.inserts)
                .map(|processor| processor.program.id.clone())
                .collect(),
            source_compensation_frames: channel.source_compensation_frames,
            output_latency_frames: channel.post_latency_frames,
        })
        .collect::<Vec<_>>();
    channel_inspection.sort_by_key(|channel| channel.index);
    processors.sort_by_key(|processor| processor.index);
    let route_inspection = routes
        .iter()
        .map(|route| CompiledRouteInspection {
            id: route.id.clone(),
            role: route.role,
            from: channels[route.from].identity.clone(),
            to: route.destination_display.clone(),
            tap: route.tap,
            layout: "stereo".to_owned(),
            compensation_frames: route.compensation_frames,
        })
        .collect::<Vec<_>>();
    let mut signal_paths = Vec::new();
    for (source_index, track_channel) in channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.role == ChannelRole::Track)
    {
        let mut stages = Vec::new();
        append_main_signal_path(
            channels,
            routes,
            source_index,
            0,
            true,
            master_channel,
            &mut stages,
        );
        signal_paths.push(SignalPathInspection {
            source: track_channel.identity.clone(),
            stages,
            total_latency_frames: channels[master_channel].post_latency_frames,
        });
    }
    for route in routes.iter().filter(|route| route.role != RouteRole::Main) {
        let source = &channels[route.from];
        let mut stages = source
            .source_processors
            .iter()
            .chain(&source.inserts)
            .map(|processor| format!("processor/{}", processor.program.id))
            .collect::<Vec<_>>();
        stages.extend([
            source.identity.clone(),
            format!("{:?}", route.tap).to_lowercase(),
            route.id.clone(),
        ]);
        append_main_signal_path(
            channels,
            routes,
            route.owner,
            route.processor_index.unwrap_or(0),
            route.processor_index.is_none(),
            master_channel,
            &mut stages,
        );
        signal_paths.push(SignalPathInspection {
            source: route.id.clone(),
            stages,
            total_latency_frames: channels[master_channel].post_latency_frames,
        });
    }
    signal_paths.sort_by(|left, right| left.source.cmp(&right.source));
    MixGraphInspection {
        channels: channel_inspection,
        routes: route_inspection,
        processors,
        signal_paths,
        resources,
    }
}

pub(super) fn append_main_signal_path(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    mut current: usize,
    first_insert: usize,
    include_first_source_processors: bool,
    master_channel: usize,
    stages: &mut Vec<String>,
) {
    let mut visited = BTreeSet::new();
    let mut first = true;
    loop {
        if !visited.insert(current) {
            break;
        }
        let channel = &channels[current];
        if !first || include_first_source_processors {
            stages.extend(
                channel
                    .source_processors
                    .iter()
                    .map(|processor| format!("processor/{}", processor.program.id)),
            );
        }
        stages.extend(
            channel
                .inserts
                .iter()
                .skip(if first { first_insert } else { 0 })
                .map(|processor| format!("processor/{}", processor.program.id)),
        );
        stages.push(channel.identity.clone());
        if current == master_channel {
            break;
        }
        let Some(route) = routes
            .iter()
            .find(|route| route.from == current && route.role == RouteRole::Main)
        else {
            break;
        };
        let RouteDestination::Bus(destination) = route.destination else {
            break;
        };
        current = destination;
        first = false;
    }
}
