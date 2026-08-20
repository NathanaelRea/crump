//! Stable human and JSON command surface for external Authoring Agents and scripts.

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use serde_json::{Value, json};

use crate::{
    dsl::parse_module,
    edit::{
        EditError, apply_structured_edit, set_event_at, set_event_velocity, set_hit_voice,
        set_note_duration, set_note_pitch, set_parameter, set_track_level, set_track_pan,
    },
    engine::{CompiledProject, RenderOptions, inspect_render_destination, render_project},
    history::{ChangeOrigin, HistoryError, HistoryStore, Publication},
    model::{DrumVoice, parse_pitch},
    project::{MAX_SOURCE_BYTES, ProjectBundle, instrument_library_dir, load_project},
    realtime::{PipeWirePlayer, StreamStatus},
    validate::{check_musical_sketch, validate_instrument_module},
};

const COMMAND_SCHEMA: u32 = 1;
static MODULE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Crump command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "crump",
    version,
    about = "A source-backed musical sketch instrument"
)]
pub struct Cli {
    /// Project directory or `project.crump` path.
    #[arg(long, global = true, default_value = ".")]
    project: PathBuf,
    /// Emit exactly one versioned JSON response on stdout.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a valid blank Project and initial history entry.
    New(NewArgs),
    /// Parse and validate the current Candidate Revision.
    Validate,
    /// Inspect the complete parsed Candidate model and revision state.
    Inspect,
    /// Show Candidate and Active Revision state.
    Status,
    /// Explicitly promote the valid Candidate to Active Revision.
    Publish(PublishArgs),
    /// Run objective Musical Sketch acceptance checks.
    Check,
    /// Play the exact Active Revision through native PipeWire.
    Play(PlayArgs),
    /// Open the keyboard-complete Ratatui Production Workspace.
    Workspace,
    /// Render the exact Active Revision to an atomic float-WAV bundle.
    Render(RenderArgs),
    /// Read or restore append-only Project History.
    #[command(subcommand)]
    History(HistoryCommand),
    /// Apply one checked, source-preserving Production View edit.
    #[command(subcommand)]
    Edit(EditCommand),
    /// Export, install, inspect, or list reusable Instrument Modules.
    #[command(subcommand)]
    Module(ModuleCommand),
    /// Report host and Project support facts.
    Doctor,
}

#[derive(Debug, Args)]
struct NewArgs {
    /// New Project directory. Defaults to the global --project path.
    path: Option<PathBuf>,
    /// Human-facing Project name.
    #[arg(long)]
    name: Option<String>,
    /// Stable source ID.
    #[arg(long)]
    id: Option<String>,
}

#[derive(Debug, Args)]
struct PublishArgs {
    /// Publication origin class.
    #[arg(long, value_enum, default_value_t = OriginArg::Human)]
    origin: OriginArg,
    /// Human or Agent identity.
    #[arg(long, default_value = "cli")]
    actor: String,
    /// Concise history-list description.
    #[arg(long)]
    summary: String,
    /// Rationale. Required for Agent publications.
    #[arg(long, default_value = "")]
    explanation: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OriginArg {
    Human,
    Agent,
    System,
}

#[derive(Debug, Args)]
struct PlayArgs {
    /// Stop after this many seconds instead of the complete Render Range.
    #[arg(long)]
    seconds: Option<f64>,
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// Destination bundle directory.
    #[arg(long, default_value = "render/master")]
    output: PathBuf,
    /// Show the exact Active Render Range without writing output.
    #[arg(long)]
    plan: bool,
    /// Fail rather than atomically replace an existing complete bundle.
    #[arg(long)]
    no_replace: bool,
}

#[derive(Debug, Subcommand)]
enum HistoryCommand {
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
enum EditCommand {
    /// Set one exact Instrument Parameter in physical units.
    Parameter {
        instrument: String,
        parameter: String,
        #[arg(allow_hyphen_values = true)]
        value: f64,
        #[arg(long, default_value = "Change Instrument Parameter")]
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
        #[arg(long, default_value = "Change Event onset")]
        summary: String,
    },
    /// Set one pitched-note duration in exact musical units.
    NoteDuration {
        pattern: String,
        event: usize,
        value: String,
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
}

#[derive(Debug, Subcommand)]
enum ModuleCommand {
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
    /// List personal Instrument Modules.
    List,
    /// Parse and inspect one personal Instrument Module.
    Inspect { name: String },
}

/// Parses process arguments, executes one command, and returns a stable exit status.
#[must_use]
pub fn run() -> ExitCode {
    run_from(std::env::args_os())
}

/// Testable command entry point.
#[must_use]
pub fn run_from(arguments: impl IntoIterator<Item = OsString>) -> ExitCode {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let json_requested = arguments.iter().any(|argument| argument == "--json");
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let informational = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let exit_code = if informational { 0 } else { 2 };
            if json_requested {
                let response = if informational {
                    json!({
                        "schema_version": COMMAND_SCHEMA,
                        "ok": true,
                        "command": "help",
                        "data": { "text": error.to_string() },
                    })
                } else {
                    json!({
                        "schema_version": COMMAND_SCHEMA,
                        "ok": false,
                        "command": "cli",
                        "error": {
                            "code": "usage_error",
                            "message": error.to_string(),
                            "diagnostics": [],
                        },
                    })
                };
                println!("{}", serialize_response(&response));
            } else {
                let _ = error.print();
            }
            return ExitCode::from(exit_code);
        }
    };
    match execute(&cli) {
        Ok(success) => {
            emit_success(&cli, success);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            emit_failure(&cli, &failure);
            ExitCode::from(failure.exit_code)
        }
    }
}

#[derive(Debug)]
struct Success {
    command: &'static str,
    human: String,
    data: Value,
}

#[derive(Debug)]
struct Failure {
    command: &'static str,
    code: &'static str,
    message: String,
    exit_code: u8,
    diagnostics: Value,
}

impl Failure {
    fn operational(
        command: &'static str,
        code: &'static str,
        error: impl std::fmt::Display,
    ) -> Self {
        Self {
            command,
            code,
            message: error.to_string(),
            exit_code: 1,
            diagnostics: json!([]),
        }
    }

    fn blocked(
        command: &'static str,
        code: &'static str,
        message: impl Into<String>,
        diagnostics: Value,
    ) -> Self {
        Self {
            command,
            code,
            message: message.into(),
            exit_code: 2,
            diagnostics,
        }
    }

    fn conflict(command: &'static str, code: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            command,
            code,
            message: error.to_string(),
            exit_code: 3,
            diagnostics: json!([]),
        }
    }
}

fn execute(cli: &Cli) -> Result<Success, Failure> {
    match cli.command.as_ref() {
        Some(Command::New(args)) => command_new(cli, args),
        Some(Command::Validate) => command_validate(cli),
        Some(Command::Inspect) => command_inspect(cli),
        Some(Command::Status) => command_status(cli),
        Some(Command::Publish(args)) => command_publish(cli, args),
        Some(Command::Check) => command_check(cli),
        Some(Command::Play(args)) => command_play(cli, args),
        Some(Command::Workspace) => command_workspace(cli),
        Some(Command::Render(args)) => command_render(cli, args),
        Some(Command::History(command)) => command_history(cli, command),
        Some(Command::Edit(command)) => command_edit(cli, command),
        Some(Command::Module(command)) => command_module(cli, command),
        Some(Command::Doctor) => command_doctor(cli),
        None if !cli.json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() => {
            command_workspace(cli)
        }
        None => command_status(cli),
    }
}

fn command_new(cli: &Cli, args: &NewArgs) -> Result<Success, Failure> {
    let root = args.path.as_ref().unwrap_or(&cli.project);
    fs::create_dir_all(root).map_err(|error| Failure::operational("new", "io_error", error))?;
    let source_path = root.join(crate::project::PROJECT_FILE);
    if source_path.exists() {
        return Err(Failure::conflict(
            "new",
            "project_exists",
            format!("{} already exists", source_path.display()),
        ));
    }
    let fallback_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Untitled");
    let name = clean_name(args.name.as_deref().unwrap_or(fallback_name));
    let id = args.id.clone().unwrap_or_else(|| slug_id(&name));
    if !valid_id(&id) {
        return Err(Failure::blocked(
            "new",
            "invalid_project_id",
            "Project ID must start with an ASCII letter and use only letters, digits, `_`, or `-`",
            json!([]),
        ));
    }
    let source = blank_project_source(&name, &id);
    write_atomic_file(&source_path, source.as_bytes(), false)
        .map_err(|error| Failure::operational("new", "io_error", error))?;
    let bundle =
        load_project(root).map_err(|error| Failure::operational("new", "project_io", error))?;
    if bundle.diagnostics.has_errors() {
        return Err(Failure::blocked(
            "new",
            "generated_project_invalid",
            "generated Project did not validate",
            json!(bundle.diagnostics.as_slice()),
        ));
    }
    let store = HistoryStore::open(root)
        .map_err(|error| Failure::operational("new", "history_error", error))?;
    let entry = store
        .publish(
            &bundle,
            Publication::new(
                ChangeOrigin::system("crump new"),
                "Create blank Project",
                "Initial valid source state",
            ),
        )
        .map_err(|error| Failure::operational("new", "history_error", error))?;
    Ok(Success {
        command: "new",
        human: format!(
            "Created {} at {} (Active Revision {})",
            name,
            root.display(),
            bundle.revision
        ),
        data: json!({
            "root": bundle.root,
            "project_id": id,
            "revision": bundle.revision,
            "history_entry": entry,
        }),
    })
}

fn command_validate(cli: &Cli) -> Result<Success, Failure> {
    let bundle = load(cli, "validate")?;
    let valid = !bundle.diagnostics.has_errors();
    let success = Success {
        command: "validate",
        human: diagnostics_human(&bundle, "Candidate is valid"),
        data: json!({
            "valid": valid,
            "revision": bundle.revision,
            "diagnostics": bundle.diagnostics.as_slice(),
        }),
    };
    if valid {
        Ok(success)
    } else {
        Err(Failure::blocked(
            "validate",
            "validation_failed",
            "Candidate Revision is invalid",
            json!(bundle.diagnostics.as_slice()),
        ))
    }
}

fn command_inspect(cli: &Cli) -> Result<Success, Failure> {
    let bundle = load(cli, "inspect")?;
    let (active_entry, active_revision) = active_identity(&bundle, "inspect")?;
    let project = bundle.project.as_ref();
    let files: Vec<_> = bundle
        .sources
        .keys()
        .map(|path| path.display().to_string())
        .collect();
    Ok(Success {
        command: "inspect",
        human: if let Some(project) = project {
            format!(
                "{}: {} Instrument(s), {} Pattern(s), {} Track(s)\nCandidate {}",
                project.name,
                project.song.instruments.len(),
                project.song.patterns.len(),
                project.song.tracks.len(),
                bundle.revision
            )
        } else {
            diagnostics_human(&bundle, "Candidate could not be parsed")
        },
        data: json!({
            "candidate_revision": bundle.revision,
            "active_revision": active_revision,
            "active_entry": active_entry,
            "candidate_is_active": active_revision.as_ref() == Some(&bundle.revision),
            "source_files": files,
            "project": project,
            "diagnostics": bundle.diagnostics.as_slice(),
        }),
    })
}

fn command_status(cli: &Cli) -> Result<Success, Failure> {
    let bundle = load(cli, "status")?;
    let (active_entry, active_revision) = active_identity(&bundle, "status")?;
    let candidate_is_active = active_revision.as_ref() == Some(&bundle.revision);
    let valid = !bundle.diagnostics.has_errors();
    let state = if !valid {
        "invalid_candidate"
    } else if active_revision.is_none() {
        "unpublished_candidate"
    } else if candidate_is_active {
        "active"
    } else {
        "valid_candidate"
    };
    Ok(Success {
        command: "status",
        human: format!(
            "State: {state}\nCandidate: {} ({})\nActive: {}",
            bundle.revision,
            if valid { "valid" } else { "invalid" },
            active_revision
                .as_ref()
                .map_or("none", |revision| revision.0.as_str())
        ),
        data: json!({
            "state": state,
            "candidate_revision": bundle.revision,
            "candidate_valid": valid,
            "active_revision": active_revision,
            "active_entry": active_entry,
            "candidate_is_active": candidate_is_active,
            "diagnostics": bundle.diagnostics.as_slice(),
        }),
    })
}

fn command_publish(cli: &Cli, args: &PublishArgs) -> Result<Success, Failure> {
    let bundle = load(cli, "publish")?;
    if bundle.diagnostics.has_errors() || bundle.project.is_none() {
        return Err(Failure::blocked(
            "publish",
            "invalid_candidate",
            "invalid Candidate Revision was not published; the previous Active Revision is unchanged",
            json!(bundle.diagnostics.as_slice()),
        ));
    }
    let store = HistoryStore::open(&bundle.root)
        .map_err(|error| Failure::operational("publish", "history_error", error))?;
    if store
        .active_state()
        .map_err(|error| Failure::operational("publish", "history_error", error))?
        .is_some_and(|state| state.active_revision == bundle.revision)
    {
        return Err(Failure::conflict(
            "publish",
            "already_active",
            format!("Revision {} is already Active", bundle.revision),
        ));
    }
    let origin = match args.origin {
        OriginArg::Human => ChangeOrigin::human(&args.actor),
        OriginArg::Agent => ChangeOrigin::agent(&args.actor),
        OriginArg::System => ChangeOrigin::system(&args.actor),
    };
    let entry = store
        .publish(
            &bundle,
            Publication::new(origin, &args.summary, &args.explanation),
        )
        .map_err(publication_failure)?;
    Ok(Success {
        command: "publish",
        human: format!(
            "Published {} as history entry {}",
            bundle.revision, entry.id
        ),
        data: json!({ "revision": bundle.revision, "history_entry": entry }),
    })
}

fn publication_failure(error: HistoryError) -> Failure {
    match error {
        error @ HistoryError::StaleCandidate { .. } => {
            Failure::conflict("publish", "stale_candidate", error)
        }
        error @ HistoryError::AlreadyActive(_) => {
            Failure::conflict("publish", "already_active", error)
        }
        error @ (HistoryError::InvalidCandidate(_)
        | HistoryError::InvalidProvenance(_)
        | HistoryError::WrongProject) => Failure::blocked(
            "publish",
            "publication_rejected",
            error.to_string(),
            json!([]),
        ),
        error => Failure::operational("publish", "history_error", error),
    }
}

fn command_check(cli: &Cli) -> Result<Success, Failure> {
    let bundle = load(cli, "check")?;
    let Some(project) = bundle.project.as_ref() else {
        return Err(Failure::blocked(
            "check",
            "unparseable_candidate",
            "Candidate Project could not be parsed",
            json!(bundle.diagnostics.as_slice()),
        ));
    };
    let report = check_musical_sketch(project);
    if !report.passed {
        return Err(Failure::blocked(
            "check",
            "acceptance_failed",
            "Candidate does not satisfy objective Musical Sketch checks",
            json!(report),
        ));
    }
    Ok(Success {
        command: "check",
        human: "All objective Musical Sketch checks passed".to_owned(),
        data: json!(report),
    })
}

fn play_stop_key(key: crossterm::event::KeyEvent) -> bool {
    matches!(
        key.code,
        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc
    ) || matches!(key.code, crossterm::event::KeyCode::Char('c'))
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
}

fn command_play(cli: &Cli, args: &PlayArgs) -> Result<Success, Failure> {
    if args
        .seconds
        .is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0)
    {
        return Err(Failure::blocked(
            "play",
            "invalid_duration",
            "--seconds must be a positive finite value",
            json!([]),
        ));
    }
    let candidate = load(cli, "play")?;
    let active = {
        let store = HistoryStore::open(&candidate.root)
            .map_err(|error| Failure::operational("play", "history_error", error))?;
        store
            .active_bundle()
            .map_err(|error| Failure::operational("play", "history_error", error))?
            .ok_or_else(|| {
                Failure::blocked(
                    "play",
                    "no_active_revision",
                    "Project has no Active Revision; publish a valid Candidate first",
                    json!([]),
                )
            })?
    };
    let project = active.publishable().ok_or_else(|| {
        Failure::blocked(
            "play",
            "invalid_active_revision",
            "Active Revision is invalid",
            json!([]),
        )
    })?;
    let compiled = CompiledProject::compile(project)
        .map_err(|error| Failure::operational("play", "compile_failed", error))?;
    let total_frames = compiled.total_frames();
    let mut player = PipeWirePlayer::start(compiled.runtime())
        .map_err(|error| Failure::operational("play", "audio_unavailable", error))?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let raw_mode = RawModeGuard::new(interactive);
    let started = std::time::Instant::now();
    let mut report;
    let mut stream_failure = None;
    loop {
        report = player.poll();
        if report.stream_state == StreamStatus::Error {
            stream_failure = Some((
                "pipewire_stream_error",
                "PipeWire stream entered an error state".to_owned(),
            ));
            break;
        }
        if report.stream_state == StreamStatus::Stopped && report.callbacks > 0 {
            stream_failure = Some((
                "pipewire_stream_stopped",
                "PipeWire stream stopped before playback completed".to_owned(),
            ));
            break;
        }
        if report.callbacks == 0 && started.elapsed() >= std::time::Duration::from_secs(10) {
            stream_failure = Some((
                "pipewire_stream_timeout",
                format!(
                    "PipeWire stream produced no audio callbacks within 10 seconds (state: {:?})",
                    report.stream_state
                ),
            ));
            break;
        }
        if report.engine.frame.saturating_add(1) >= total_frames {
            break;
        }
        if args
            .seconds
            .is_some_and(|seconds| started.elapsed().as_secs_f64() >= seconds)
        {
            break;
        }
        if interactive
            && crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false)
            && let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read()
            && play_stop_key(key)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(raw_mode);
    player
        .shutdown()
        .map_err(|error| Failure::operational("play", "shutdown_failed", error))?;
    if let Some((code, message)) = stream_failure {
        return Err(Failure::operational("play", code, message));
    }
    Ok(Success {
        command: "play",
        human: format!(
            "Played Active Revision {} ({:.2}s, peak {:.3})",
            active.revision,
            started.elapsed().as_secs_f64(),
            report.engine.peak
        ),
        data: json!({
            "active_revision": active.revision,
            "candidate_revision": candidate.revision,
            "candidate_was_active": candidate.revision == active.revision,
            "elapsed_seconds": started.elapsed().as_secs_f64(),
            "telemetry": report,
        }),
    })
}

fn command_workspace(cli: &Cli) -> Result<Success, Failure> {
    if cli.json {
        return Err(Failure::blocked(
            "workspace",
            "interactive_command",
            "the Production Workspace is interactive and cannot use --json",
            json!([]),
        ));
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Failure::blocked(
            "workspace",
            "terminal_required",
            "the Production Workspace requires an interactive terminal",
            json!([]),
        ));
    }
    crate::workspace::run(&cli.project)
        .map_err(|error| Failure::operational("workspace", "workspace_error", error))?;
    Ok(Success {
        command: "workspace",
        human: "Production Workspace closed".to_owned(),
        data: json!({ "closed": true }),
    })
}

fn command_render(cli: &Cli, args: &RenderArgs) -> Result<Success, Failure> {
    let candidate = load(cli, "render")?;
    let active = {
        let store = HistoryStore::open(&candidate.root)
            .map_err(|error| Failure::operational("render", "history_error", error))?;
        store
            .active_bundle()
            .map_err(|error| Failure::operational("render", "history_error", error))?
            .ok_or_else(|| {
                Failure::blocked(
                    "render",
                    "no_active_revision",
                    "Project has no Active Revision; publish a valid Candidate first",
                    json!([]),
                )
            })?
    };
    let output = if args.output.is_absolute() {
        args.output.clone()
    } else {
        candidate.root.join(&args.output)
    };
    if args.plan {
        let project = active.project.as_ref().ok_or_else(|| {
            Failure::operational(
                "render",
                "render_failed",
                "Active Revision could not be parsed",
            )
        })?;
        let compiled = CompiledProject::compile(project)
            .map_err(|error| Failure::operational("render", "render_failed", error))?;
        let arrangement_frames = compiled.arrangement_frames();
        let total_frames = compiled.total_frames();
        let render_tail_frames = total_frames.saturating_sub(arrangement_frames);
        let duration_seconds = total_frames as f64 / f64::from(project.sample_rate);
        let (planned_destination, destination_exists) =
            inspect_render_destination(&active, &output)
                .map_err(|error| Failure::operational("render", "unsafe_destination", error))?;
        return Ok(Success {
            command: "render",
            human: format!(
                "Render Range: [0, {total_frames}) frames ({duration_seconds:.6} seconds)\nDestination: {}",
                planned_destination.display()
            ),
            data: json!({
                "candidate_revision": candidate.revision,
                "rendered_active_revision": active.revision,
                "candidate_was_active": candidate.revision == active.revision,
                "destination": planned_destination,
                "destination_exists": destination_exists,
                "would_replace": !args.no_replace && destination_exists,
                "would_conflict": args.no_replace && destination_exists,
                "render_range": {
                    "start_frame": 0,
                    "arrangement_end_frame": arrangement_frames,
                    "render_tail_frames": render_tail_frames,
                    "end_frame_exclusive": total_frames,
                    "sample_rate": project.sample_rate,
                    "duration_seconds": duration_seconds,
                },
            }),
        });
    }
    if args.no_replace && output.exists() {
        return Err(Failure::conflict(
            "render",
            "destination_exists",
            format!("render destination {} already exists", output.display()),
        ));
    }
    let mut options = RenderOptions::new(output);
    options.replace = !args.no_replace;
    let result = render_project(&active, &options)
        .map_err(|error| Failure::operational("render", "render_failed", error))?;
    Ok(Success {
        command: "render",
        human: format!(
            "Rendered {} frames to {}\nWAV SHA-256: {}",
            result.manifest.frame_count,
            result.wav.display(),
            result.manifest.wav_sha256
        ),
        data: json!({
            "candidate_revision": candidate.revision,
            "rendered_active_revision": active.revision,
            "candidate_was_active": candidate.revision == active.revision,
            "render": result,
        }),
    })
}

fn command_history(cli: &Cli, command: &HistoryCommand) -> Result<Success, Failure> {
    let bundle = load(cli, "history")?;
    let store = HistoryStore::open(&bundle.root)
        .map_err(|error| Failure::operational("history", "history_error", error))?;
    match command {
        HistoryCommand::List => {
            let entries = store
                .list()
                .map_err(|error| history_failure("history.list", error))?;
            let human = if entries.is_empty() {
                "Project History is empty".to_owned()
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}  {}  {:?}/{}  {}",
                            entry.id,
                            entry.revision,
                            entry.origin.kind,
                            entry.origin.name,
                            entry.summary
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(Success {
                command: "history.list",
                human,
                data: json!({ "entries": entries }),
            })
        }
        HistoryCommand::Show { id } => {
            let entry = store
                .entry(id)
                .map_err(|error| history_failure("history.show", error))?;
            let diff = store
                .diff(id)
                .map_err(|error| history_failure("history.show", error))?;
            Ok(Success {
                command: "history.show",
                human: format!(
                    "{} · {}\n{} / {}\n{}\n\n{}",
                    entry.id,
                    entry.summary,
                    entry.origin.name,
                    entry.created_at,
                    entry.explanation,
                    diff
                ),
                data: json!({ "entry": entry, "diff": diff }),
            })
        }
        HistoryCommand::Diff { id } => {
            let diff = store
                .diff(id)
                .map_err(|error| history_failure("history.diff", error))?;
            Ok(Success {
                command: "history.diff",
                human: diff.clone(),
                data: json!({ "id": id, "diff": diff }),
            })
        }
        HistoryCommand::Restore {
            id,
            explanation,
            actor,
        } => {
            let entry = store
                .restore(id, ChangeOrigin::human(actor), explanation)
                .map_err(|error| history_failure("history.restore", error))?;
            Ok(Success {
                command: "history.restore",
                human: format!("Restored {id} as new history entry {}", entry.id),
                data: json!({ "history_entry": entry }),
            })
        }
    }
}

fn history_failure(command: &'static str, error: HistoryError) -> Failure {
    match error {
        HistoryError::UnknownEntry(id) => Failure::blocked(
            command,
            "unknown_history_entry",
            format!("unknown committed history entry `{id}`"),
            json!([]),
        ),
        error @ (HistoryError::AlreadyActive(_) | HistoryError::StaleCandidate { .. }) => {
            Failure::conflict(command, "history_conflict", error)
        }
        error @ (HistoryError::InvalidCandidate(_)
        | HistoryError::InvalidProvenance(_)
        | HistoryError::WrongProject) => {
            Failure::blocked(command, "history_rejected", error.to_string(), json!([]))
        }
        error => Failure::operational(command, "history_error", error),
    }
}

fn command_edit(cli: &Cli, command: &EditCommand) -> Result<Success, Failure> {
    let bundle = load(cli, "edit")?;
    if bundle.diagnostics.has_errors() {
        return Err(Failure::blocked(
            "edit",
            "invalid_base",
            "Production View edits require a valid Active Revision",
            json!(bundle.diagnostics.as_slice()),
        ));
    }
    let (replacement, summary) = match command {
        EditCommand::Parameter {
            instrument,
            parameter,
            value,
            summary,
        } => (
            set_parameter(&bundle, instrument, parameter, *value).map_err(|error| {
                Failure::blocked(
                    "edit.parameter",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::TrackLevel {
            track,
            value,
            summary,
        } => (
            set_track_level(&bundle, track, *value).map_err(|error| {
                Failure::blocked(
                    "edit.track_level",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::TrackPan {
            track,
            value,
            summary,
        } => (
            set_track_pan(&bundle, track, *value).map_err(|error| {
                Failure::blocked(
                    "edit.track_pan",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::NotePitch {
            pattern,
            event,
            pitch,
            summary,
        } => {
            let parsed = parse_pitch(pitch).map_err(|error| {
                Failure::blocked(
                    "edit.note_pitch",
                    "invalid_pitch",
                    error.to_string(),
                    json!([]),
                )
            })?;
            (
                set_note_pitch(&bundle, pattern, *event, parsed).map_err(|error| {
                    Failure::blocked(
                        "edit.note_pitch",
                        "edit_rejected",
                        error.to_string(),
                        json!([]),
                    )
                })?,
                summary,
            )
        }
        EditCommand::EventTime {
            pattern,
            event,
            value,
            summary,
        } => (
            set_event_at(&bundle, pattern, *event, value).map_err(|error| {
                Failure::blocked(
                    "edit.event_time",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::NoteDuration {
            pattern,
            event,
            value,
            summary,
        } => (
            set_note_duration(&bundle, pattern, *event, value).map_err(|error| {
                Failure::blocked(
                    "edit.note_duration",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::EventVelocity {
            pattern,
            event,
            value,
            summary,
        } => (
            set_event_velocity(&bundle, pattern, *event, *value).map_err(|error| {
                Failure::blocked(
                    "edit.event_velocity",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::HitVoice {
            pattern,
            event,
            voice,
            summary,
        } => {
            let voice = voice.parse::<DrumVoice>().map_err(|error| {
                Failure::blocked(
                    "edit.hit_voice",
                    "invalid_drum_voice",
                    error.to_string(),
                    json!([]),
                )
            })?;
            (
                set_hit_voice(&bundle, pattern, *event, voice).map_err(|error| {
                    Failure::blocked(
                        "edit.hit_voice",
                        "edit_rejected",
                        error.to_string(),
                        json!([]),
                    )
                })?,
                summary,
            )
        }
    };
    let entry = apply_structured_edit(
        &bundle.root,
        &bundle.revision,
        vec![replacement],
        Publication::new(
            ChangeOrigin::human("cli"),
            summary,
            "Checked Production View edit",
        ),
    )
    .map_err(edit_transaction_failure)?;
    Ok(Success {
        command: "edit",
        human: format!("Applied source-backed edit as history entry {}", entry.id),
        data: json!({ "history_entry": entry }),
    })
}

fn edit_transaction_failure(error: EditError) -> Failure {
    match error {
        error @ (EditError::StaleRevision { .. }
        | EditError::CandidateNotActive { .. }
        | EditError::StaleTarget { .. }) => Failure::conflict("edit", "edit_conflict", error),
        EditError::InvalidResult(diagnostics) => Failure::blocked(
            "edit",
            "invalid_result",
            "Structured Source Edit produced an invalid Candidate Revision",
            json!(diagnostics),
        ),
        error @ (EditError::Empty
        | EditError::InvalidBase
        | EditError::OverlappingOrInvalidRange(_)
        | EditError::InvalidUtf8Boundary(_)
        | EditError::UnknownSource(_)
        | EditError::UnsafePath(_)
        | EditError::UnknownTarget(_)
        | EditError::WrongTargetKind(_)
        | EditError::InvalidValue(_)
        | EditError::OutOfRange { .. }) => {
            Failure::blocked("edit", "edit_rejected", error.to_string(), json!([]))
        }
        EditError::History(
            error @ (HistoryError::AlreadyActive(_) | HistoryError::StaleCandidate { .. }),
        ) => Failure::conflict("edit", "edit_conflict", error),
        error @ (EditError::Project(_) | EditError::History(_) | EditError::Io { .. }) => {
            Failure::operational("edit", "edit_transaction_failed", error)
        }
    }
}

fn command_module(cli: &Cli, command: &ModuleCommand) -> Result<Success, Failure> {
    match command {
        ModuleCommand::List => {
            let library = instrument_library_dir()
                .map_err(|error| Failure::operational("module.list", "library_error", error))?;
            fs::create_dir_all(&library)
                .map_err(|error| Failure::operational("module.list", "io_error", error))?;
            let mut modules = fs::read_dir(&library)
                .map_err(|error| Failure::operational("module.list", "io_error", error))?
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    (path
                        .extension()
                        .is_some_and(|extension| extension == "crump"))
                    .then_some(path)
                })
                .collect::<Vec<_>>();
            modules.sort();
            let names: Vec<_> = modules
                .iter()
                .filter_map(|path| path.file_stem().and_then(|name| name.to_str()))
                .map(str::to_owned)
                .collect();
            Ok(Success {
                command: "module.list",
                human: if names.is_empty() {
                    "No personal Instrument Modules".to_owned()
                } else {
                    names.join("\n")
                },
                data: json!({ "library": library, "modules": names }),
            })
        }
        ModuleCommand::Inspect { name } => {
            let path = library_module_path(name, "module.inspect")?;
            let bytes = read_module_file(&path, "module.inspect")?;
            let source = std::str::from_utf8(&bytes).map_err(|error| {
                Failure::blocked(
                    "module.inspect",
                    "invalid_utf8",
                    error.to_string(),
                    json!([]),
                )
            })?;
            let document = parse_module(&path, source);
            let mut diagnostics = document.diagnostics;
            diagnostics.extend(validate_instrument_module(&document.instruments).into_vec());
            let valid = !diagnostics.has_errors();
            if !valid {
                return Err(Failure::blocked(
                    "module.inspect",
                    "invalid_module",
                    "Instrument Module is invalid",
                    json!(diagnostics.as_slice()),
                ));
            }
            Ok(Success {
                command: "module.inspect",
                human: format!("{}: {} Instrument(s)", name, document.instruments.len()),
                data: json!({
                    "name": name,
                    "path": path,
                    "import": format!("library:{name}"),
                    "instruments": document.instruments,
                    "diagnostics": diagnostics.as_slice(),
                }),
            })
        }
        ModuleCommand::Install {
            source,
            name,
            replace,
        } => {
            let bytes = read_module_file(source, "module.install")?;
            let text = std::str::from_utf8(&bytes).map_err(|error| {
                Failure::blocked(
                    "module.install",
                    "invalid_utf8",
                    error.to_string(),
                    json!([]),
                )
            })?;
            let document = parse_module(source, text);
            let mut diagnostics = document.diagnostics;
            diagnostics.extend(validate_instrument_module(&document.instruments).into_vec());
            if diagnostics.has_errors() {
                return Err(Failure::blocked(
                    "module.install",
                    "invalid_module",
                    "Instrument Module is invalid and was not installed",
                    json!(diagnostics.as_slice()),
                ));
            }
            let fallback = source
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("instrument");
            let name = name.as_deref().unwrap_or(fallback);
            let path = library_module_path(name, "module.install")?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| Failure::operational("module.install", "io_error", error))?;
            }
            write_atomic_file(&path, &bytes, *replace)
                .map_err(|error| module_write_failure("module.install", error))?;
            Ok(Success {
                command: "module.install",
                human: format!(
                    "Installed {}\nAdd: import \"library:{}\"",
                    path.display(),
                    name
                ),
                data: json!({ "name": name, "path": path, "import": format!("library:{name}") }),
            })
        }
        ModuleCommand::Export {
            instrument,
            name,
            replace,
        } => {
            let bundle = load(cli, "module.export")?;
            if bundle.diagnostics.has_errors() {
                return Err(Failure::blocked(
                    "module.export",
                    "invalid_candidate",
                    "Instrument export requires a valid Candidate",
                    json!(bundle.diagnostics.as_slice()),
                ));
            }
            let authored = bundle
                .project
                .as_ref()
                .and_then(|project| {
                    project
                        .song
                        .instruments
                        .iter()
                        .find(|item| item.id == *instrument)
                })
                .ok_or_else(|| {
                    Failure::blocked(
                        "module.export",
                        "unknown_instrument",
                        format!("unknown Instrument `{instrument}`"),
                        json!([]),
                    )
                })?;
            let source = bundle.sources.get(&authored.location.file).ok_or_else(|| {
                Failure::operational(
                    "module.export",
                    "missing_source",
                    format!("source {} is unavailable", authored.location.file.display()),
                )
            })?;
            let declaration = source
                .get(authored.location.start..authored.location.end)
                .ok_or_else(|| {
                    Failure::operational(
                        "module.export",
                        "invalid_source_range",
                        "Instrument source range is invalid",
                    )
                })?;
            let module_source = format!("crump-module 1\n{declaration}\n");
            let parsed = parse_module(Path::new("export.crump"), &module_source);
            let mut diagnostics = parsed.diagnostics;
            diagnostics.extend(validate_instrument_module(&parsed.instruments).into_vec());
            if diagnostics.has_errors() {
                return Err(Failure::blocked(
                    "module.export",
                    "export_invalid",
                    "extracted Instrument did not form a valid module",
                    json!(diagnostics.as_slice()),
                ));
            }
            let name = name.as_deref().unwrap_or(instrument);
            let path = library_module_path(name, "module.export")?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| Failure::operational("module.export", "io_error", error))?;
            }
            write_atomic_file(&path, module_source.as_bytes(), *replace)
                .map_err(|error| module_write_failure("module.export", error))?;
            Ok(Success {
                command: "module.export",
                human: format!(
                    "Exported {}\nAdd: import \"library:{}\"",
                    path.display(),
                    name
                ),
                data: json!({ "name": name, "path": path, "import": format!("library:{name}") }),
            })
        }
    }
}

fn command_doctor(cli: &Cli) -> Result<Success, Failure> {
    let pipewire = std::process::Command::new("pw-cli")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let (project, project_error) = match load_project(&cli.project) {
        Ok(bundle) => (Some(bundle), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let compatible_platform = std::env::consts::OS == "linux" && std::env::consts::ARCH == "x86_64";
    let os_release = host_os_release_identity();
    let reference_environment =
        compatible_platform && os_release.as_deref() == Some("ubuntu/26.04");
    let pipewire_compiled = cfg!(feature = "pipewire");
    let project_valid = project
        .as_ref()
        .is_some_and(|bundle| !bundle.diagnostics.has_errors());
    let ready = reference_environment
        && pipewire_compiled
        && pipewire.is_some()
        && project.as_ref().is_none_or(|_| project_valid);
    Ok(Success {
        command: "doctor",
        human: format!(
            "Platform: {}-{} ({})\nOS release: {}\nPipeWire: {} (compiled support: {})\nProject: {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            if reference_environment {
                "first-release reference"
            } else if compatible_platform {
                "compatible, not reference-certified"
            } else {
                "unsupported"
            },
            os_release.as_deref().unwrap_or("unknown"),
            pipewire.as_deref().unwrap_or("pw-cli not detected"),
            pipewire_compiled,
            project.as_ref().map_or("not loaded", |_| if project_valid {
                "valid"
            } else {
                "invalid"
            })
        ),
        data: json!({
            "ready": ready,
            "platform": {
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "os_release": os_release,
                "compatible": compatible_platform,
                "reference_environment": reference_environment,
                "supported": reference_environment,
            },
            "pipewire": {
                "available": pipewire.is_some(),
                "cli_available": pipewire.is_some(),
                "compiled_support": pipewire_compiled,
                "version": pipewire,
            },
            "project": project.as_ref().map(|bundle| json!({
                "root": bundle.root,
                "revision": bundle.revision,
                "valid": !bundle.diagnostics.has_errors(),
                "diagnostics": bundle.diagnostics.as_slice(),
            })),
            "project_error": project_error,
        }),
    })
}

fn host_os_release_identity() -> Option<String> {
    let source = fs::read_to_string("/etc/os-release").ok()?;
    let value = |key: &str| {
        source.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name == key).then(|| value.trim().trim_matches('"').to_owned())
        })
    };
    let id = value("ID")?;
    let version = value("VERSION_ID")
        .or_else(|| value("BUILD_ID"))
        .unwrap_or_default();
    Some(format!("{id}/{version}"))
}

fn load(cli: &Cli, command: &'static str) -> Result<ProjectBundle, Failure> {
    load_project(&cli.project).map_err(|error| Failure::operational(command, "project_io", error))
}

fn active_identity(
    bundle: &ProjectBundle,
    command: &'static str,
) -> Result<(Option<String>, Option<crate::project::RevisionId>), Failure> {
    let store = HistoryStore::open(&bundle.root)
        .map_err(|error| Failure::operational(command, "history_error", error))?;
    let state = store
        .active_state()
        .map_err(|error| Failure::operational(command, "history_error", error))?;
    Ok(state.map_or((None, None), |state| {
        (Some(state.active_entry), Some(state.active_revision))
    }))
}

fn library_module_path(name: &str, command: &'static str) -> Result<PathBuf, Failure> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(Failure::blocked(
            command,
            "invalid_module_name",
            "module name must use only ASCII letters, digits, `_`, or `-`",
            json!([]),
        ));
    }
    instrument_library_dir()
        .map(|directory| directory.join(format!("{name}.crump")))
        .map_err(|error| Failure::operational(command, "library_error", error))
}

fn serialize_response(response: &Value) -> String {
    match serde_json::to_string(response) {
        Ok(response) => response,
        Err(_) => "{\"schema_version\":1,\"ok\":false,\"command\":\"cli\",\"error\":{\"code\":\"serialization_failed\",\"message\":\"could not serialize CLI response\",\"diagnostics\":[]}}"
        .to_owned(),
    }
}

fn emit_success(cli: &Cli, success: Success) {
    if cli.json {
        println!(
            "{}",
            serialize_response(&json!({
                "schema_version": COMMAND_SCHEMA,
                "ok": true,
                "command": success.command,
                "data": success.data,
            }))
        );
    } else {
        println!("{}", success.human);
    }
}

fn emit_failure(cli: &Cli, failure: &Failure) {
    if cli.json {
        println!(
            "{}",
            serialize_response(&json!({
                "schema_version": COMMAND_SCHEMA,
                "ok": false,
                "command": failure.command,
                "error": {
                    "code": failure.code,
                    "message": failure.message,
                    "diagnostics": failure.diagnostics,
                },
            }))
        );
    } else {
        eprintln!("crump: {}: {}", failure.code, failure.message);
        if let Some(diagnostics) = failure.diagnostics.as_array() {
            for diagnostic in diagnostics {
                if let Ok(diagnostic) =
                    serde_json::from_value::<crate::diagnostic::Diagnostic>(diagnostic.clone())
                {
                    eprintln!("{diagnostic}");
                }
            }
        }
    }
}

fn diagnostics_human(bundle: &ProjectBundle, empty: &str) -> String {
    if bundle.diagnostics.is_empty() {
        format!("{empty}\nRevision: {}", bundle.revision)
    } else {
        bundle
            .diagnostics
            .as_slice()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn blank_project_source(name: &str, id: &str) -> String {
    format!(
        "crump 1\nproject \"{name}\" {{\n  id {id}\n  seed 1\n  sample_rate 48000\n  render_tail 2s\n  song {{\n    tempo 120bpm\n    meter 4/4\n    end 16bar\n    instrument starter \"Starter Drums\" stock_drums {{\n      polyphony 8\n      parameter tone \"Tone\" ratio 0 1 0.5 0.5\n      faceplate {{\n        control tone \"Tone\" \"Percussion brightness\" maps tone\n      }}\n    }}\n    pattern empty 1bar {{\n    }}\n    track starter \"Starter\" instrument starter level -12db pan 0 {{\n    }}\n  }}\n}}\n"
    )
}

fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| !matches!(character, '"' | '\\' | '\n' | '\r'))
        .collect();
    if cleaned.trim().is_empty() {
        "Untitled".to_owned()
    } else {
        cleaned.trim().to_owned()
    }
}

fn slug_id(name: &str) -> String {
    let mut id = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    if id
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
    {
        id
    } else {
        format!("project-{id}")
    }
}

fn valid_id(id: &str) -> bool {
    id.chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

struct RawModeGuard(bool);

impl RawModeGuard {
    fn new(enable: bool) -> Self {
        let enabled = enable && crossterm::terminal::enable_raw_mode().is_ok();
        Self(enabled)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.0 {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

fn read_module_file(path: &Path, command: &'static str) -> Result<Vec<u8>, Failure> {
    let metadata =
        fs::metadata(path).map_err(|error| Failure::operational(command, "io_error", error))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(Failure::blocked(
            command,
            "module_too_large",
            format!(
                "Instrument Module is {} bytes; maximum is {MAX_SOURCE_BYTES}",
                metadata.len()
            ),
            json!([]),
        ));
    }
    fs::read(path).map_err(|error| Failure::operational(command, "io_error", error))
}

fn module_write_failure(command: &'static str, error: std::io::Error) -> Failure {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        Failure::conflict(command, "module_exists", error)
    } else {
        Failure::operational(command, "io_error", error)
    }
}

fn write_atomic_file(path: &Path, bytes: &[u8], replace: bool) -> Result<(), std::io::Error> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let counter = MODULE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{counter}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("crump"),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if replace {
            fs::rename(&temporary, path)?;
        } else {
            fs::hard_link(&temporary, path)?;
            fs::remove_file(&temporary)?;
        }
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_stops_interactive_playback() {
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        assert!(play_stop_key(key));
    }

    #[test]
    fn project_slug_is_source_safe() {
        assert_eq!(slug_id("My First Track!"), "my-first-track");
        assert_eq!(slug_id("123"), "project-123");
    }

    #[test]
    fn blank_project_parses_and_validates() {
        let document = crate::dsl::parse_project(
            Path::new("project.crump"),
            &blank_project_source("Test", "test"),
        );
        let project = document.project.unwrap();
        assert!(!crate::validate::validate_project(&project).has_errors());
    }
}
