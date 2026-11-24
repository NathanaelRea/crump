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
    Target(EditTargetArgs),
    /// Set one exact Instrument Parameter in physical units.
    Parameter(EditParameterArgs),
    /// Freeze or unfreeze a Track for live playback.
    TrackFreeze(EditTrackFreezeArgs),
    /// Set one Track level in dB.
    TrackLevel(EditTrackLevelArgs),
    /// Set one Track pan value from -1 to 1.
    TrackPan(EditTrackPanArgs),
    /// Set one processor setting from an exact source literal.
    ProcessorSetting(EditProcessorSettingArgs),
    /// Edit one durable Instrument-owned Modulation route.
    ///
    /// Flattening keeps the public command names stable while bounding derive-time stack use.
    #[command(flatten)]
    InstrumentRoute(InstrumentRouteEditCommand),
    /// Set one durable source-backed processor bypass value.
    ProcessorBypass(EditProcessorBypassArgs),
    /// Move one processor by one position in its authored chain.
    ProcessorMove(EditProcessorMoveArgs),
    /// Create one absolute Song Automation lane with its required zero point.
    AutomationCreate(EditAutomationCreateArgs),
    /// Delete one complete Song Automation lane.
    AutomationDelete(EditAutomationDeleteArgs),
    /// Create one durable Automation point.
    AutomationPointCreate(EditAutomationPointCreateArgs),
    /// Set one Automation point's exact Song time.
    AutomationPointTime(EditAutomationPointTimeArgs),
    /// Set one Automation point's physical value.
    AutomationPointValue(EditAutomationPointValueArgs),
    /// Set the interpolation leaving one Automation point.
    AutomationPointCurve(EditAutomationPointCurveArgs),
    /// Delete one durable Automation point.
    AutomationPointDelete(EditAutomationPointDeleteArgs),
    /// Create one reusable deterministic Groove Template.
    GrooveCreate(EditGrooveCreateArgs),
    /// Update one reusable deterministic Groove Template.
    GrooveUpdate(EditGrooveUpdateArgs),
    /// Delete one unused Groove Template.
    GrooveDelete(EditGrooveDeleteArgs),
    /// Create one procedural Pattern Variation without flattening Events.
    PatternVariationCreate(EditPatternVariationCreateArgs),
    /// Update one procedural Pattern Variation's checked transforms.
    PatternVariationUpdate(EditPatternVariationUpdateArgs),
    /// Delete one unreferenced procedural Pattern Variation.
    PatternVariationDelete(EditPatternVariationDeleteArgs),
    /// Create one durable Pattern Parameter lock.
    ParameterLockCreate(EditParameterLockCreateArgs),
    /// Update one durable Pattern Parameter lock.
    ParameterLockUpdate(EditParameterLockUpdateArgs),
    /// Delete one durable Pattern Parameter lock.
    ParameterLockDelete(EditParameterLockDeleteArgs),
    /// Assign a Groove Template to a Clip, or omit GROOVE to clear it.
    ClipGroove(EditClipGrooveArgs),
    /// Insert empty arrangement time and shift later material.
    TimeInsert(EditTimeInsertArgs),
    /// Delete arrangement time; boundary-crossing Clips must be split first.
    TimeDelete(EditTimeDeleteArgs),
    /// Duplicate complete Clips into newly inserted arrangement time.
    TimeDuplicate(EditTimeDuplicateArgs),
    /// Create one named Song position for navigation and ranges.
    LocatorCreate(EditLocatorCreateArgs),
    /// Move one named Song Locator.
    LocatorMove(EditLocatorMoveArgs),
    /// Delete one named Song Locator.
    LocatorDelete(EditLocatorDeleteArgs),
    /// Create one readable Clip on a Timeline Track.
    ClipCreate(EditClipCreateArgs),
    /// Delete one complete Clip declaration.
    ClipDelete(EditClipDeleteArgs),
    /// Move one Clip to an exact Song time.
    ClipMove(EditClipMoveArgs),
    /// Resize one Clip to an exact positive duration.
    ClipResize(EditClipResizeArgs),
    /// Duplicate one Clip under a fresh identity.
    ClipDuplicate(EditClipDuplicateArgs),
    /// Split one Clip and give its right side a fresh identity.
    ClipSplit(EditClipSplitArgs),
    /// Set one Clip's Pattern-local source phase.
    ClipOffset(EditClipOffsetArgs),
    /// Set one Clip's whole-semitone transposition.
    ClipTranspose(EditClipTransposeArgs),
    /// Set one Clip's event-excitation gain.
    ClipGain(EditClipGainArgs),
    /// Set one Clip's left/right fade durations.
    ClipFade(EditClipFadeArgs),
    /// Set one pitched Event by zero-based Pattern Event index.
    NotePitch(EditNotePitchArgs),
    /// Set one note-or-hit onset in exact musical units.
    EventTime(EditEventTimeArgs),
    /// Set one pitched-note duration in exact musical units.
    NoteDuration(EditNoteDurationArgs),
    /// Set one note-or-hit velocity from 0 to 1.
    EventVelocity(EditEventVelocityArgs),
    /// Set one percussion hit voice.
    HitVoice(EditHitVoiceArgs),
    /// Place one durable sample occurrence in a Pattern.
    SamplePlace(EditSamplePlaceArgs),
    /// Create adjacent one-key mappings from exact source-frame boundaries.
    SampleSliceMap(EditSampleSliceMapArgs),
    /// Move one durable sample occurrence.
    SampleMove(EditSampleMoveArgs),
    /// Duplicate one durable sample occurrence.
    SampleDuplicate(EditSampleDuplicateArgs),
    /// Split one sample occurrence at matching timeline/source positions.
    SampleSplit(EditSampleSplitArgs),
    /// Trim one sample occurrence to a half-open source region.
    SampleTrim(EditSampleTrimArgs),
    /// Slip one sample occurrence by signed original-source frames.
    SampleSlip(EditSampleSlipArgs),
    /// Set source-edge fades in original-source frames.
    SampleFade(EditSampleFadeArgs),
    /// Set one occurrence-local sample traversal direction.
    SampleDirection(EditSampleDirectionArgs),
    /// Set one bounded occurrence-local sample loop.
    SampleLoop(EditSampleLoopArgs),
    /// Clear an occurrence-local sample loop and inherit the mapping.
    SampleLoopClear(EditSampleLoopClearArgs),
    /// Set one durable sample occurrence gain trim.
    SampleGain(EditSampleGainArgs),
    /// Set one durable sample occurrence pan.
    SamplePan(EditSamplePanArgs),
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

#[derive(Debug, Args)]
pub(super) struct EditTargetArgs {
    pub(super) target: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: f64,
    #[arg(long, default_value = "Change Parameter target")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditParameterArgs {
    pub(super) instrument: String,
    pub(super) parameter: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: f64,
    #[arg(long, default_value = "Change Instrument Parameter")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTrackFreezeArgs {
    pub(super) track: String,
    #[arg(action = clap::ArgAction::Set)]
    pub(super) frozen: bool,
    #[arg(long, default_value = "Change Track Freeze")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTrackLevelArgs {
    pub(super) track: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: f64,
    #[arg(long, default_value = "Change Track level")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTrackPanArgs {
    pub(super) track: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: f64,
    #[arg(long, default_value = "Change Track pan")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditProcessorSettingArgs {
    pub(super) processor: String,
    pub(super) setting: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[arg(long, default_value = "Change processor setting")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditProcessorBypassArgs {
    pub(super) processor: String,
    #[arg(action = clap::ArgAction::Set)]
    pub(super) bypass: bool,
    #[arg(long, default_value = "Change processor bypass")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditProcessorMoveArgs {
    pub(super) processor: String,
    pub(super) direction: ProcessorMoveArg,
    #[arg(long, default_value = "Reorder processor chain")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationCreateArgs {
    pub(super) lane: String,
    pub(super) target: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[arg(long, default_value = "Create Automation lane")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationDeleteArgs {
    pub(super) lane: String,
    #[arg(long, default_value = "Delete Automation lane")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationPointCreateArgs {
    pub(super) lane: String,
    pub(super) point: String,
    pub(super) time: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[arg(long, default_value = "linear")]
    pub(super) curve: String,
    #[arg(long, default_value = "Create Automation point")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationPointTimeArgs {
    pub(super) lane: String,
    pub(super) point: String,
    pub(super) time: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Move Automation point")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationPointValueArgs {
    pub(super) lane: String,
    pub(super) point: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[arg(long, default_value = "Change Automation value")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationPointCurveArgs {
    pub(super) lane: String,
    pub(super) point: String,
    pub(super) curve: String,
    #[arg(long, default_value = "Change Automation curve")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditAutomationPointDeleteArgs {
    pub(super) lane: String,
    pub(super) point: String,
    #[arg(long, default_value = "Delete Automation point")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditGrooveCreateArgs {
    pub(super) id: String,
    pub(super) grid: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) swing: f64,
    pub(super) timing: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) velocity: f64,
    #[arg(allow_hyphen_values = true)]
    pub(super) probability: f64,
    pub(super) seed: u64,
    #[arg(long, default_value = "Create Groove Template")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditGrooveUpdateArgs {
    pub(super) id: String,
    pub(super) grid: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) swing: f64,
    pub(super) timing: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) velocity: f64,
    #[arg(allow_hyphen_values = true)]
    pub(super) probability: f64,
    pub(super) seed: u64,
    #[arg(long, default_value = "Update Groove Template")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditGrooveDeleteArgs {
    pub(super) id: String,
    #[arg(long, default_value = "Delete Groove Template")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditPatternVariationCreateArgs {
    pub(super) id: String,
    pub(super) source: String,
    pub(super) rotate: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) transpose: i16,
    #[arg(allow_hyphen_values = true)]
    pub(super) velocity: f64,
    #[arg(action = clap::ArgAction::Set)]
    pub(super) reverse: bool,
    #[arg(allow_hyphen_values = true)]
    pub(super) probability: f64,
    pub(super) seed: u64,
    #[arg(long, default_value = "Create Pattern Variation")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditPatternVariationUpdateArgs {
    pub(super) id: String,
    pub(super) source: String,
    pub(super) rotate: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) transpose: i16,
    #[arg(allow_hyphen_values = true)]
    pub(super) velocity: f64,
    #[arg(action = clap::ArgAction::Set)]
    pub(super) reverse: bool,
    #[arg(allow_hyphen_values = true)]
    pub(super) probability: f64,
    pub(super) seed: u64,
    #[arg(long, default_value = "Update Pattern Variation")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditPatternVariationDeleteArgs {
    pub(super) id: String,
    #[arg(long, default_value = "Delete Pattern Variation")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditParameterLockCreateArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) at: String,
    pub(super) parameter: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Create Parameter lock")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditParameterLockUpdateArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) at: String,
    pub(super) parameter: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Update Parameter lock")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditParameterLockDeleteArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    #[arg(long, default_value = "Delete Parameter lock")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipGrooveArgs {
    pub(super) id: String,
    pub(super) groove: Option<String>,
    #[arg(long, default_value = "Assign Clip Groove")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTimeInsertArgs {
    pub(super) at: String,
    pub(super) duration: String,
    #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
    pub(super) automation: ArrangementAutomationArg,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Insert arrangement time")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTimeDeleteArgs {
    pub(super) at: String,
    pub(super) duration: String,
    #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
    pub(super) automation: ArrangementAutomationArg,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Delete arrangement time")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditTimeDuplicateArgs {
    pub(super) source_at: String,
    pub(super) duration: String,
    pub(super) destination: String,
    pub(super) suffix: String,
    #[arg(long, value_enum, default_value_t = ArrangementAutomationArg::Follow)]
    pub(super) automation: ArrangementAutomationArg,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Duplicate arrangement time")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditLocatorCreateArgs {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) at: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Create Song Locator")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditLocatorMoveArgs {
    pub(super) id: String,
    pub(super) at: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Move Song Locator")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditLocatorDeleteArgs {
    pub(super) id: String,
    #[arg(long, default_value = "Delete Song Locator")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipCreateArgs {
    pub(super) track: String,
    pub(super) id: String,
    pub(super) pattern: String,
    pub(super) start: String,
    pub(super) duration: String,
    #[arg(long, default_value = "once")]
    pub(super) mode: String,
    #[arg(long, default_value = "0beat")]
    pub(super) offset: String,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
    pub(super) transpose: i16,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
    pub(super) gain_db: f64,
    #[arg(long, default_value = "0beat")]
    pub(super) fade_in: String,
    #[arg(long, default_value = "0beat")]
    pub(super) fade_out: String,
    #[arg(long)]
    pub(super) groove: Option<String>,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Create Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipDeleteArgs {
    pub(super) id: String,
    #[arg(long, default_value = "Delete Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipMoveArgs {
    pub(super) id: String,
    pub(super) start: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Move Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipResizeArgs {
    pub(super) id: String,
    pub(super) duration: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Resize Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipDuplicateArgs {
    pub(super) id: String,
    pub(super) new_id: String,
    pub(super) start: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Duplicate Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipSplitArgs {
    pub(super) id: String,
    pub(super) new_id: String,
    pub(super) at: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Split Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipOffsetArgs {
    pub(super) id: String,
    pub(super) offset: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Change Clip source offset")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipTransposeArgs {
    pub(super) id: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) semitones: i16,
    #[arg(long, default_value = "Transpose Timeline Clip")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipGainArgs {
    pub(super) id: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) gain_db: f64,
    #[arg(long, default_value = "Change Clip gain")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditClipFadeArgs {
    pub(super) id: String,
    pub(super) fade_in: String,
    pub(super) fade_out: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Change Clip fades")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditNotePitchArgs {
    pub(super) pattern: String,
    pub(super) event: usize,
    pub(super) pitch: String,
    #[arg(long, default_value = "Change note pitch")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditEventTimeArgs {
    pub(super) pattern: String,
    pub(super) event: usize,
    pub(super) value: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Change Event onset")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditNoteDurationArgs {
    pub(super) pattern: String,
    pub(super) event: usize,
    pub(super) value: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Change note duration")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditEventVelocityArgs {
    pub(super) pattern: String,
    pub(super) event: usize,
    #[arg(allow_hyphen_values = true)]
    pub(super) value: f64,
    #[arg(long, default_value = "Change Event velocity")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditHitVoiceArgs {
    pub(super) pattern: String,
    pub(super) event: usize,
    pub(super) voice: String,
    #[arg(long, default_value = "Change percussion voice")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSamplePlaceArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) mapping: String,
    pub(super) at: String,
    pub(super) duration: String,
    pub(super) start_frame: u64,
    pub(super) end_frame: u64,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
    pub(super) gain_db: f64,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
    pub(super) pan: f64,
    #[arg(long, default_value_t = 0)]
    pub(super) fade_in: u32,
    #[arg(long, default_value_t = 0)]
    pub(super) fade_out: u32,
    #[arg(long, default_value_t = 1.0)]
    pub(super) velocity: f64,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Place sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleSliceMapArgs {
    pub(super) instrument: String,
    pub(super) asset: String,
    pub(super) prefix: String,
    pub(super) start_key: String,
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub(super) boundaries: Vec<u64>,
    #[arg(long, value_enum, default_value_t = SampleDirectionArg::Forward)]
    pub(super) direction: SampleDirectionArg,
    #[arg(long, default_value = "Create sample slice map")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleMoveArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) at: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Move sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleDuplicateArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) new_id: String,
    pub(super) at: String,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Duplicate sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleSplitArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) new_id: String,
    pub(super) at: String,
    pub(super) source_frame: u64,
    #[command(flatten)]
    pub(super) timing: TimeGridArgs,
    #[arg(long, default_value = "Split sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleTrimArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) start_frame: u64,
    pub(super) end_frame: u64,
    #[arg(long, default_value = "Trim sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleSlipArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) frames: i64,
    #[arg(long, default_value = "Slip sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleFadeArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) fade_in: u32,
    pub(super) fade_out: u32,
    #[arg(long, default_value = "Fade sample occurrence")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleDirectionArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) direction: SampleDirectionArg,
    #[arg(long, default_value = "Change sample direction")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleLoopArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    pub(super) start_frame: u64,
    pub(super) end_frame: u64,
    #[arg(long, default_value_t = 0)]
    pub(super) crossfade: u32,
    #[arg(long, default_value = "Change sample loop")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleLoopClearArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    #[arg(long, default_value = "Clear sample loop")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSampleGainArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) gain_db: f64,
    #[arg(long, default_value = "Change sample gain")]
    pub(super) summary: String,
}

#[derive(Debug, Args)]
pub(super) struct EditSamplePanArgs {
    pub(super) pattern: String,
    pub(super) id: String,
    #[arg(allow_hyphen_values = true)]
    pub(super) pan: f64,
    #[arg(long, default_value = "Change sample pan")]
    pub(super) summary: String,
}
