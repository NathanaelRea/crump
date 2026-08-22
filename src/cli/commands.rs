use super::*;

pub(super) fn execute(cli: &Cli) -> Result<Success, Failure> {
    match cli.command.as_ref() {
        Some(Command::New(args)) => command_new(cli, args),
        Some(Command::Validate) => command_validate(cli),
        Some(Command::Inspect(args)) => command_inspect(cli, args),
        Some(Command::Status) => command_status(cli),
        Some(Command::Publish(args)) => command_publish(cli, args),
        Some(Command::Check) => command_check(cli),
        Some(Command::Play(args)) => command_play(cli, args),
        Some(Command::Workspace) => command_workspace(cli),
        Some(Command::Render(args)) => command_render(cli, args),
        Some(Command::Qualify(args)) => command_qualify(cli, args),
        Some(Command::History(command)) => command_history(cli, command),
        Some(Command::Edit(command)) => command_edit(cli, command),
        Some(Command::Asset(command)) => command_asset(cli, command),
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

fn command_inspect(cli: &Cli, args: &InspectArgs) -> Result<Success, Failure> {
    let bundle = load(cli, "inspect")?;
    let (active_entry, active_revision) = active_identity(&bundle, "inspect")?;
    let project = bundle.project.as_ref();
    let files: Vec<_> = bundle
        .sources
        .keys()
        .map(|path| path.display().to_string())
        .collect();
    let assets = portable_asset_data(&bundle);
    let (control_frame, effective_parameters, mix_graph) = if let Some(project) = project
        && !bundle.diagnostics.has_errors()
    {
        let at =
            crate::time::parse_musical_time(&args.at, project.song.meter).map_err(|error| {
                Failure::blocked(
                    "inspect",
                    "invalid_control_time",
                    error.to_string(),
                    json!([]),
                )
            })?;
        let frame = at
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .map_err(|error| {
                Failure::blocked(
                    "inspect",
                    "invalid_control_time",
                    error.to_string(),
                    json!([]),
                )
            })?;
        let compiled =
            CompiledProject::compile_with_assets(project, &bundle.assets).map_err(|error| {
                Failure::blocked(
                    "inspect",
                    "control_compile_failed",
                    error.to_string(),
                    json!([]),
                )
            })?;
        if frame > compiled.total_frames() {
            return Err(Failure::blocked(
                "inspect",
                "invalid_control_time",
                format!(
                    "requested control frame {frame} exceeds the inclusive render boundary {}",
                    compiled.total_frames()
                ),
                json!([]),
            ));
        }
        (
            Some(frame),
            Some(compiled.control_snapshot_at_frame(frame)),
            Some(compiled.mix_graph_inspection().clone()),
        )
    } else {
        (None, None, None)
    };
    let audition = if active_revision.as_ref() == Some(&bundle.revision)
        && let Some(project) = project
        && !bundle.diagnostics.has_errors()
    {
        Some(
            AuditionPlan::default()
                .prepare(&bundle.revision, project, &bundle.assets)
                .map_err(|error| {
                    Failure::blocked(
                        "inspect",
                        "audition_compile_failed",
                        error.to_string(),
                        json!([]),
                    )
                })?
                .inspection(),
        )
    } else {
        None
    };
    Ok(Success {
        command: "inspect",
        human: if let Some(project) = project {
            format!(
                "{}: {} Instrument(s), {} Pattern(s), {} Track(s), {} Automation lane(s)\nCandidate {}",
                project.name,
                project.song.instruments.len(),
                project.song.patterns.len(),
                project.song.tracks.len(),
                project.song.automations.len(),
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
            "assets": assets,
            "project": project,
            "control_frame": control_frame,
            "effective_parameters": effective_parameters,
            "mix_graph": mix_graph,
            "audition": audition,
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
    let origin = match args.origin {
        OriginArg::Human => ChangeOrigin::human(&args.actor),
        OriginArg::Agent => ChangeOrigin::agent(&args.actor),
        OriginArg::System => ChangeOrigin::system(&args.actor),
    };
    let outcome = execute_production_view(
        &cli.project,
        ProductionViewRequest {
            expected_revision: None,
            operation: ProductionViewOperation::PublishCandidate,
            provenance: ProductionViewProvenance {
                origin,
                summary: args.summary.clone(),
                explanation: args.explanation.clone(),
            },
        },
    )
    .map_err(|error| match error {
        ProductionViewError::InvalidBase { .. } => Failure::blocked(
            "publish",
            "invalid_candidate",
            "invalid Candidate Revision was not published; the previous Active Revision is unchanged",
            json!(error.diagnostics()),
        ),
        ProductionViewError::History(error) => publication_failure(error),
        ProductionViewError::Project(error) => {
            Failure::operational("publish", "project_io", error)
        }
        error => Failure::operational("publish", "history_error", error),
    })?;
    let revision = outcome.history_entry.revision.clone();
    Ok(Success {
        command: "publish",
        human: format!(
            "Published {} as history entry {}",
            revision, outcome.history_entry.id
        ),
        data: json!({ "revision": revision, "history_entry": outcome.history_entry }),
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

pub(super) fn play_stop_key(key: crossterm::event::KeyEvent) -> bool {
    matches!(
        key.code,
        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc
    ) || matches!(key.code, crossterm::event::KeyCode::Char('c'))
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
}

fn audition_plan(args: &PlayArgs) -> Result<AuditionPlan, Failure> {
    let parts = args.target.split('/').collect::<Vec<_>>();
    let target = match parts.as_slice() {
        ["song"] => AuditionTarget::Song,
        ["track", track] if !track.is_empty() => AuditionTarget::Track {
            track: (*track).to_owned(),
        },
        ["clip", clip] if !clip.is_empty() => AuditionTarget::Clip {
            clip: (*clip).to_owned(),
            track: None,
        },
        ["clip", track, clip] if !track.is_empty() && !clip.is_empty() => AuditionTarget::Clip {
            clip: (*clip).to_owned(),
            track: Some((*track).to_owned()),
        },
        ["pattern", pattern] if !pattern.is_empty() => AuditionTarget::Pattern {
            pattern: (*pattern).to_owned(),
            track: None,
        },
        ["pattern", pattern, track] if !pattern.is_empty() && !track.is_empty() => {
            AuditionTarget::Pattern {
                pattern: (*pattern).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        ["asset", asset] if !asset.is_empty() => AuditionTarget::Asset {
            asset: (*asset).to_owned(),
            start_frame: args.sample_start_frame,
            end_frame: args.sample_end_frame,
        },
        ["mapping", instrument, mapping] if !instrument.is_empty() && !mapping.is_empty() => {
            AuditionTarget::Mapping {
                instrument: (*instrument).to_owned(),
                mapping: (*mapping).to_owned(),
                track: None,
            }
        }
        ["mapping", instrument, mapping, track]
            if !instrument.is_empty() && !mapping.is_empty() && !track.is_empty() =>
        {
            AuditionTarget::Mapping {
                instrument: (*instrument).to_owned(),
                mapping: (*mapping).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        ["occurrence", pattern, occurrence] if !pattern.is_empty() && !occurrence.is_empty() => {
            AuditionTarget::Occurrence {
                pattern: (*pattern).to_owned(),
                occurrence: (*occurrence).to_owned(),
                track: None,
            }
        }
        ["occurrence", pattern, occurrence, track]
            if !pattern.is_empty() && !occurrence.is_empty() && !track.is_empty() =>
        {
            AuditionTarget::Occurrence {
                pattern: (*pattern).to_owned(),
                occurrence: (*occurrence).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        _ => {
            return Err(Failure::blocked(
                "play",
                "invalid_audition_target",
                "--target must be song, track/ID, clip/[TRACK/]ID, pattern/ID[/TRACK], asset/ID, mapping/INSTRUMENT/MAPPING[/TRACK], or occurrence/PATTERN/ID[/TRACK]",
                json!([]),
            ));
        }
    };
    if !matches!(target, AuditionTarget::Asset { .. })
        && (args.sample_start_frame.is_some() || args.sample_end_frame.is_some())
    {
        return Err(Failure::blocked(
            "play",
            "invalid_audition_target",
            "--sample-start-frame and --sample-end-frame require an asset target",
            json!([]),
        ));
    }
    if let (Some(start), Some(end)) = (args.sample_start_frame, args.sample_end_frame)
        && start >= end
    {
        return Err(Failure::blocked(
            "play",
            "invalid_audition_target",
            "sample region start must precede its exclusive end",
            json!([]),
        ));
    }
    let transient_bypass = args
        .bypass
        .iter()
        .cloned()
        .map(|processor| (processor, true))
        .collect::<BTreeMap<_, _>>();
    Ok(AuditionPlan {
        target,
        path: args.path.into(),
        range: AuditionRange {
            from: args.from.clone(),
            to: args.to.clone(),
            looping: args.loop_range,
        },
        transient_bypass,
        soloed_channels: Default::default(),
        cue: None,
    })
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
    let audition_plan = audition_plan(args)?;
    let prepared = audition_plan
        .prepare(&active.revision, project, &active.assets)
        .map_err(|error| {
            Failure::blocked(
                "play",
                "audition_prepare_failed",
                error.to_string(),
                json!([]),
            )
        })?;
    let audition_status = prepared.status().clone();
    let mix_graph = prepared.mix_graph_inspection().clone();
    let initial_inspection = prepared.inspection();
    let start_frame = audition_status.start_frame;
    let end_frame = audition_status.end_frame;
    let range_data = json!({
        "from_locator": args.from,
        "to_locator": args.to,
        "start_frame": start_frame,
        "end_frame": end_frame,
        "loop": audition_status.looping,
    });
    if args.plan {
        return Ok(Success {
            command: "play",
            human: format!(
                "Audition {:?} via {:?} over [{start_frame}, {end_frame}) frames{}",
                audition_status.target,
                audition_status.path,
                if audition_status.looping {
                    " · looped"
                } else {
                    ""
                }
            ),
            data: json!({
                "active_revision": active.revision,
                "candidate_revision": candidate.revision,
                "candidate_was_active": candidate.revision == active.revision,
                "range": range_data,
                "audition": initial_inspection,
            }),
        });
    }

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if args.loop_range && args.seconds.is_none() && !interactive {
        return Err(Failure::blocked(
            "play",
            "unbounded_loop",
            "non-interactive loop playback requires --seconds",
            json!([]),
        ));
    }
    let mut player = PipeWirePlayer::start(prepared.into_runtime())
        .map_err(|error| Failure::operational("play", "audio_unavailable", error))?;
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
        if !audition_status.looping && report.engine.frame.saturating_add(1) >= end_frame {
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
    let audition_inspection = inspection_from_parts(
        audition_status,
        &mix_graph,
        report.engine,
        &audition_plan.transient_bypass,
    );
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
            "range": range_data,
            "audition": audition_inspection,
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

    let project = if workspace_needs_project_picker(cli) {
        match select_project(&cli.project)
            .map_err(|error| Failure::operational("workspace", "project_selection", error))?
        {
            ProjectSelection::Selected(project) => project,
            ProjectSelection::Cancelled => {
                return Ok(Success {
                    command: "workspace",
                    human: "Project selection cancelled".to_owned(),
                    data: json!({ "closed": true, "opened": false }),
                });
            }
        }
    } else {
        cli.project.clone()
    };

    crate::workspace::run(&project)
        .map_err(|error| Failure::operational("workspace", "workspace_error", error))?;
    Ok(Success {
        command: "workspace",
        human: "Production Workspace closed".to_owned(),
        data: json!({ "closed": true, "opened": true, "project": project }),
    })
}

pub(super) fn workspace_needs_project_picker(cli: &Cli) -> bool {
    !cli.project_explicit && !cli.project.join(crate::project::PROJECT_FILE).is_file()
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
        let compiled = CompiledProject::compile_with_assets(project, &active.assets)
            .map_err(|error| Failure::operational("render", "render_failed", error))?;
        let stems = plan_render_stems(&compiled, &args.stems)
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
                "stems": stems,
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
    options.stems.clone_from(&args.stems);
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

fn command_qualify(cli: &Cli, args: &QualifyArgs) -> Result<Success, Failure> {
    if !args.maximum_p999_utilization.is_finite()
        || args.maximum_p999_utilization <= 0.0
        || args.maximum_p999_utilization > 1.0
    {
        return Err(Failure::blocked(
            "qualify",
            "invalid_utilization_margin",
            "--maximum-p999-utilization must be finite and in (0, 1]",
            json!([]),
        ));
    }
    let candidate = load(cli, "qualify")?;
    let active = {
        let store = HistoryStore::open(&candidate.root)
            .map_err(|error| Failure::operational("qualify", "history_error", error))?;
        store
            .active_bundle()
            .map_err(|error| Failure::operational("qualify", "history_error", error))?
            .ok_or_else(|| {
                Failure::blocked(
                    "qualify",
                    "no_active_revision",
                    "Project has no Active Revision; publish a valid Candidate first",
                    json!([]),
                )
            })?
    };
    let project = active.publishable().ok_or_else(|| {
        Failure::blocked(
            "qualify",
            "invalid_active_revision",
            "Active Revision is invalid",
            json!([]),
        )
    })?;
    let compiled = CompiledProject::compile_with_assets(project, &active.assets)
        .map_err(|error| Failure::operational("qualify", "compile_failed", error))?;
    let locator_frame = |locator_id: &str| {
        project
            .song
            .locators
            .iter()
            .find(|locator| locator.id == locator_id)
            .ok_or_else(|| {
                Failure::blocked(
                    "qualify",
                    "unknown_locator",
                    format!("unknown Song Locator `{locator_id}`"),
                    json!([]),
                )
            })?
            .at
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .map_err(|error| {
                Failure::blocked("qualify", "invalid_range", error.to_string(), json!([]))
            })
    };
    let start_frame = args.from.as_deref().map_or(Ok(0), locator_frame)?;
    let end_frame = args
        .to
        .as_deref()
        .map_or(Ok(compiled.total_frames()), locator_frame)?;
    if start_frame >= end_frame {
        return Err(Failure::blocked(
            "qualify",
            "invalid_range",
            "the qualification start must precede its end",
            json!([]),
        ));
    }
    let options = QualificationOptions {
        start_frame,
        end_frame,
        quantum_frames: args.quantum,
        maximum_p999_utilization: args.maximum_p999_utilization,
    };
    let output = args
        .output
        .as_ref()
        .map(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                candidate.root.join(path)
            };
            inspect_qualification_output(&candidate, &active, &path)
        })
        .transpose()
        .map_err(|error| {
            Failure::blocked(
                "qualify",
                "unsafe_destination",
                error.to_string(),
                json!([]),
            )
        })?;
    let resources = &compiled.mix_graph_inspection().resources;
    if args.plan {
        return Ok(Success {
            command: "qualify",
            human: format!(
                "Qualification range [{start_frame}, {end_frame}) at {} frames · offline plan only",
                args.quantum
            ),
            data: json!({
                "candidate_revision": candidate.revision,
                "qualified_active_revision": active.revision,
                "candidate_was_active": candidate.revision == active.revision,
                "range": {
                    "from_locator": args.from,
                    "to_locator": args.to,
                    "start_frame": start_frame,
                    "end_frame": end_frame,
                    "sample_rate": project.sample_rate,
                    "duration_seconds": (end_frame - start_frame) as f64 / f64::from(project.sample_rate),
                },
                "quantum_frames": args.quantum,
                "final_block_frames": ((end_frame - start_frame - 1) % args.quantum as u64 + 1),
                "maximum_p999_utilization": args.maximum_p999_utilization,
                "resources": resources,
                "report_path": output,
                "report_exists": output.as_ref().is_some_and(|path| path.exists()),
                "would_replace": args.replace && output.as_ref().is_some_and(|path| path.exists()),
                "would_conflict": !args.replace && output.as_ref().is_some_and(|path| path.exists()),
                "backend": "offline",
                "external_evidence_required": [
                    "PipeWire xrun count from the target graph",
                    "named-hardware 30-minute realtime endurance",
                    "callback allocation/blocking/I/O audit",
                ],
            }),
        });
    }

    if !args.replace
        && let Some(path) = &output
        && path.exists()
    {
        return Err(Failure::conflict(
            "qualify",
            "report_exists",
            format!("qualification report {} already exists", path.display()),
        ));
    }
    let report = qualify_compiled_project(&active, &compiled, options)
        .map_err(|error| Failure::operational("qualify", "qualification_failed", error))?;
    if let Some(path) = &output {
        let mut bytes = serde_json::to_vec_pretty(&report)
            .map_err(|error| Failure::operational("qualify", "report_failed", error))?;
        bytes.push(b'\n');
        if let Err(error) = write_atomic_file(path, &bytes, args.replace) {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(Failure::conflict(
                    "qualify",
                    "report_exists",
                    format!("qualification report {} already exists", path.display()),
                ));
            }
            return Err(Failure::operational("qualify", "report_failed", error));
        }
    }
    Ok(Success {
        command: "qualify",
        human: format!(
            "Profiled {} frames at quantum {} · p99.9 {:.3} · missed {} · {}",
            report.profiled_frames,
            report.actual_quantum_frames,
            report.p999_callback_utilization,
            report.missed_deadlines,
            if report.passed { "PASS" } else { "FAIL" }
        ),
        data: json!({
            "candidate_revision": candidate.revision,
            "qualified_active_revision": active.revision,
            "candidate_was_active": candidate.revision == active.revision,
            "report_path": output,
            "qualification": report,
        }),
    })
}

fn command_history(cli: &Cli, command: &HistoryCommand) -> Result<Success, Failure> {
    let bundle = load(cli, "history")?;
    match command {
        HistoryCommand::List => {
            let store = open_history(&bundle.root)?;
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
            let store = open_history(&bundle.root)?;
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
            let store = open_history(&bundle.root)?;
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
            let outcome = execute_production_view(
                &bundle.root,
                ProductionViewRequest {
                    expected_revision: Some(bundle.revision.clone()),
                    operation: ProductionViewOperation::RestoreHistory {
                        entry_id: id.clone(),
                    },
                    provenance: ProductionViewProvenance::human(
                        actor,
                        "Restore selected Project state",
                        explanation,
                    ),
                },
            )
            .map_err(|error| match error {
                ProductionViewError::History(error) => history_failure("history.restore", error),
                ProductionViewError::Project(error) => {
                    Failure::operational("history.restore", "project_io", error)
                }
                ProductionViewError::Edit(EditError::StaleRevision { .. }) => {
                    Failure::conflict("history.restore", "history_conflict", error)
                }
                error => Failure::operational("history.restore", "history_error", error),
            })?;
            Ok(Success {
                command: "history.restore",
                human: format!(
                    "Restored {id} as new history entry {}",
                    outcome.history_entry.id
                ),
                data: json!({ "history_entry": outcome.history_entry }),
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
    if let Some((operation, summary, operation_command)) = shared_edit_operation(command) {
        return execute_cli_production_view(cli, operation, summary, operation_command);
    }
    let bundle = load(cli, "edit")?;
    if bundle.diagnostics.has_errors() {
        return Err(Failure::blocked(
            "edit",
            "invalid_base",
            "Production View edits require a valid Active Revision",
            json!(bundle.diagnostics.as_slice()),
        ));
    }
    let multi_replacement_edit = match command {
        EditCommand::GrooveCreate {
            id,
            grid,
            swing,
            timing,
            velocity,
            probability,
            seed,
            summary,
        } => Some((
            vec![
                create_groove(
                    &bundle,
                    &GroovePlacement {
                        id: id.clone(),
                        grid: grid.clone(),
                        swing: *swing,
                        timing: timing.clone(),
                        velocity: *velocity,
                        probability: *probability,
                        seed: *seed,
                    },
                )
                .map_err(|error| sample_edit_failure("edit.groove_create", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::GrooveUpdate {
            id,
            grid,
            swing,
            timing,
            velocity,
            probability,
            seed,
            summary,
        } => Some((
            update_groove(
                &bundle,
                &GroovePlacement {
                    id: id.clone(),
                    grid: grid.clone(),
                    swing: *swing,
                    timing: timing.clone(),
                    velocity: *velocity,
                    probability: *probability,
                    seed: *seed,
                },
            )
            .map_err(|error| sample_edit_failure("edit.groove_update", error))?,
            summary.as_str(),
        )),
        EditCommand::GrooveDelete { id, summary } => Some((
            vec![
                delete_groove(&bundle, id)
                    .map_err(|error| sample_edit_failure("edit.groove_delete", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::PatternVariationCreate {
            id,
            source,
            rotate,
            transpose,
            velocity,
            reverse,
            probability,
            seed,
            summary,
        } => Some((
            vec![
                create_pattern_variation(
                    &bundle,
                    &PatternVariationPlacement {
                        id: id.clone(),
                        source: source.clone(),
                        rotate: rotate.clone(),
                        transpose_semitones: *transpose,
                        velocity_scale: *velocity,
                        reverse: *reverse,
                        probability: *probability,
                        seed: *seed,
                    },
                )
                .map_err(|error| sample_edit_failure("edit.pattern_variation_create", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::PatternVariationUpdate {
            id,
            source,
            rotate,
            transpose,
            velocity,
            reverse,
            probability,
            seed,
            summary,
        } => Some((
            update_pattern_variation(
                &bundle,
                &PatternVariationPlacement {
                    id: id.clone(),
                    source: source.clone(),
                    rotate: rotate.clone(),
                    transpose_semitones: *transpose,
                    velocity_scale: *velocity,
                    reverse: *reverse,
                    probability: *probability,
                    seed: *seed,
                },
            )
            .map_err(|error| sample_edit_failure("edit.pattern_variation_update", error))?,
            summary.as_str(),
        )),
        EditCommand::PatternVariationDelete { id, summary } => Some((
            vec![
                delete_pattern_variation(&bundle, id)
                    .map_err(|error| sample_edit_failure("edit.pattern_variation_delete", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::ParameterLockCreate {
            pattern,
            id,
            at,
            parameter,
            value,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.parameter_lock_create")?;
            Some((
                vec![
                    create_parameter_lock(
                        &bundle,
                        &ParameterLockPlacement {
                            pattern: pattern.clone(),
                            id: id.clone(),
                            at,
                            parameter: parameter.clone(),
                            value: value.clone(),
                        },
                    )
                    .map_err(|error| sample_edit_failure("edit.parameter_lock_create", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::ParameterLockUpdate {
            pattern,
            id,
            at,
            parameter,
            value,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.parameter_lock_update")?;
            Some((
                update_parameter_lock(
                    &bundle,
                    &ParameterLockPlacement {
                        pattern: pattern.clone(),
                        id: id.clone(),
                        at,
                        parameter: parameter.clone(),
                        value: value.clone(),
                    },
                )
                .map_err(|error| sample_edit_failure("edit.parameter_lock_update", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::ParameterLockDelete {
            pattern,
            id,
            summary,
        } => Some((
            vec![
                delete_parameter_lock(&bundle, pattern, id)
                    .map_err(|error| sample_edit_failure("edit.parameter_lock_delete", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::ClipGroove {
            id,
            groove,
            summary,
        } => Some((
            vec![
                set_clip_groove(&bundle, id, groove.as_deref())
                    .map_err(|error| sample_edit_failure("edit.clip_groove", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::TimeInsert {
            at,
            duration,
            automation,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.time_insert")?;
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.time_insert")?;
            Some((
                insert_arrangement_time(&bundle, &at, &duration, (*automation).into())
                    .map_err(|error| sample_edit_failure("edit.time_insert", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::TimeDelete {
            at,
            duration,
            automation,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.time_delete")?;
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.time_delete")?;
            Some((
                delete_arrangement_time(&bundle, &at, &duration, (*automation).into())
                    .map_err(|error| sample_edit_failure("edit.time_delete", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::TimeDuplicate {
            source_at,
            duration,
            destination,
            suffix,
            automation,
            timing,
            summary,
        } => {
            let source_at = resolve_edit_time(&bundle, source_at, timing, "edit.time_duplicate")?;
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.time_duplicate")?;
            let destination =
                resolve_edit_time(&bundle, destination, timing, "edit.time_duplicate")?;
            Some((
                duplicate_arrangement_time(
                    &bundle,
                    &source_at,
                    &duration,
                    &destination,
                    suffix,
                    (*automation).into(),
                )
                .map_err(|error| sample_edit_failure("edit.time_duplicate", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::LocatorCreate {
            id,
            name,
            at,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.locator_create")?;
            Some((
                vec![
                    create_locator(&bundle, id, name, &at)
                        .map_err(|error| sample_edit_failure("edit.locator_create", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::LocatorMove {
            id,
            at,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.locator_move")?;
            Some((
                vec![
                    move_locator(&bundle, id, &at)
                        .map_err(|error| sample_edit_failure("edit.locator_move", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::LocatorDelete { id, summary } => Some((
            vec![
                delete_locator(&bundle, id)
                    .map_err(|error| sample_edit_failure("edit.locator_delete", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::ClipCreate {
            track,
            id,
            pattern,
            start,
            duration,
            mode,
            offset,
            transpose,
            gain_db,
            fade_in,
            fade_out,
            groove,
            timing,
            summary,
        } => {
            let mode = mode.parse::<ClipMode>().map_err(|error| {
                Failure::blocked(
                    "edit.clip_create",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?;
            let start = resolve_edit_time(&bundle, start, timing, "edit.clip_create")?;
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.clip_create")?;
            let offset = resolve_edit_time(&bundle, offset, timing, "edit.clip_create")?;
            Some((
                vec![
                    create_clip(
                        &bundle,
                        track,
                        &ClipPlacement {
                            id: id.clone(),
                            pattern: pattern.clone(),
                            start,
                            duration,
                            mode,
                            source_offset: offset,
                            transpose_semitones: *transpose,
                            gain_db: *gain_db,
                            fade_in: fade_in.clone(),
                            fade_out: fade_out.clone(),
                            groove: groove.clone(),
                        },
                    )
                    .map_err(|error| sample_edit_failure("edit.clip_create", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::ClipDelete { id, summary } => Some((
            vec![
                delete_clip(&bundle, id)
                    .map_err(|error| sample_edit_failure("edit.clip_delete", error))?,
            ],
            summary.as_str(),
        )),
        EditCommand::ClipMove {
            id,
            start,
            timing,
            summary,
        } => {
            let start = resolve_edit_time(&bundle, start, timing, "edit.clip_move")?;
            Some((
                vec![
                    move_clip(&bundle, id, &start)
                        .map_err(|error| sample_edit_failure("edit.clip_move", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::ClipResize {
            id,
            duration,
            timing,
            summary,
        } => {
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.clip_resize")?;
            Some((
                vec![
                    resize_clip(&bundle, id, &duration)
                        .map_err(|error| sample_edit_failure("edit.clip_resize", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::ClipDuplicate {
            id,
            new_id,
            start,
            timing,
            summary,
        } => {
            let start = resolve_edit_time(&bundle, start, timing, "edit.clip_duplicate")?;
            Some((
                vec![
                    duplicate_clip(&bundle, id, new_id, &start)
                        .map_err(|error| sample_edit_failure("edit.clip_duplicate", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::ClipSplit {
            id,
            new_id,
            at,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.clip_split")?;
            Some((
                split_clip(&bundle, id, new_id, &at)
                    .map_err(|error| sample_edit_failure("edit.clip_split", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::ClipOffset {
            id,
            offset,
            timing,
            summary,
        } => {
            let offset = resolve_edit_time(&bundle, offset, timing, "edit.clip_offset")?;
            Some((
                set_clip_source_offset(&bundle, id, &offset)
                    .map_err(|error| sample_edit_failure("edit.clip_offset", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::ClipTranspose {
            id,
            semitones,
            summary,
        } => Some((
            set_clip_transpose(&bundle, id, *semitones)
                .map_err(|error| sample_edit_failure("edit.clip_transpose", error))?,
            summary.as_str(),
        )),
        EditCommand::ClipGain {
            id,
            gain_db,
            summary,
        } => Some((
            set_clip_gain(&bundle, id, *gain_db)
                .map_err(|error| sample_edit_failure("edit.clip_gain", error))?,
            summary.as_str(),
        )),
        EditCommand::ClipFade {
            id,
            fade_in,
            fade_out,
            timing,
            summary,
        } => {
            let fade_in = resolve_edit_time(&bundle, fade_in, timing, "edit.clip_fade")?;
            let fade_out = resolve_edit_time(&bundle, fade_out, timing, "edit.clip_fade")?;
            Some((
                set_clip_fades(&bundle, id, &fade_in, &fade_out)
                    .map_err(|error| sample_edit_failure("edit.clip_fade", error))?,
                summary.as_str(),
            ))
        }
        EditCommand::SamplePlace {
            pattern,
            id,
            mapping,
            at,
            duration,
            start_frame,
            end_frame,
            gain_db,
            pan,
            fade_in,
            fade_out,
            velocity,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.sample_place")?;
            let duration = resolve_edit_time(&bundle, duration, timing, "edit.sample_place")?;
            Some((
                vec![
                    place_sample_occurrence(
                        &bundle,
                        pattern,
                        &SamplePlacement {
                            id: id.clone(),
                            mapping: mapping.clone(),
                            at,
                            duration,
                            start_frame: *start_frame,
                            end_frame: *end_frame,
                            gain_db: *gain_db,
                            pan: *pan,
                            fade_in_frames: *fade_in,
                            fade_out_frames: *fade_out,
                            velocity: *velocity,
                        },
                    )
                    .map_err(|error| sample_edit_failure("edit.sample_place", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::SampleMove {
            pattern,
            id,
            at,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.sample_move")?;
            Some((
                vec![
                    move_sample_occurrence(&bundle, pattern, id, &at)
                        .map_err(|error| sample_edit_failure("edit.sample_move", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::SampleDuplicate {
            pattern,
            id,
            new_id,
            at,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.sample_duplicate")?;
            Some((
                vec![
                    duplicate_sample_occurrence(&bundle, pattern, id, new_id, &at)
                        .map_err(|error| sample_edit_failure("edit.sample_duplicate", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::SampleSplit {
            pattern,
            id,
            new_id,
            at,
            source_frame,
            timing,
            summary,
        } => {
            let at = resolve_edit_time(&bundle, at, timing, "edit.sample_split")?;
            Some((
                vec![
                    split_sample_occurrence(&bundle, pattern, id, new_id, &at, *source_frame)
                        .map_err(|error| sample_edit_failure("edit.sample_split", error))?,
                ],
                summary.as_str(),
            ))
        }
        EditCommand::SampleTrim { .. } => unreachable!("shared operation returned above"),
        EditCommand::SampleSlip {
            pattern,
            id,
            frames,
            summary,
        } => Some((
            slip_sample_occurrence(&bundle, pattern, id, *frames)
                .map_err(|error| sample_edit_failure("edit.sample_slip", error))?,
            summary.as_str(),
        )),
        EditCommand::SampleFade { .. }
        | EditCommand::SampleGain { .. }
        | EditCommand::SamplePan { .. } => unreachable!("shared operation returned above"),
        _ => None,
    };
    if let Some((replacements, summary)) = multi_replacement_edit {
        return commit_cli_edit(&bundle, replacements, summary);
    }
    let (replacement, summary) = match command {
        EditCommand::Target { .. }
        | EditCommand::Parameter { .. }
        | EditCommand::TrackLevel { .. }
        | EditCommand::TrackPan { .. }
        | EditCommand::ProcessorSetting { .. }
        | EditCommand::ProcessorBypass { .. }
        | EditCommand::ProcessorMove { .. } => unreachable!("shared operation returned above"),
        EditCommand::AutomationCreate {
            lane,
            target,
            value,
            summary,
        } => {
            let target = target.parse::<ParameterTarget>().map_err(|error| {
                Failure::blocked(
                    "edit.automation_create",
                    "invalid_target",
                    error.to_string(),
                    json!([]),
                )
            })?;
            (
                create_automation_lane(&bundle, lane, &target, value).map_err(|error| {
                    Failure::blocked(
                        "edit.automation_create",
                        "edit_rejected",
                        error.to_string(),
                        json!([]),
                    )
                })?,
                summary,
            )
        }
        EditCommand::AutomationDelete { lane, summary } => (
            delete_automation_lane(&bundle, lane).map_err(|error| {
                Failure::blocked(
                    "edit.automation_delete",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::AutomationPointCreate {
            lane,
            point,
            time,
            timing,
            value,
            curve,
            summary,
        } => {
            let time = resolve_edit_time(&bundle, time, timing, "edit.automation_point_create")?;
            let curve = curve.parse::<AutomationInterpolation>().map_err(|error| {
                Failure::blocked(
                    "edit.automation_point_create",
                    "invalid_curve",
                    error.to_string(),
                    json!([]),
                )
            })?;
            (
                create_automation_point(&bundle, lane, point, &time, value, curve).map_err(
                    |error| {
                        Failure::blocked(
                            "edit.automation_point_create",
                            "edit_rejected",
                            error.to_string(),
                            json!([]),
                        )
                    },
                )?,
                summary,
            )
        }
        EditCommand::AutomationPointTime { .. }
        | EditCommand::AutomationPointValue { .. }
        | EditCommand::AutomationPointCurve { .. } => {
            unreachable!("shared operation returned above")
        }
        EditCommand::AutomationPointDelete {
            lane,
            point,
            summary,
        } => (
            delete_automation_point(&bundle, lane, point).map_err(|error| {
                Failure::blocked(
                    "edit.automation_point_delete",
                    "edit_rejected",
                    error.to_string(),
                    json!([]),
                )
            })?,
            summary,
        ),
        EditCommand::NotePitch { .. }
        | EditCommand::EventTime { .. }
        | EditCommand::NoteDuration { .. }
        | EditCommand::EventVelocity { .. }
        | EditCommand::HitVoice { .. } => unreachable!("shared operation returned above"),
        EditCommand::GrooveCreate { .. }
        | EditCommand::GrooveUpdate { .. }
        | EditCommand::GrooveDelete { .. }
        | EditCommand::PatternVariationCreate { .. }
        | EditCommand::PatternVariationUpdate { .. }
        | EditCommand::PatternVariationDelete { .. }
        | EditCommand::ParameterLockCreate { .. }
        | EditCommand::ParameterLockUpdate { .. }
        | EditCommand::ParameterLockDelete { .. }
        | EditCommand::ClipGroove { .. }
        | EditCommand::TimeInsert { .. }
        | EditCommand::TimeDelete { .. }
        | EditCommand::TimeDuplicate { .. }
        | EditCommand::LocatorCreate { .. }
        | EditCommand::LocatorMove { .. }
        | EditCommand::LocatorDelete { .. }
        | EditCommand::ClipCreate { .. }
        | EditCommand::ClipDelete { .. }
        | EditCommand::ClipMove { .. }
        | EditCommand::ClipResize { .. }
        | EditCommand::ClipDuplicate { .. }
        | EditCommand::ClipSplit { .. }
        | EditCommand::ClipOffset { .. }
        | EditCommand::ClipTranspose { .. }
        | EditCommand::ClipGain { .. }
        | EditCommand::ClipFade { .. }
        | EditCommand::SamplePlace { .. }
        | EditCommand::SampleMove { .. }
        | EditCommand::SampleDuplicate { .. }
        | EditCommand::SampleSplit { .. }
        | EditCommand::SampleTrim { .. }
        | EditCommand::SampleSlip { .. }
        | EditCommand::SampleFade { .. }
        | EditCommand::SampleGain { .. }
        | EditCommand::SamplePan { .. } => unreachable!("multi-replacement edits returned above"),
    };
    commit_cli_edit(&bundle, vec![replacement], summary)
}

fn shared_edit_operation(
    command: &EditCommand,
) -> Option<(ProductionViewOperation, &str, &'static str)> {
    let result = match command {
        EditCommand::Target {
            target,
            value,
            summary,
        } => (
            ProductionViewOperation::SetParameterTarget {
                target: target.clone(),
                value: PhysicalValue::Number(*value),
            },
            summary.as_str(),
            "edit.target",
        ),
        EditCommand::Parameter {
            instrument,
            parameter,
            value,
            summary,
        } => (
            ProductionViewOperation::SetInstrumentParameter {
                instrument: instrument.clone(),
                parameter: parameter.clone(),
                value: PhysicalValue::Number(*value),
            },
            summary.as_str(),
            "edit.parameter",
        ),
        EditCommand::TrackLevel {
            track,
            value,
            summary,
        } => (
            ProductionViewOperation::SetTrackLevel {
                track: track.clone(),
                value: PhysicalValue::Number(*value),
            },
            summary.as_str(),
            "edit.track_level",
        ),
        EditCommand::TrackPan {
            track,
            value,
            summary,
        } => (
            ProductionViewOperation::SetTrackPan {
                track: track.clone(),
                value: PhysicalValue::Number(*value),
            },
            summary.as_str(),
            "edit.track_pan",
        ),
        EditCommand::ProcessorSetting {
            processor,
            setting,
            value,
            summary,
        } => (
            ProductionViewOperation::SetProcessorSetting {
                processor: processor.clone(),
                setting: setting.clone(),
                value: value.clone(),
            },
            summary.as_str(),
            "edit.processor_setting",
        ),
        EditCommand::ProcessorBypass {
            processor,
            bypass,
            summary,
        } => (
            ProductionViewOperation::SetProcessorBypass {
                processor: processor.clone(),
                bypass: *bypass,
            },
            summary.as_str(),
            "edit.processor_bypass",
        ),
        EditCommand::ProcessorMove {
            processor,
            direction,
            summary,
        } => (
            ProductionViewOperation::MoveProcessor {
                processor: processor.clone(),
                direction: match direction {
                    ProcessorMoveArg::Earlier => MoveDirection::Earlier,
                    ProcessorMoveArg::Later => MoveDirection::Later,
                },
            },
            summary.as_str(),
            "edit.processor_move",
        ),
        EditCommand::AutomationPointTime {
            lane,
            point,
            time,
            timing,
            summary,
        } => (
            ProductionViewOperation::SetAutomationPointTime {
                lane: lane.clone(),
                point: point.clone(),
                time: time.clone(),
                snap_to: timing.snap.then(|| timing.grid.clone()).flatten(),
            },
            summary.as_str(),
            "edit.automation_point_time",
        ),
        EditCommand::AutomationPointValue {
            lane,
            point,
            value,
            summary,
        } => (
            ProductionViewOperation::SetAutomationPointValue {
                lane: lane.clone(),
                point: point.clone(),
                value: value.clone(),
            },
            summary.as_str(),
            "edit.automation_point_value",
        ),
        EditCommand::AutomationPointCurve {
            lane,
            point,
            curve,
            summary,
        } => (
            ProductionViewOperation::SetAutomationPointCurve {
                lane: lane.clone(),
                point: point.clone(),
                curve: curve.clone(),
            },
            summary.as_str(),
            "edit.automation_point_curve",
        ),
        EditCommand::NotePitch {
            pattern,
            event,
            pitch,
            summary,
        } => (
            ProductionViewOperation::SetNotePitch {
                pattern: pattern.clone(),
                event: *event,
                pitch: pitch.clone(),
            },
            summary.as_str(),
            "edit.note_pitch",
        ),
        EditCommand::EventTime {
            pattern,
            event,
            value,
            timing,
            summary,
        } => (
            ProductionViewOperation::SetEventTime {
                pattern: pattern.clone(),
                event: *event,
                time: value.clone(),
                snap_to: timing.snap.then(|| timing.grid.clone()).flatten(),
            },
            summary.as_str(),
            "edit.event_time",
        ),
        EditCommand::NoteDuration {
            pattern,
            event,
            value,
            timing,
            summary,
        } => (
            ProductionViewOperation::SetNoteDuration {
                pattern: pattern.clone(),
                event: *event,
                duration: value.clone(),
                snap_to: timing.snap.then(|| timing.grid.clone()).flatten(),
            },
            summary.as_str(),
            "edit.note_duration",
        ),
        EditCommand::EventVelocity {
            pattern,
            event,
            value,
            summary,
        } => (
            ProductionViewOperation::SetEventVelocity {
                pattern: pattern.clone(),
                event: *event,
                value: PhysicalValue::Number(*value),
            },
            summary.as_str(),
            "edit.event_velocity",
        ),
        EditCommand::HitVoice {
            pattern,
            event,
            voice,
            summary,
        } => (
            ProductionViewOperation::SetHitVoice {
                pattern: pattern.clone(),
                event: *event,
                voice: voice.clone(),
            },
            summary.as_str(),
            "edit.hit_voice",
        ),
        EditCommand::SampleTrim {
            pattern,
            id,
            start_frame,
            end_frame,
            summary,
        } => (
            ProductionViewOperation::TrimSample {
                pattern: pattern.clone(),
                occurrence: id.clone(),
                start_frame: *start_frame,
                end_frame: *end_frame,
            },
            summary.as_str(),
            "edit.sample_trim",
        ),
        EditCommand::SampleFade {
            pattern,
            id,
            fade_in,
            fade_out,
            summary,
        } => (
            ProductionViewOperation::FadeSample {
                pattern: pattern.clone(),
                occurrence: id.clone(),
                fade_in_frames: *fade_in,
                fade_out_frames: *fade_out,
            },
            summary.as_str(),
            "edit.sample_fade",
        ),
        EditCommand::SampleGain {
            pattern,
            id,
            gain_db,
            summary,
        } => (
            ProductionViewOperation::SetSampleGain {
                pattern: pattern.clone(),
                occurrence: id.clone(),
                value: PhysicalValue::Number(*gain_db),
            },
            summary.as_str(),
            "edit.sample_gain",
        ),
        EditCommand::SamplePan {
            pattern,
            id,
            pan,
            summary,
        } => (
            ProductionViewOperation::SetSamplePan {
                pattern: pattern.clone(),
                occurrence: id.clone(),
                value: PhysicalValue::Number(*pan),
            },
            summary.as_str(),
            "edit.sample_pan",
        ),
        _ => return None,
    };
    Some(result)
}

fn execute_cli_production_view(
    cli: &Cli,
    operation: ProductionViewOperation,
    summary: &str,
    operation_command: &'static str,
) -> Result<Success, Failure> {
    debug_assert!(crate::production_view::is_shared_edit(&operation));
    let outcome = execute_production_view(
        &cli.project,
        ProductionViewRequest {
            expected_revision: None,
            operation,
            provenance: ProductionViewProvenance::human(
                "cli",
                summary,
                "Checked Production View edit",
            ),
        },
    )
    .map_err(|error| production_view_failure(operation_command, error))?;
    Ok(Success {
        command: "edit",
        human: format!(
            "Applied source-backed edit as history entry {}",
            outcome.history_entry.id
        ),
        data: json!({ "history_entry": outcome.history_entry }),
    })
}

fn production_view_failure(command: &'static str, error: ProductionViewError) -> Failure {
    match error {
        ProductionViewError::InvalidBase { .. } => Failure::blocked(
            "edit",
            "invalid_base",
            error.to_string(),
            json!(error.diagnostics()),
        ),
        ProductionViewError::InvalidOperationValue { .. } => {
            let code = match error.value_kind() {
                Some(ProductionViewValueKind::ParameterTarget) => "invalid_target",
                Some(ProductionViewValueKind::AutomationCurve) => "invalid_curve",
                Some(ProductionViewValueKind::Pitch) => "invalid_pitch",
                Some(ProductionViewValueKind::DrumVoice) => "invalid_drum_voice",
                Some(ProductionViewValueKind::MusicalTime) => "invalid_time",
                Some(ProductionViewValueKind::TimeGrid) => "invalid_grid",
                Some(ProductionViewValueKind::PhysicalValue) | None => "edit_rejected",
            };
            Failure::blocked(command, code, error.to_string(), json!([]))
        }
        ProductionViewError::Edit(error) => match error {
            error @ (EditError::StaleRevision { .. }
            | EditError::CandidateNotActive { .. }
            | EditError::StaleTarget { .. }
            | EditError::AssetDestinationExists(_)) => {
                Failure::conflict("edit", "edit_conflict", error)
            }
            EditError::InvalidResult(diagnostics) => Failure::blocked(
                "edit",
                "invalid_result",
                "Structured Source Edit produced an invalid Candidate Revision",
                json!(diagnostics),
            ),
            error @ (EditError::Project(_) | EditError::History(_) | EditError::Io { .. }) => {
                Failure::operational("edit", "edit_transaction_failed", error)
            }
            error => Failure::blocked(command, "edit_rejected", error.to_string(), json!([])),
        },
        ProductionViewError::History(error) => publication_failure(error),
        ProductionViewError::Project(error) => Failure::operational("edit", "project_io", error),
    }
}

fn sample_edit_failure(command: &'static str, error: EditError) -> Failure {
    Failure::blocked(command, "edit_rejected", error.to_string(), json!([]))
}

fn resolve_edit_time(
    bundle: &ProjectBundle,
    value: &str,
    timing: &TimeGridArgs,
    command: &'static str,
) -> Result<String, Failure> {
    let project = bundle.project.as_ref().ok_or_else(|| {
        Failure::blocked(
            command,
            "invalid_base",
            "Project model is unavailable",
            json!([]),
        )
    })?;
    let resolved = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == value)
        .map_or_else(|| value.to_owned(), |locator| locator.at.to_string());
    if timing.free || !timing.snap {
        return Ok(resolved);
    }
    let grid = timing.grid.as_deref().ok_or_else(|| {
        Failure::blocked(
            command,
            "invalid_grid",
            "--snap requires --grid TIME",
            json!([]),
        )
    })?;
    let value = parse_musical_time(&resolved, project.song.meter)
        .map_err(|error| Failure::blocked(command, "invalid_time", error.to_string(), json!([])))?;
    let grid = parse_musical_time(grid, project.song.meter)
        .map_err(|error| Failure::blocked(command, "invalid_grid", error.to_string(), json!([])))?;
    value
        .snap_to_grid(grid)
        .map(|snapped| snapped.to_string())
        .map_err(|error| Failure::blocked(command, "invalid_grid", error.to_string(), json!([])))
}

fn commit_cli_edit(
    bundle: &ProjectBundle,
    replacements: Vec<crate::edit::CheckedReplacement>,
    summary: &str,
) -> Result<Success, Failure> {
    let entry = apply_structured_edit(
        &bundle.root,
        &bundle.revision,
        replacements,
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
        | EditError::StaleTarget { .. }
        | EditError::AssetDestinationExists(_)) => {
            Failure::conflict("edit", "edit_conflict", error)
        }
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
        | EditError::UndeclaredAssetPath(_)
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

fn command_asset(cli: &Cli, command: &AssetCommand) -> Result<Success, Failure> {
    match command {
        AssetCommand::Import {
            source,
            id,
            name,
            summary,
        } => {
            let bundle = load(cli, "asset.import")?;
            if bundle.diagnostics.has_errors() {
                return Err(Failure::blocked(
                    "asset.import",
                    "invalid_base",
                    "media import requires a valid Active Candidate",
                    json!(bundle.diagnostics.as_slice()),
                ));
            }
            let entry = import_media_asset(
                &bundle.root,
                &bundle.revision,
                source,
                id,
                name.as_deref().unwrap_or(id),
                Publication::new(
                    ChangeOrigin::human("cli"),
                    summary,
                    "Copied external WAV into portable Project media",
                ),
            )
            .map_err(|error| {
                let mut failure = edit_transaction_failure(error);
                failure.command = "asset.import";
                failure
            })?;
            Ok(Success {
                command: "asset.import",
                human: format!("Imported media asset {id} as history entry {}", entry.id),
                data: json!({ "asset_id": id, "history_entry": entry }),
            })
        }
        AssetCommand::Collect { summary } => {
            let bundle = load(cli, "asset.collect")?;
            if bundle.diagnostics.has_errors() {
                return Err(Failure::blocked(
                    "asset.collect",
                    "invalid_base",
                    "media collection requires a valid Active Candidate",
                    json!(bundle.diagnostics.as_slice()),
                ));
            }
            let entry = collect_media_assets(
                &bundle.root,
                &bundle.revision,
                Publication::new(
                    ChangeOrigin::human("cli"),
                    summary,
                    "Collected exact WAV bytes into canonical portable paths",
                ),
            )
            .map_err(|error| {
                let mut failure = edit_transaction_failure(error);
                failure.command = "asset.collect";
                failure
            })?;
            Ok(Success {
                command: "asset.collect",
                human: format!("Collected portable media as history entry {}", entry.id),
                data: json!({ "history_entry": entry }),
            })
        }
        AssetCommand::List => {
            let bundle = load(cli, "asset.list")?;
            let assets = portable_asset_data(&bundle);
            Ok(Success {
                command: "asset.list",
                human: format!("{} portable media asset(s)", assets.len()),
                data: json!({
                    "revision": bundle.revision,
                    "assets": assets,
                    "diagnostics": bundle.diagnostics.as_slice(),
                }),
            })
        }
    }
}

fn portable_asset_data(bundle: &ProjectBundle) -> Vec<Value> {
    bundle
        .assets
        .iter()
        .map(|asset| {
            let metadata = asset.metadata();
            json!({
                "id": asset.id(),
                "name": asset.name(),
                "path": asset.path(),
                "sha256": asset.sha256(),
                "byte_len": asset.byte_len(),
                "channels": metadata.channels,
                "sample_rate": metadata.sample_rate,
                "frames": metadata.frames,
                "sample_format": format!("{:?}", metadata.sample_format).to_lowercase(),
                "bits_per_sample": metadata.bits_per_sample,
            })
        })
        .collect()
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

pub(super) fn serialize_response(response: &Value) -> String {
    match serde_json::to_string(response) {
        Ok(response) => response,
        Err(_) => "{\"schema_version\":1,\"ok\":false,\"command\":\"cli\",\"error\":{\"code\":\"serialization_failed\",\"message\":\"could not serialize CLI response\",\"diagnostics\":[]}}"
        .to_owned(),
    }
}

pub(super) fn emit_success(cli: &Cli, success: Success) {
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

pub(super) fn emit_failure(cli: &Cli, failure: &Failure) {
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

pub(super) fn blank_project_source(name: &str, id: &str) -> String {
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

pub(super) fn slug_id(name: &str) -> String {
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

fn resolved_output_file(path: &Path) -> Result<PathBuf, std::io::Error> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    let name = normalized.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "qualification output requires a file name",
        )
    })?;
    let mut cursor = normalized.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "qualification output has no parent",
        )
    })?;
    let mut missing = Vec::new();
    while !cursor.exists() {
        let component = cursor.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "qualification output has no existing ancestor",
            )
        })?;
        missing.push(component.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "qualification output has no existing ancestor",
            )
        })?;
    }
    let mut resolved = fs::canonicalize(cursor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    resolved.push(name);
    Ok(resolved)
}

fn inspect_qualification_output(
    candidate: &ProjectBundle,
    active: &ProjectBundle,
    output: &Path,
) -> Result<PathBuf, std::io::Error> {
    let destination = resolved_output_file(output)?;
    for bundle in [candidate, active] {
        let root = fs::canonicalize(&bundle.root)?;
        let history = root.join(".crump");
        let overlaps_source = bundle
            .sources
            .keys()
            .map(|path| root.join(path))
            .any(|path| path.starts_with(&destination));
        let overlaps_declared_media = bundle
            .project
            .iter()
            .flat_map(|project| &project.assets)
            .map(|asset| root.join(&asset.path))
            .any(|path| path.starts_with(&destination));
        let overlaps_loaded_media = bundle
            .assets
            .iter()
            .map(|asset| root.join(asset.path()))
            .any(|path| path.starts_with(&destination));
        if root.starts_with(&destination)
            || destination.starts_with(&history)
            || overlaps_source
            || overlaps_declared_media
            || overlaps_loaded_media
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "qualification report destination {} overlaps Project source, media, or history",
                    destination.display()
                ),
            ));
        }
    }
    Ok(destination)
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
