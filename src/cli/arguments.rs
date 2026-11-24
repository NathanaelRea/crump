use super::*;

/// Crump command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "crump",
    version,
    about = "A source-backed musical sketch instrument"
)]
pub struct Cli {
    /// Project directory or `project.crump` path. Interactive startup searches when omitted.
    #[arg(long, global = true, default_value = ".")]
    pub(super) project: PathBuf,
    #[arg(skip)]
    pub(super) project_explicit: bool,
    /// Emit exactly one versioned JSON response on stdout.
    #[arg(long, global = true)]
    pub(super) json: bool,
    #[command(subcommand)]
    pub(super) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Create a valid blank Project and initial history entry.
    New(NewArgs),
    /// Parse and validate the current Candidate Revision.
    Validate,
    /// Inspect the complete parsed Candidate model, controls, and revision state.
    Inspect(InspectArgs),
    /// Show Candidate and Active Revision state.
    Status,
    /// Explicitly promote the valid Candidate to Active Revision.
    Publish(PublishArgs),
    /// Run objective Musical Sketch acceptance checks.
    Check,
    /// Play the exact Active Revision through native PipeWire.
    Play(PlayArgs),
    /// Open the Production Workspace in a desktop window (builds with the `gui` feature).
    #[command(alias = "workspace")]
    Gui,
    /// Render the exact Active Revision to an atomic float-WAV bundle.
    Render(RenderArgs),
    /// Profile one exact Active range at a supported offline callback quantum.
    Qualify(QualifyArgs),
    /// Read or restore append-only Project History.
    #[command(subcommand)]
    History(HistoryCommand),
    /// Apply one checked, source-preserving Production View edit.
    #[command(subcommand)]
    Edit(Box<EditCommand>),
    /// Measure and adjust mix levels through checked Structured Source Edits.
    #[command(subcommand)]
    Mix(MixCommand),
    /// Import or inspect portable Project media.
    #[command(subcommand)]
    Asset(AssetCommand),
    /// Export, install, inspect, or list reusable Instrument Modules.
    #[command(subcommand)]
    Module(ModuleCommand),
    /// Report host and Project support facts.
    Doctor,
}

#[derive(Debug, Args)]
pub(super) struct NewArgs {
    /// New Project directory. Defaults to the global --project path.
    pub(super) path: Option<PathBuf>,
    /// Human-facing Project name.
    #[arg(long)]
    pub(super) name: Option<String>,
    /// Stable source ID.
    #[arg(long)]
    pub(super) id: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct PublishArgs {
    /// Publication origin class.
    #[arg(long, value_enum, default_value_t = OriginArg::Human)]
    pub(super) origin: OriginArg,
    /// Human or Agent identity.
    #[arg(long, default_value = "cli")]
    pub(super) actor: String,
    /// Concise history-list description.
    #[arg(long)]
    pub(super) summary: String,
    /// Rationale. Required for Agent publications.
    #[arg(long, default_value = "")]
    pub(super) explanation: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum OriginArg {
    Human,
    Agent,
    System,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ArrangementAutomationArg {
    /// Move Automation with transformed Timeline material.
    Follow,
    /// Keep Automation at absolute Song times.
    Locked,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum SampleDirectionArg {
    Forward,
    Reverse,
}

impl From<SampleDirectionArg> for crate::model::SampleDirection {
    fn from(value: SampleDirectionArg) -> Self {
        match value {
            SampleDirectionArg::Forward => Self::Forward,
            SampleDirectionArg::Reverse => Self::Reverse,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum FormantPolicyArg {
    Shift,
    Preserve,
}

impl From<FormantPolicyArg> for crate::model::FormantPolicy {
    fn from(value: FormantPolicyArg) -> Self {
        match value {
            FormantPolicyArg::Shift => Self::Shift,
            FormantPolicyArg::Preserve => Self::Preserve,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum SampleTransformQualityArg {
    EffectV1,
}

impl From<SampleTransformQualityArg> for crate::model::SampleTransformQuality {
    fn from(value: SampleTransformQualityArg) -> Self {
        match value {
            SampleTransformQualityArg::EffectV1 => Self::EffectV1,
        }
    }
}

impl From<ArrangementAutomationArg> for ArrangementAutomation {
    fn from(value: ArrangementAutomationArg) -> Self {
        match value {
            ArrangementAutomationArg::Follow => Self::FollowArrangement,
            ArrangementAutomationArg::Locked => Self::LockToSong,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct InspectArgs {
    /// Evaluate effective Parameter provenance at this exact Song time.
    #[arg(long, default_value = "0bar")]
    pub(super) at: String,
}

#[derive(Debug, Args)]
pub(super) struct PlayArgs {
    /// Stop after this many seconds instead of the complete audition range.
    #[arg(long)]
    pub(super) seconds: Option<f64>,
    /// Target: song, track/ID, clip/ID, pattern/ID, asset/ID, mapping/I/M, or occurrence/P/O.
    #[arg(long, default_value = "song")]
    pub(super) target: String,
    /// Explicit monitoring path.
    #[arg(long, value_enum, default_value_t = AuditionPathArg::Master)]
    pub(super) path: AuditionPathArg,
    /// Start at this named Song Locator or exact musical time after deterministic preroll.
    #[arg(long)]
    pub(super) from: Option<String>,
    /// Stop at this named Song Locator or exact musical time.
    #[arg(long)]
    pub(super) to: Option<String>,
    /// Loop the explicit --from/--to range transiently.
    #[arg(long, requires_all = ["from", "to"])]
    pub(super) loop_range: bool,
    /// Inclusive original-source-frame start for an asset target.
    #[arg(long)]
    pub(super) sample_start_frame: Option<u64>,
    /// Exclusive original-source-frame end for an asset target.
    #[arg(long)]
    pub(super) sample_end_frame: Option<u64>,
    /// Bypass one stable processor ID for this audition only. Repeat as needed.
    #[arg(long = "bypass", value_name = "PROCESSOR")]
    pub(super) bypass: Vec<String>,
    /// Resolve and report the audition plan without opening audio.
    #[arg(long)]
    pub(super) plan: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum AuditionPathArg {
    Raw,
    Mapping,
    ChannelPre,
    ChannelPost,
    Master,
}

impl From<AuditionPathArg> for AuditionPath {
    fn from(value: AuditionPathArg) -> Self {
        match value {
            AuditionPathArg::Raw => Self::Raw,
            AuditionPathArg::Mapping => Self::Mapping,
            AuditionPathArg::ChannelPre => Self::Channel {
                tap: TapPoint::PreFader,
            },
            AuditionPathArg::ChannelPost => Self::Channel {
                tap: TapPoint::PostFader,
            },
            AuditionPathArg::Master => Self::Master,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ProcessorMoveArg {
    Earlier,
    Later,
}

#[derive(Debug, Args, Default)]
pub(super) struct TimeGridArgs {
    /// Snap the supplied exact time to an explicit grid.
    #[arg(long, requires = "grid", conflicts_with = "free")]
    pub(super) snap: bool,
    /// Positive exact musical grid used with --snap.
    #[arg(long, requires = "snap", conflicts_with = "free", value_name = "TIME")]
    pub(super) grid: Option<String>,
    /// Apply the supplied exact time without snapping.
    #[arg(long, conflicts_with_all = ["snap", "grid"])]
    pub(super) free: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum RenderTailPolicyArg {
    /// Keep rendered tail frames after the arrangement end.
    Append,
    /// Sum rendered tail frames back onto the start of the arrangement for a seamless loop.
    Wrap,
}

impl From<RenderTailPolicyArg> for RenderTailPolicy {
    fn from(value: RenderTailPolicyArg) -> Self {
        match value {
            RenderTailPolicyArg::Append => Self::Append,
            RenderTailPolicyArg::Wrap => Self::Wrap,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct RenderArgs {
    /// Destination bundle directory.
    #[arg(long, default_value = "render/master")]
    pub(super) output: PathBuf,
    /// Show the exact Active Render Range without writing output.
    #[arg(long)]
    pub(super) plan: bool,
    /// Export one aligned Track/Group tap (`track/ID:post_fader`). Repeat as needed.
    #[arg(long = "stem", value_name = "OWNER:TAP")]
    pub(super) stems: Vec<RenderStemRequest>,
    /// Override the Project's authored `render_tail` policy (`append` or `wrap`) for this render.
    #[arg(long, value_enum)]
    pub(super) tail: Option<RenderTailPolicyArg>,
    /// Fail rather than atomically replace an existing complete bundle.
    #[arg(long)]
    pub(super) no_replace: bool,
}

#[derive(Debug, Subcommand)]
pub(super) enum MixCommand {
    /// Measure an owner's offline post-fader loudness and write its updated `level` through History.
    LevelTo(MixLevelToArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum LoudnessMeasureArg {
    /// K-weighted ITU-R BS.1770 / EBU R128 integrated loudness (LUFS).
    IntegratedLufs,
    /// Unweighted stereo RMS across the entire finalized signal (dBFS).
    Rms,
    /// Unweighted stereo RMS across active 400 ms blocks (100 ms step) at or above -60 dBFS.
    ActiveRms,
}

impl From<LoudnessMeasureArg> for LoudnessMeasure {
    fn from(value: LoudnessMeasureArg) -> Self {
        match value {
            LoudnessMeasureArg::IntegratedLufs => Self::IntegratedLufs,
            LoudnessMeasureArg::Rms => Self::Rms,
            LoudnessMeasureArg::ActiveRms => Self::ActiveRms,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct MixLevelToArgs {
    /// Owner whose `level` fader is measured and updated (`master`, `track/ID`, or `group/ID`).
    #[arg(long, value_name = "OWNER")]
    pub(super) owner: MixLevelOwner,
    /// Target loudness (e.g. `-20db`, `-14lufs`, `-18dbfs`).
    #[arg(long, allow_hyphen_values = true, value_parser = parse_level_target_db, value_name = "LEVEL")]
    pub(super) target: f64,
    /// Loudness metric to match.
    #[arg(long, value_enum, default_value_t = LoudnessMeasureArg::IntegratedLufs)]
    pub(super) measure: LoudnessMeasureArg,
    /// Measure and compute the fader change without writing Authoritative Source or History.
    #[arg(long)]
    pub(super) dry_run: bool,
    /// Publication origin class when writing History.
    #[arg(long, value_enum, default_value_t = OriginArg::Human)]
    pub(super) origin: OriginArg,
    /// Human or Agent identity recorded on the published History entry.
    #[arg(long, default_value = "cli")]
    pub(super) actor: String,
    /// Optional explicit History summary.
    #[arg(long)]
    pub(super) summary: Option<String>,
    /// Optional explicit History explanation.
    #[arg(long)]
    pub(super) explanation: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct QualifyArgs {
    /// Start at this named Song Locator after deterministic untimed preroll.
    #[arg(long)]
    pub(super) from: Option<String>,
    /// Stop at this named Song Locator.
    #[arg(long)]
    pub(super) to: Option<String>,
    /// Supported callback quantum to profile.
    #[arg(long, default_value_t = 128, value_parser = parse_qualification_quantum)]
    pub(super) quantum: usize,
    /// Required p99.9 callback-utilization ceiling in `(0, 1]`.
    #[arg(long, default_value_t = 0.8)]
    pub(super) maximum_p999_utilization: f64,
    /// Optional machine-readable report destination.
    #[arg(long)]
    pub(super) output: Option<PathBuf>,
    /// Replace an existing report file atomically.
    #[arg(long, requires = "output")]
    pub(super) replace: bool,
    /// Exit unsuccessfully when the completed offline report does not pass its thresholds.
    #[arg(long)]
    pub(super) require_pass: bool,
    /// Resolve the range and resources without timing Audio or writing a report.
    #[arg(long)]
    pub(super) plan: bool,
}

fn parse_qualification_quantum(value: &str) -> Result<usize, String> {
    match value {
        "64" => Ok(64),
        "128" => Ok(128),
        "256" => Ok(256),
        _ => Err("quantum must be 64, 128, or 256 frames".to_owned()),
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum HistoryCommand {
    /// List all committed entries in sequence order.
    List,
    /// Show one entry and its exact parent diff.
    Show { id: String },
    /// Print one entry's exact parent diff.
    Diff { id: String },
    /// Restore an earlier exact snapshot as a new append-only entry.
    Restore {
        id: String,
        #[arg(long, default_value = "Restore selected Project state")]
        explanation: String,
        #[arg(long, default_value = "cli")]
        actor: String,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum EditCommand {
    /// Set any continuous stable Parameter target in physical units.
    Target {
        target: String,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Parameter target")]
        summary: String,
    },
    /// Set one exact Instrument Parameter in physical units.
    Parameter {
        instrument: String,
        parameter: String,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Instrument Parameter")]
        summary: String,
    },
    /// Freeze or unfreeze a Track for live playback.
    TrackFreeze {
        track: String,
        #[arg(action = clap::ArgAction::Set)]
        frozen: bool,
        #[arg(long, default_value = "Change Track Freeze")]
        summary: String,
    },
    /// Set one Track level in dB.
    TrackLevel {
        track: String,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Track level")]
        summary: String,
    },
    /// Set one Track pan value from -1 to 1.
    TrackPan {
        track: String,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Track pan")]
        summary: String,
    },
    /// Set one processor setting from an exact source literal.
    ProcessorSetting {
        processor: String,
        setting: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long, default_value = "Change processor setting")]
        summary: String,
    },
    /// Edit one durable Instrument-owned Modulation route.
    ///
    /// Flattening keeps the public command names stable while bounding derive-time stack use.
    #[command(flatten)]
    InstrumentRoute(InstrumentRouteEditCommand),
    /// Set one durable source-backed processor bypass value.
    ProcessorBypass {
        processor: String,
        #[arg(action = clap::ArgAction::Set)]
        bypass: bool,
        #[arg(long, default_value = "Change processor bypass")]
        summary: String,
    },
    /// Move one processor by one position in its authored chain.
    ProcessorMove {
        processor: String,
        direction: ProcessorMoveArg,
        #[arg(long, default_value = "Reorder processor chain")]
        summary: String,
    },
    /// Create one absolute Song Automation lane with its required zero point.
    AutomationCreate {
        lane: String,
        target: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long, default_value = "Create Automation lane")]
        summary: String,
    },
    /// Delete one complete Song Automation lane.
    AutomationDelete {
        lane: String,
        #[arg(long, default_value = "Delete Automation lane")]
        summary: String,
    },
    /// Create one durable Automation point.
    AutomationPointCreate {
        lane: String,
        point: String,
        time: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long, default_value = "linear")]
        curve: String,
        #[arg(long, default_value = "Create Automation point")]
        summary: String,
    },
    /// Set one Automation point's exact Song time.
    AutomationPointTime {
        lane: String,
        point: String,
        time: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Move Automation point")]
        summary: String,
    },
    /// Set one Automation point's physical value.
    AutomationPointValue {
        lane: String,
        point: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long, default_value = "Change Automation value")]
        summary: String,
    },
    /// Set the interpolation leaving one Automation point.
    AutomationPointCurve {
        lane: String,
        point: String,
        curve: String,
        #[arg(long, default_value = "Change Automation curve")]
        summary: String,
    },
    /// Delete one durable Automation point.
    AutomationPointDelete {
        lane: String,
        point: String,
        #[arg(long, default_value = "Delete Automation point")]
        summary: String,
    },
    /// Create one reusable deterministic Groove Template.
    GrooveCreate {
        id: String,
        grid: String,
        #[arg(allow_hyphen_values = true)]
        swing: f64,
        timing: String,
        #[arg(allow_hyphen_values = true)]
        velocity: f64,
        #[arg(allow_hyphen_values = true)]
        probability: f64,
        seed: u64,
        #[arg(long, default_value = "Create Groove Template")]
        summary: String,
    },
    /// Update one reusable deterministic Groove Template.
    GrooveUpdate {
        id: String,
        grid: String,
        #[arg(allow_hyphen_values = true)]
        swing: f64,
        timing: String,
        #[arg(allow_hyphen_values = true)]
        velocity: f64,
        #[arg(allow_hyphen_values = true)]
        probability: f64,
        seed: u64,
        #[arg(long, default_value = "Update Groove Template")]
        summary: String,
    },
    /// Delete one unused Groove Template.
    GrooveDelete {
        id: String,
        #[arg(long, default_value = "Delete Groove Template")]
        summary: String,
    },
    /// Create one procedural Pattern Variation without flattening Events.
    PatternVariationCreate {
        id: String,
        source: String,
        rotate: String,
        #[arg(allow_hyphen_values = true)]
        transpose: i16,
        #[arg(allow_hyphen_values = true)]
        velocity: f64,
        #[arg(action = clap::ArgAction::Set)]
        reverse: bool,
        #[arg(allow_hyphen_values = true)]
        probability: f64,
        seed: u64,
        #[arg(long, default_value = "Create Pattern Variation")]
        summary: String,
    },
    /// Update one procedural Pattern Variation's checked transforms.
    PatternVariationUpdate {
        id: String,
        source: String,
        rotate: String,
        #[arg(allow_hyphen_values = true)]
        transpose: i16,
        #[arg(allow_hyphen_values = true)]
        velocity: f64,
        #[arg(action = clap::ArgAction::Set)]
        reverse: bool,
        #[arg(allow_hyphen_values = true)]
        probability: f64,
        seed: u64,
        #[arg(long, default_value = "Update Pattern Variation")]
        summary: String,
    },
    /// Delete one unreferenced procedural Pattern Variation.
    PatternVariationDelete {
        id: String,
        #[arg(long, default_value = "Delete Pattern Variation")]
        summary: String,
    },
    /// Create one durable Pattern Parameter lock.
    ParameterLockCreate {
        pattern: String,
        id: String,
        at: String,
        parameter: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Create Parameter lock")]
        summary: String,
    },
    /// Update one durable Pattern Parameter lock.
    ParameterLockUpdate {
        pattern: String,
        id: String,
        at: String,
        parameter: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Update Parameter lock")]
        summary: String,
    },
    /// Delete one durable Pattern Parameter lock.
    ParameterLockDelete {
        pattern: String,
        id: String,
        #[arg(long, default_value = "Delete Parameter lock")]
        summary: String,
    },
    /// Assign a Groove Template to a Clip, or omit GROOVE to clear it.
    ClipGroove {
        id: String,
        groove: Option<String>,
        #[arg(long, default_value = "Assign Clip Groove")]
        summary: String,
    },
    /// Insert empty arrangement time and shift later material.
    TimeInsert {
        at: String,
        duration: String,
        #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
        automation: ArrangementAutomationArg,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Insert arrangement time")]
        summary: String,
    },
    /// Delete arrangement time; boundary-crossing Clips must be split first.
    TimeDelete {
        at: String,
        duration: String,
        #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
        automation: ArrangementAutomationArg,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Delete arrangement time")]
        summary: String,
    },
    /// Duplicate complete Clips into newly inserted arrangement time.
    TimeDuplicate {
        source_at: String,
        duration: String,
        destination: String,
        suffix: String,
        #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
        automation: ArrangementAutomationArg,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Duplicate arrangement time")]
        summary: String,
    },
    /// Create one named Song position for navigation and ranges.
    LocatorCreate {
        id: String,
        name: String,
        at: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Create Song Locator")]
        summary: String,
    },
    /// Move one named Song Locator.
    LocatorMove {
        id: String,
        at: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Move Song Locator")]
        summary: String,
    },
    /// Delete one named Song Locator.
    LocatorDelete {
        id: String,
        #[arg(long, default_value = "Delete Song Locator")]
        summary: String,
    },
    /// Create one readable Clip on a Timeline Track.
    ClipCreate {
        track: String,
        id: String,
        pattern: String,
        start: String,
        duration: String,
        #[arg(long, default_value = "once")]
        mode: String,
        #[arg(long, default_value = "0beat")]
        offset: String,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
        transpose: i16,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
        gain_db: f64,
        #[arg(long, default_value = "0beat")]
        fade_in: String,
        #[arg(long, default_value = "0beat")]
        fade_out: String,
        #[arg(long)]
        groove: Option<String>,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Create Timeline Clip")]
        summary: String,
    },
    /// Delete one complete Clip declaration.
    ClipDelete {
        id: String,
        #[arg(long, default_value = "Delete Timeline Clip")]
        summary: String,
    },
    /// Move one Clip to an exact Song time.
    ClipMove {
        id: String,
        start: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Move Timeline Clip")]
        summary: String,
    },
    /// Resize one Clip to an exact positive duration.
    ClipResize {
        id: String,
        duration: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Resize Timeline Clip")]
        summary: String,
    },
    /// Duplicate one Clip under a fresh identity.
    ClipDuplicate {
        id: String,
        new_id: String,
        start: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Duplicate Timeline Clip")]
        summary: String,
    },
    /// Split one Clip and give its right side a fresh identity.
    ClipSplit {
        id: String,
        new_id: String,
        at: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Split Timeline Clip")]
        summary: String,
    },
    /// Set one Clip's Pattern-local source phase.
    ClipOffset {
        id: String,
        offset: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Change Clip source offset")]
        summary: String,
    },
    /// Set one Clip's whole-semitone transposition.
    ClipTranspose {
        id: String,
        #[arg(allow_hyphen_values = true)]
        semitones: i16,
        #[arg(long, default_value = "Transpose Timeline Clip")]
        summary: String,
    },
    /// Set one Clip's event-excitation gain.
    ClipGain {
        id: String,
        #[arg(allow_hyphen_values = true)]
        gain_db: f64,
        #[arg(long, default_value = "Change Clip gain")]
        summary: String,
    },
    /// Set one Clip's left/right fade durations.
    ClipFade {
        id: String,
        fade_in: String,
        fade_out: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Change Clip fades")]
        summary: String,
    },
    /// Set one pitched Event by zero-based Pattern Event index.
    NotePitch {
        pattern: String,
        event: usize,
        pitch: String,
        #[arg(long, default_value = "Change note pitch")]
        summary: String,
    },
    /// Set one note-or-hit onset in exact musical units.
    EventTime {
        pattern: String,
        event: usize,
        value: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Change Event onset")]
        summary: String,
    },
    /// Set one pitched-note duration in exact musical units.
    NoteDuration {
        pattern: String,
        event: usize,
        value: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Change note duration")]
        summary: String,
    },
    /// Set one note-or-hit velocity from 0 to 1.
    EventVelocity {
        pattern: String,
        event: usize,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Event velocity")]
        summary: String,
    },
    /// Set one percussion hit voice.
    HitVoice {
        pattern: String,
        event: usize,
        voice: String,
        #[arg(long, default_value = "Change percussion voice")]
        summary: String,
    },
    /// Place one durable sample occurrence in a Pattern.
    SamplePlace {
        pattern: String,
        id: String,
        mapping: String,
        at: String,
        duration: String,
        start_frame: u64,
        end_frame: u64,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
        gain_db: f64,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
        pan: f64,
        #[arg(long, default_value_t = 0)]
        fade_in: u32,
        #[arg(long, default_value_t = 0)]
        fade_out: u32,
        #[arg(long, default_value_t = 1.0)]
        velocity: f64,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Place sample occurrence")]
        summary: String,
    },
    /// Create adjacent one-key mappings from exact source-frame boundaries.
    SampleSliceMap {
        instrument: String,
        asset: String,
        prefix: String,
        start_key: String,
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        boundaries: Vec<u64>,
        #[arg(long, value_enum, default_value_t = SampleDirectionArg::Forward)]
        direction: SampleDirectionArg,
        #[arg(long, default_value = "Create sample slice map")]
        summary: String,
    },
    /// Move one durable sample occurrence.
    SampleMove {
        pattern: String,
        id: String,
        at: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Move sample occurrence")]
        summary: String,
    },
    /// Duplicate one durable sample occurrence.
    SampleDuplicate {
        pattern: String,
        id: String,
        new_id: String,
        at: String,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Duplicate sample occurrence")]
        summary: String,
    },
    /// Split one sample occurrence at matching timeline/source positions.
    SampleSplit {
        pattern: String,
        id: String,
        new_id: String,
        at: String,
        source_frame: u64,
        #[command(flatten)]
        timing: TimeGridArgs,
        #[arg(long, default_value = "Split sample occurrence")]
        summary: String,
    },
    /// Trim one sample occurrence to a half-open source region.
    SampleTrim {
        pattern: String,
        id: String,
        start_frame: u64,
        end_frame: u64,
        #[arg(long, default_value = "Trim sample occurrence")]
        summary: String,
    },
    /// Slip one sample occurrence by signed original-source frames.
    SampleSlip {
        pattern: String,
        id: String,
        #[arg(allow_hyphen_values = true)]
        frames: i64,
        #[arg(long, default_value = "Slip sample occurrence")]
        summary: String,
    },
    /// Set source-edge fades in original-source frames.
    SampleFade {
        pattern: String,
        id: String,
        fade_in: u32,
        fade_out: u32,
        #[arg(long, default_value = "Fade sample occurrence")]
        summary: String,
    },
    /// Set one occurrence-local sample traversal direction.
    SampleDirection {
        pattern: String,
        id: String,
        direction: SampleDirectionArg,
        #[arg(long, default_value = "Change sample direction")]
        summary: String,
    },
    /// Set one bounded occurrence-local sample loop.
    SampleLoop {
        pattern: String,
        id: String,
        start_frame: u64,
        end_frame: u64,
        #[arg(long, default_value_t = 0)]
        crossfade: u32,
        #[arg(long, default_value = "Change sample loop")]
        summary: String,
    },
    /// Clear an occurrence-local sample loop and inherit the mapping.
    SampleLoopClear {
        pattern: String,
        id: String,
        #[arg(long, default_value = "Clear sample loop")]
        summary: String,
    },
    /// Set one durable sample occurrence gain trim.
    SampleGain {
        pattern: String,
        id: String,
        #[arg(allow_hyphen_values = true)]
        gain_db: f64,
        #[arg(long, default_value = "Change sample gain")]
        summary: String,
    },
    /// Set one durable sample occurrence pan.
    SamplePan {
        pattern: String,
        id: String,
        #[arg(allow_hyphen_values = true)]
        pan: f64,
        #[arg(long, default_value = "Change sample pan")]
        summary: String,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum InstrumentRouteEditCommand {
    /// Set one Instrument-owned Modulation route depth from an exact physical literal.
    #[command(name = "instrument-route-depth")]
    Depth {
        instrument: String,
        route: String,
        #[arg(allow_hyphen_values = true)]
        depth: String,
        #[arg(long, default_value = "Change Instrument Modulation route depth")]
        summary: String,
    },
    /// Set one durable Instrument-owned Modulation route bypass value.
    #[command(name = "instrument-route-bypass")]
    Bypass {
        instrument: String,
        route: String,
        #[arg(action = clap::ArgAction::Set)]
        bypass: bool,
        #[arg(long, default_value = "Change Instrument Modulation route bypass")]
        summary: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum WavetableNormalizationArg {
    Preserve,
    GlobalPeak,
    FramePeak,
}

impl From<WavetableNormalizationArg> for WavetableNormalization {
    fn from(value: WavetableNormalizationArg) -> Self {
        match value {
            WavetableNormalizationArg::Preserve => Self::Preserve,
            WavetableNormalizationArg::GlobalPeak => Self::GlobalPeak,
            WavetableNormalizationArg::FramePeak => Self::FramePeak,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum WavetableMorphArg {
    Linear,
    CatmullRom,
}

impl From<WavetableMorphArg> for WavetableMorph {
    fn from(value: WavetableMorphArg) -> Self {
        match value {
            WavetableMorphArg::Linear => Self::Linear,
            WavetableMorphArg::CatmullRom => Self::CatmullRom,
        }
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum AssetCommand {
    /// Copy one external WAV into the Project and publish its declaration.
    Import {
        /// External WAV input; it is never retained in authoritative source.
        source: PathBuf,
        /// Durable Project-local identity and destination filename stem.
        #[arg(long)]
        id: String,
        /// Human-facing asset name; defaults to the ID.
        #[arg(long)]
        name: Option<String>,
        /// History summary.
        #[arg(long, default_value = "Import portable WAV asset")]
        summary: String,
    },
    /// Import one exact mono WAV as a bounded multi-frame wavetable.
    ImportWavetable {
        /// External mono WAV input; it is never retained as an authority dependency.
        source: PathBuf,
        /// Durable Project-local identity and destination filename stem.
        #[arg(long)]
        id: String,
        /// Human-facing asset name; defaults to the ID.
        #[arg(long)]
        name: Option<String>,
        /// Power-of-two samples in each periodic frame.
        #[arg(long)]
        frame_length: u32,
        /// Number of consecutive frames in the exact WAV.
        #[arg(long)]
        frames: u16,
        /// Source-amplitude policy applied before mip compilation.
        #[arg(long, value_enum, default_value_t = WavetableNormalizationArg::Preserve)]
        normalization: WavetableNormalizationArg,
        /// Deterministic inter-frame interpolation law.
        #[arg(long, value_enum, default_value_t = WavetableMorphArg::Linear)]
        morph: WavetableMorphArg,
        /// History summary.
        #[arg(long, default_value = "Import portable wavetable")]
        summary: String,
    },
    /// Bounce one exact Active Revision range and publish a generated WAV asset atomically.
    Bounce {
        /// `master` or `OWNER:TAP`, for example `track/lead:post_fader`.
        #[arg(long, default_value = "master")]
        source: String,
        /// Inclusive Song Locator ID or exact musical time.
        #[arg(long)]
        from: String,
        /// Exclusive Song Locator ID or exact musical time.
        #[arg(long)]
        to: String,
        /// Isolated post-range tail length in seconds.
        #[arg(long, default_value_t = 0.0)]
        tail_seconds: f64,
        /// New durable generated asset identity.
        #[arg(long)]
        id: String,
        /// Human-facing asset name; defaults to the ID.
        #[arg(long)]
        name: Option<String>,
        /// Publication origin class.
        #[arg(long, value_enum, default_value_t = OriginArg::Human)]
        origin: OriginArg,
        /// Human or Agent identity.
        #[arg(long, default_value = "cli")]
        actor: String,
        /// History summary.
        #[arg(long, default_value = "Bounce source range to portable asset")]
        summary: String,
        /// Durable rationale; required for Agent publications.
        #[arg(long, default_value = "")]
        explanation: String,
    },
    /// Prepare one exact asset region with independent output duration and pitch.
    Prepare {
        /// Existing durable source asset identity.
        #[arg(long)]
        source: String,
        /// Inclusive original-WAV source frame.
        #[arg(long)]
        from_frame: u64,
        /// Exclusive original-WAV source frame.
        #[arg(long)]
        to_frame: u64,
        /// Exact generated frame count at the Project sample rate.
        #[arg(long)]
        output_frames: u64,
        /// Independent pitch displacement in semitones.
        #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
        pitch_semitones: f64,
        /// Explicit formant behavior; preserve is currently rejected as unavailable.
        #[arg(long, value_enum, default_value_t = FormantPolicyArg::Shift)]
        formants: FormantPolicyArg,
        /// Versioned transform quality contract.
        #[arg(long, value_enum, default_value_t = SampleTransformQualityArg::EffectV1)]
        quality: SampleTransformQualityArg,
        /// New durable generated asset identity.
        #[arg(long)]
        id: String,
        /// Human-facing asset name; defaults to the ID.
        #[arg(long)]
        name: Option<String>,
        /// Publication origin class.
        #[arg(long, value_enum, default_value_t = OriginArg::Human)]
        origin: OriginArg,
        /// Human or Agent identity.
        #[arg(long, default_value = "cli")]
        actor: String,
        /// History summary.
        #[arg(long, default_value = "Prepare independent-duration sample asset")]
        summary: String,
        /// Durable rationale; required for Agent publications.
        #[arg(long, default_value = "")]
        explanation: String,
    },
    /// Collect declared WAVs into canonical `media/<asset-id>.wav` paths.
    Collect {
        /// History summary.
        #[arg(long, default_value = "Collect portable Project media")]
        summary: String,
    },
    /// List verified portable media without exposing host backing paths.
    List,
}

#[derive(Debug, Subcommand)]
pub(super) enum ModuleCommand {
    /// Export one validated Instrument to the personal library.
    Export {
        instrument: String,
        /// Personal library module name; defaults to the Instrument ID.
        #[arg(long)]
        name: Option<String>,
        /// Replace an existing module atomically.
        #[arg(long)]
        replace: bool,
    },
    /// Validate and copy a module source file into the personal library.
    Install {
        source: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        replace: bool,
    },
    /// List and search personal Instrument Modules by source-readable metadata.
    List {
        /// Case-insensitive text search across library name, Instrument ID/name, and tags.
        #[arg(long)]
        search: Option<String>,
        /// Require a source-readable tag; repeat for intersection matching.
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Parse and inspect one personal Instrument Module and its Project-asset dependencies.
    Inspect { name: String },
    /// Render the Module's bounded three-note preview to a float WAV.
    Preview {
        name: String,
        /// Explicit output WAV; never becomes Project authority implicitly.
        #[arg(long)]
        output: PathBuf,
        /// Replace an existing output atomically.
        #[arg(long)]
        replace: bool,
    },
    /// Materialize one personal Module beneath the current Project and report its import line.
    Copy {
        name: String,
        /// Project-local module name; defaults to the library name.
        #[arg(long)]
        local_name: Option<String>,
        /// Replace an existing exact destination atomically.
        #[arg(long)]
        replace: bool,
    },
    /// Materialize every `library:` import beneath the current Project.
    Collect,
}
