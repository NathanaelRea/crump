use super::*;

pub(super) fn render(frame: &mut Frame<'_>, app: &Workspace) {
    let area = frame.area();
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
    render_header(frame, app, regions[0]);
    render_tabs(frame, app, regions[1]);
    match app.view {
        View::Timeline => render_timeline(frame, app, regions[2]),
        View::Mixer => render_mixer(frame, app, regions[2]),
        View::Processing => render_processing(frame, app, regions[2]),
        View::EventGrid => render_event_grid(frame, app, regions[2]),
        View::Faceplate => render_faceplate(frame, app, regions[2]),
        View::Parameters => render_parameters(frame, app, regions[2]),
        View::Automation => render_automation(frame, app, regions[2]),
        View::Graph => render_graph(frame, app, regions[2]),
        View::Source => render_source(frame, app, regions[2]),
        View::Diagnostics => render_diagnostics(frame, app, regions[2]),
        View::History => render_history(frame, app, regions[2]),
        View::Help => render_help(frame, regions[2]),
    }
    render_footer(frame, app, regions[3]);
    if let Some(modal) = &app.modal {
        render_modal(frame, modal, area);
    }
}

fn render_header(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let project_name = app
        .bundle
        .project
        .as_ref()
        .map_or("Unparseable Project", |project| project.name.as_str());
    let candidate_state = if app.bundle.diagnostics.has_errors() {
        Span::styled(
            "INVALID CANDIDATE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if app.candidate_is_active() {
        Span::styled(
            "ACTIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "VALID CANDIDATE",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };
    let audio = if app.player.is_some() {
        let transport = format!(
            "{}{}",
            transport_label(app),
            if app.playback_paused { " PAUSED" } else { "" }
        );
        let audition = app
            .audition_status
            .as_ref()
            .map_or_else(String::new, |status| {
                format!(
                    " {} via {:?}{}",
                    audition_target_spec(&status.target),
                    status.path,
                    if status.looping { " LOOP" } else { "" }
                )
            });
        format!(
            " {transport}{audition}  {:?}{}  peak {:>5.2}  voices {:>2}  MIDI {}",
            app.telemetry.stream_state,
            if app.telemetry.transition_active {
                " XFADE/RESTART"
            } else {
                ""
            },
            app.telemetry.engine.current_peak,
            app.telemetry.engine.active_voices,
            if app.telemetry.midi_connected {
                "●"
            } else {
                "○"
            }
        )
    } else {
        format!(
            " stopped · armed {} via {:?}",
            audition_target_spec(&app.audition_plan.target),
            app.audition_plan.path
        )
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" CRUMP · {project_name}  "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        candidate_state,
        Span::raw(format!(
            "  C:{}  A:{}  │{}",
            short_revision(&app.bundle.revision),
            short_optional(app.active_revision.as_ref()),
            audio
        )),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_tabs(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let titles = VIEWS
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let key = match index {
                0..=8 => (index + 1).to_string(),
                9 => "0".to_owned(),
                10 => "H".to_owned(),
                _ => "?".to_owned(),
            };
            Line::from(format!("{key} {}", view.title()))
        })
        .collect::<Vec<_>>();
    let selected = VIEWS.iter().position(|view| *view == app.view).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::TOP | Borders::BOTTOM))
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" │ ");
    frame.render_widget(tabs, area);
}

fn render_timeline(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Timeline unavailable while Project source is unparseable",
        );
        return;
    };
    let context_height = if project.song.locators.is_empty() && project.song.grooves.is_empty() {
        0
    } else {
        3
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(context_height), Constraint::Min(5)])
        .split(area);
    if context_height > 0 {
        let locators = project
            .song
            .locators
            .iter()
            .map(|locator| format!("{}={} ({})", locator.id, locator.at, locator.name))
            .collect::<Vec<_>>()
            .join("  ");
        let grooves = project
            .song
            .grooves
            .iter()
            .map(|groove| groove.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        frame.render_widget(
            Paragraph::new(format!(
                "Locators: {}\nGrooves: {}",
                if locators.is_empty() {
                    "—"
                } else {
                    &locators
                },
                if grooves.is_empty() { "—" } else { &grooves }
            ))
            .block(
                Block::default()
                    .title(" Arrangement context ")
                    .borders(Borders::ALL),
            ),
            regions[0],
        );
    }
    let rows = project.song.tracks.iter().map(|track| {
        let clips = track
            .clips
            .iter()
            .map(|clip| {
                format!(
                    "{}:{}+{} {:?} off={} tr={:+}st gain={:+.1}db fade={}/{}{}",
                    clip.id,
                    clip.start,
                    clip.duration,
                    clip.mode,
                    clip.source_offset,
                    clip.transpose_semitones,
                    clip.gain_db,
                    clip.fade_in,
                    clip.fade_out,
                    clip.groove
                        .as_deref()
                        .map_or_else(String::new, |groove| format!(" groove={groove}"))
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        Row::new(vec![
            Cell::from(track.name.clone()),
            Cell::from(track.instrument.clone()),
            Cell::from(format!("{:.1} dB", track.channel.level_db)),
            Cell::from(format!("{:+.2}", track.channel.panner.primary_position())),
            Cell::from(clips),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(["Track", "Instrument", "Level", "Pan", "Procedural Clips"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Timeline · playhead {} · {} BPM · {} · end {} ",
                transport_label(app),
                project.song.tempo_bpm,
                project.song.meter,
                project.song.end
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_track));
    frame.render_stateful_widget(table, regions[1], &mut state);
}

fn render_mixer(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Mixer unavailable while Project source is unparseable",
        );
        return;
    };
    let parameters = project.mix_parameter_specs();
    let rows = parameters.iter().map(|(target, spec)| {
        let identity = mixer_channel_identity(project, target);
        let meter = identity
            .as_ref()
            .and_then(|identity| {
                app.mix_channel_identities
                    .iter()
                    .position(|candidate| candidate == identity)
            })
            .filter(|index| *index < app.telemetry.engine.channel_count)
            .map(|index| app.telemetry.engine.channel_activity[index]);
        let muted = identity
            .as_deref()
            .and_then(mixer_mute_target)
            .and_then(|target| project.parameter_spec(&target))
            .is_some_and(|mute| mute.base >= 0.5);
        let audition = identity.as_ref().map_or_else(
            || "—".to_owned(),
            |identity| {
                format!(
                    "{}{}{}",
                    if muted { 'M' } else { '·' },
                    if app.audition_plan.soloed_channels.contains(identity) {
                        'S'
                    } else {
                        '·'
                    },
                    if app
                        .audition_plan
                        .cue
                        .as_ref()
                        .is_some_and(|cue| cue.channel == *identity)
                    {
                        'C'
                    } else {
                        '·'
                    }
                )
            },
        );
        Row::new(vec![
            Cell::from(target.to_string()),
            Cell::from(spec.unit.format_value(spec.base)),
            Cell::from(format!(
                "{} … {}",
                spec.unit.format_value(spec.minimum),
                spec.unit.format_value(spec.maximum)
            )),
            Cell::from(
                meter
                    .map(|meter| format!("{:.3}", meter.current_peak))
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            Cell::from(
                meter
                    .map(|meter| format!("{:.3}", meter.average))
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            Cell::from(audition),
            Cell::from(format!(
                "{}/{}",
                if spec.automatable { "A" } else { "·" },
                if spec.modulatable { "M" } else { "·" }
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(31),
            Constraint::Length(12),
            Constraint::Percentage(24),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new([
            "Stable target",
            "Base",
            "Legal domain",
            "Peak",
            "Average",
            "M/S/C",
            "A/M",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Mixer · {} Tracks · {} Groups · {} Returns · {} Sidechains · m source Mute · s transient Solo · c/C post/pre cue · Enter exact · ←/→ step ",
                project.song.tracks.len(),
                project.song.groups.len(),
                project.song.returns.len(),
                project.song.sidechains.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_mixer_parameter));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_processing(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(graph) = app.processing_graph.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Processing inspection unavailable while the selected Project cannot compile",
        );
        return;
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(5)])
        .split(area);
    let status = app.audition_status.as_ref();
    let target = status
        .map(|status| format!("{:?}", status.target))
        .unwrap_or_else(|| format!("{:?}", app.audition_plan.target));
    let path = status
        .map(|status| format!("{:?}", status.path))
        .unwrap_or_else(|| format!("{:?}", app.audition_plan.path));
    let range = status.map_or_else(
        || "target default".to_owned(),
        |status| {
            format!(
                "{}..{} ({} frames){}",
                status.start_frame,
                status.end_frame,
                status.end_frame.saturating_sub(status.start_frame),
                if status.looping { " loop" } else { "" }
            )
        },
    );
    let revision = status
        .map(|status| status.active_revision.to_string())
        .or_else(|| app.active_revision.as_ref().map(ToString::to_string))
        .unwrap_or_else(|| "none".to_owned());
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("Target {target}  ·  Path {path}")),
            Line::from(format!(
                "Active Revision {revision}  ·  Range {range}  ·  Transport {}{}",
                transport_label(app),
                if app.playback_paused { " paused" } else { "" }
            )),
            Line::from(
                "A target · a path · F exact range · L loop · b transient bypass · B durable bypass · Enter setting · K/J reorder",
            ),
        ])
        .block(
            Block::default()
                .title(" Unified Audition ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true }),
        regions[0],
    );

    let rows = graph.processors.iter().map(|processor| {
        let meter = (processor.index < app.telemetry.engine.processor_count)
            .then(|| app.telemetry.engine.processor_activity[processor.index]);
        let transient = app
            .audition_plan
            .transient_bypass
            .get(&processor.id)
            .copied();
        let effective = meter.map_or_else(
            || transient.unwrap_or(processor.authored_bypass),
            |meter| meter.effective_bypass,
        );
        let bypass = format!(
            "D:{} T:{} E:{}{}",
            bool_mark(processor.authored_bypass),
            transient.map_or("—", bool_mark),
            bool_mark(effective),
            meter.map_or_else(String::new, |meter| format!(
                " {:.0}%",
                meter.bypass_mix * 100.0
            ))
        );
        let settings = processor
            .settings
            .iter()
            .map(|setting| {
                let value = setting.choice.clone().unwrap_or_else(|| {
                    setting
                        .unit
                        .zip(setting.value)
                        .map_or_else(|| "—".to_owned(), |(unit, value)| unit.format_value(value))
                });
                format!(
                    "{}={}{}",
                    setting.key,
                    value,
                    if setting.authored { "*" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        Row::new(vec![
            processor.index.to_string(),
            format!("{} / {}", processor.owner, processor.id),
            format!("{:?}", processor.kind).to_lowercase(),
            bypass,
            meter.map_or_else(
                || "—".to_owned(),
                |meter| format!("{:.3}", meter.input_peak),
            ),
            meter.map_or_else(
                || "—".to_owned(),
                |meter| format!("{:.3}", meter.output_peak),
            ),
            meter.map_or_else(
                || "—".to_owned(),
                |meter| {
                    format!(
                        "{:.1}/{:.1}",
                        meter.gain_reduction_db, meter.maximum_gain_reduction_db
                    )
                },
            ),
            processor.output_latency_frames.to_string(),
            settings,
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Percentage(22),
            Constraint::Length(11),
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new([
            "#",
            "Owner / durable ID",
            "Kind",
            "Bypass D/T/E",
            "Input",
            "Output",
            "GR now/max",
            "Latency",
            "Resolved settings (* authored)",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Processing chain · {} instances · state continues while bypassed · aligned dry path ",
                graph.processors.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default()
        .with_selected((!graph.processors.is_empty()).then_some(app.selected_processor));
    frame.render_stateful_widget(table, regions[1], &mut state);
}

fn bool_mark(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn render_event_grid(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Event Grid unavailable");
        return;
    };
    let Some(pattern) = project.song.patterns.get(app.selected_pattern) else {
        render_unavailable(frame, area, "No Pattern");
        return;
    };
    let mut procedure_lines = Vec::new();
    if let Some(variation) = &pattern.variation {
        procedure_lines.push(Line::from(format!(
            "Variation from {} · rotate {} · transpose {:+}st · velocity ×{} · reverse {} · probability {} · seed {}",
            variation.source,
            variation.rotate,
            variation.transpose_semitones,
            variation.velocity_scale,
            variation.reverse,
            variation.probability,
            variation.seed
        )));
    }
    procedure_lines.extend(pattern.locks.iter().map(|lock| {
        Line::from(format!(
            "Lock {} · {} · {} = {}",
            lock.id, lock.at, lock.parameter, lock.value
        ))
    }));
    let context_height = if procedure_lines.is_empty() {
        0
    } else {
        u16::try_from(procedure_lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height.saturating_sub(5))
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(context_height)])
        .split(area);
    let rows = pattern.events.iter().enumerate().map(|(index, event)| {
        let row = match event {
            PatternEvent::Note {
                at,
                pitch,
                duration,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "note".to_owned(),
                crate::model::format_pitch(*pitch),
                duration.to_string(),
                format!("{velocity:.2}"),
            ]),
            PatternEvent::Hit {
                at,
                voice,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "hit".to_owned(),
                format!("{voice:?}").to_lowercase(),
                "—".to_owned(),
                format!("{velocity:.2}"),
            ]),
            PatternEvent::Sample {
                id,
                at,
                mapping,
                duration,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "sample".to_owned(),
                format!("{id}:{mapping}"),
                duration.to_string(),
                format!("{velocity:.2}"),
            ]),
        };
        if event_is_active(app, &pattern.id, event) {
            row.style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(["#", "At", "Kind", "Pitch / Voice", "Duration", "Velocity"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Pattern {} · playhead {} · span {} · [/ ] Pattern · ←/→ note/voice · Enter pitch/voice · t time · u duration · v velocity ",
                pattern.id,
                transport_label(app),
                pattern.span
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_event));
    frame.render_stateful_widget(table, regions[0], &mut state);
    if context_height > 0 {
        frame.render_widget(
            Paragraph::new(procedure_lines).block(
                Block::default()
                    .title(" Procedural source & Parameter locks ")
                    .borders(Borders::ALL),
            ),
            regions[1],
        );
    }
}

fn render_faceplate(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Instrument Faceplate unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{}  ·  {}  ·  polyphony {}  ·  ←/→ adjust  ·  Enter exact value",
            instrument.name, instrument.kind, instrument.polyphony
        ))
        .block(
            Block::default()
                .title(" Instrument Faceplate ")
                .borders(Borders::ALL),
        ),
        vertical[0],
    );
    let constraints = if instrument.faceplate.is_empty() {
        vec![Constraint::Min(1)]
    } else {
        vec![Constraint::Ratio(1, instrument.faceplate.len() as u32); instrument.faceplate.len()]
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(vertical[1]);
    for (index, control) in instrument.faceplate.iter().enumerate() {
        let parameter = instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == control.parameter);
        let selected = index == app.selected_parameter;
        let value = parameter.map_or_else(
            || "missing".to_owned(),
            |parameter| parameter.unit.format_value(parameter.value),
        );
        let range = parameter.map_or_else(
            || "".to_owned(),
            |parameter| {
                format!(
                    "{} … {}",
                    parameter.unit.format_value(parameter.minimum),
                    parameter.unit.format_value(parameter.maximum)
                )
            },
        );
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    value,
                    Style::default()
                        .fg(if selected {
                            Color::Black
                        } else {
                            Color::Yellow
                        })
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(range),
                Line::from(control.explanation.clone()),
            ]))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(format!(" {} ", control.label))
                    .borders(Borders::ALL)
                    .style(if selected {
                        selected_style()
                    } else {
                        Style::default()
                    }),
            ),
            columns[index],
        );
    }
    frame.render_widget(
        Paragraph::new("Note Audition: A W S E D F T G Y H U J K · transient only · PipeWire MIDI follows this Instrument")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP)),
        vertical[2],
    );
}

fn render_parameters(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Parameters unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let rows = instrument.parameters.iter().map(|parameter| {
        Row::new(vec![
            parameter.name.clone(),
            parameter.key.clone(),
            parameter.unit.to_string(),
            parameter.unit.format_value(parameter.value),
            format!(
                "{} … {}",
                parameter.unit.format_value(parameter.minimum),
                parameter.unit.format_value(parameter.maximum)
            ),
            parameter.unit.format_value(parameter.default),
            if parameter.automatable { "yes" } else { "no" }.to_owned(),
            if parameter.modulatable { "yes" } else { "no" }.to_owned(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(11),
            Constraint::Length(14),
            Constraint::Length(24),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new([
            "Parameter",
            "Key",
            "Unit",
            "Value",
            "Legal domain",
            "Default",
            "Automate",
            "Modulate",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(" Exact Parameters · {} ", instrument.name))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_parameter));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_automation(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Automation unavailable while Project source is unparseable",
        );
        return;
    };
    let Some(lane) = project.song.automations.get(app.selected_automation) else {
        render_unavailable(
            frame,
            area,
            "No Song Automation lanes · use the CLI or readable source to create one",
        );
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(4)])
        .split(area);
    let spec = project.parameter_spec(&lane.target);
    let frame_position = app.telemetry.engine.frame;
    let snapshot = crate::engine::control::CompiledControlProgram::compile(project)
        .ok()
        .and_then(|program| {
            program
                .snapshot_at(frame_position)
                .into_iter()
                .find(|value| value.target == lane.target)
        });
    let base = spec.map_or_else(
        || "unresolved".to_owned(),
        |spec| spec.unit.format_value(spec.base),
    );
    let provenance = snapshot.map_or_else(
        || "effective unavailable".to_owned(),
        |value| {
            let automation = value.automation.map_or_else(
                || "none".to_owned(),
                |automation| value.unit.format_value(automation),
            );
            let routes = value
                .modulation_routes
                .iter()
                .map(|route| route.route.as_str())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "base {base}  →  automation {automation}  +  modulation {} [{}]  →  effective {}{}",
                value.unit.format_value(value.modulation),
                if routes.is_empty() { "none" } else { &routes },
                value.unit.format_value(value.effective),
                if value.clamped { "  CLAMPED" } else { "" }
            )
        },
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "Lane {}  target {}  points {}  source {}:{}",
                lane.id,
                lane.target,
                lane.points.len(),
                lane.location.file.display(),
                lane.location.line
            )),
            Line::from(format!("frame {frame_position}  {provenance}")),
        ])
        .block(
            Block::default()
                .title(" Effective Parameter Provenance ")
                .borders(Borders::ALL),
        ),
        sections[0],
    );
    let rows = lane.points.iter().map(|point| {
        Row::new(vec![
            Cell::from(point.id.clone()),
            Cell::from(point.at.to_string()),
            Cell::from(point.value.clone()),
            Cell::from(point.interpolation.to_string()),
            Cell::from(format!(
                "{}:{}",
                point.location.file.display(),
                point.location.line
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Min(18),
        ],
    )
    .header(Row::new(["Point", "Song time", "Physical value", "Curve out", "Source"]))
    .block(
        Block::default()
            .title(format!(
                " Automation Points · lane {}/{} · [ ] lane  ↑↓ point  ←→ value  Enter exact  t time  c curve ",
                app.selected_automation + 1,
                project.song.automations.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default();
    if !lane.points.is_empty() {
        state.select(Some(app.selected_automation_point));
    }
    frame.render_stateful_widget(table, sections[1], &mut state);
}

fn render_graph(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Graph unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let body = if let Some(graph) = &instrument.graph {
        let processing = app.player.is_some()
            && app.candidate_is_active()
            && app.telemetry.engine.active_voices > 0;
        let selected = app.selected_graph_node;
        let mut lines = vec![Line::from(Span::styled(
            format!(
                "NODES  {} / {} declared ceiling  ·  {}",
                graph.nodes.len(),
                graph.max_nodes,
                if processing { "processing" } else { "idle" }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.extend(graph.nodes.iter().enumerate().map(|(index, node)| {
            let settings = node
                .settings
                .iter()
                .map(|(key, value)| format!("{key}={value:?}"))
                .collect::<Vec<_>>()
                .join("  ");
            let line = Line::from(vec![
                Span::styled(
                    if processing { " ● " } else { " ○ " },
                    Style::default().fg(if processing {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    format!("{:<16}", node.id),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:<12}", format!("{:?}", node.kind).to_lowercase()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(format!(
                    " {settings}  · {}:{}",
                    node.location.file.display(),
                    node.location.line
                )),
            ]);
            if index == selected {
                line.style(selected_style())
            } else {
                line
            }
        }));

        let connection_offset = graph.nodes.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "BUSES",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            graph
                .connections
                .iter()
                .enumerate()
                .map(|(index, connection)| {
                    let line = Line::from(format!(
                        "  {}.{}  ──▶  {}.{}  · {}:{}",
                        connection.from.node,
                        connection.from.port,
                        connection.to.node,
                        connection.to.port,
                        connection.location.file.display(),
                        connection.location.line
                    ));
                    if connection_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );

        let parameter_offset = connection_offset + graph.connections.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "PARAMETERS",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            instrument
                .parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    let line = Line::from(format!(
                        "  {:<16} {} = {}  [{} … {}]  · {}:{}",
                        parameter.key,
                        parameter.unit,
                        parameter.unit.format_value(parameter.value),
                        parameter.unit.format_value(parameter.minimum),
                        parameter.unit.format_value(parameter.maximum),
                        parameter.location.file.display(),
                        parameter.location.line
                    ));
                    if parameter_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );

        let faceplate_offset = parameter_offset + instrument.parameters.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "FACEPLATE MAPPINGS",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            instrument
                .faceplate
                .iter()
                .enumerate()
                .map(|(index, control)| {
                    let line = Line::from(format!(
                        "  {:<16} {}  ──▶  ${}  · {}:{}",
                        control.key,
                        control.label,
                        control.parameter,
                        control.location.file.display(),
                        control.location.line
                    ));
                    if faceplate_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );
        Text::from(lines)
    } else {
        Text::from(vec![
            Line::from("Stock synthesized percussion"),
            Line::from("kick · snare · hat · clap"),
            Line::from("No sample assets and no editable topology"),
        ])
    };
    let selected_row = instrument.graph.as_ref().map_or(0, |graph| {
        let selected = app.selected_graph_node;
        if selected < graph.nodes.len() {
            1 + selected
        } else if selected < graph.nodes.len() + graph.connections.len() {
            3 + selected
        } else if selected
            < graph.nodes.len() + graph.connections.len() + instrument.parameters.len()
        {
            5 + selected
        } else {
            7 + selected
        }
    });
    let scroll = selected_row
        .saturating_sub(usize::from(area.height.saturating_sub(4)) / 2)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(format!(
                        " Inspectable Graph · {} · ↑/↓ Item · ←/→ Instrument · Enter Source ",
                        instrument.name
                    ))
                    .borders(Borders::ALL),
            ),
        area,
    );
}

fn render_source(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some((path, source)) = app.bundle.sources.iter().nth(app.selected_source) else {
        render_unavailable(frame, area, "No source file");
        return;
    };
    let height = area.height.saturating_sub(2) as usize;
    let lines = source
        .lines()
        .enumerate()
        .skip(app.source_scroll)
        .take(height)
        .map(|(index, line)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>5} │ ", index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(line.to_owned()),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(format!(
                    " Authoritative Source · {} · ←/→ file · scroll {} ",
                    path.display(),
                    app.source_scroll + 1
                ))
                .borders(Borders::ALL),
        ),
        area,
    );
    let line_count = source.lines().count();
    let mut scrollbar =
        ScrollbarState::new(line_count.saturating_sub(height)).position(app.source_scroll);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar,
    );
}

fn render_diagnostics(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let diagnostics = app.bundle.diagnostics.as_slice();
    let items = if diagnostics.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "✓ Candidate has no diagnostics and may publish",
            Style::default().fg(Color::Green),
        )))]
    } else {
        diagnostics
            .iter()
            .map(|diagnostic| {
                let location = diagnostic.location.as_ref().map_or_else(
                    || "".to_owned(),
                    |location| {
                        format!(
                            "{}:{}:{}  ",
                            location.file.display(),
                            location.line,
                            location.column
                        )
                    },
                );
                let color = if diagnostic.severity == Severity::Error {
                    Color::Red
                } else {
                    Color::Yellow
                };
                let mut lines = vec![Line::from(vec![
                    Span::styled(
                        format!("{:?} {}  ", diagnostic.severity, diagnostic.code),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(location),
                    Span::raw(diagnostic.message.clone()),
                ])];
                if let Some(help) = &diagnostic.help {
                    lines.push(Line::from(vec![
                        Span::styled("  help: ", Style::default().fg(Color::Cyan)),
                        Span::raw(help.clone()),
                    ]));
                }
                ListItem::new(lines)
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(format!(
                    " Candidate Diagnostics · {} finding(s) ",
                    diagnostics.len()
                ))
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_history(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let rows = app.history.iter().map(|entry| {
        let active = if app.active_entry.as_ref() == Some(&entry.id) {
            "●"
        } else {
            " "
        };
        Row::new(vec![
            active.to_owned(),
            entry.id.clone(),
            format!("{:?}", entry.origin.kind).to_lowercase(),
            entry.origin.name.clone(),
            entry.summary.clone(),
            entry.changes.len().to_string(),
            entry.created_at.clone(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Min(24),
            Constraint::Length(8),
            Constraint::Length(24),
        ],
    )
    .header(
        Row::new([
            "",
            "Entry",
            "Origin",
            "Actor",
            "Summary",
            "Files",
            "Created UTC",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(" Append-only Project History · Enter/d diff · R restore ")
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_history));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let help = vec![
        Line::from(Span::styled(
            "WORKSPACE",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("1–9/0 views · H History · ? Help · Tab / Shift-Tab next/previous · ↑/↓ select"),
        Line::from(
            "Space start/pause/resume exact Active Revision · S stop · P inspect/publish Candidate",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "PRODUCTION VIEWS",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "Timeline: ←/→ level ±1 dB · Enter/l exact level · p exact pan · A Track audition",
        ),
        Line::from(
            "Processing: A target · a path · F exact range · L loop · b transient/B durable bypass",
        ),
        Line::from(
            "Processing: Enter exact setting · K/J move processor earlier/later · meters show D/T/E",
        ),
        Line::from(
            "Event Grid: A Pattern/sample audition · t onset · u duration · v velocity · L loop",
        ),
        Line::from("Sample occurrence: x region · f fade · g gain · p pan"),
        Line::from("Faceplate / Parameters: ←/→ bounded step · Enter exact physical value"),
        Line::from("Automation: [/] lane · ↑/↓ point · ←/→ value · Enter exact · t time · c curve"),
        Line::from(
            "Graph: ↑/↓ Node/Bus/Parameter/Mapping · ←/→ Instrument · Enter Source location",
        ),
        Line::from("Source: ←/→ file · ↑/↓ scroll · e open $VISUAL/$EDITOR"),
        Line::from("History: Enter/d exact source diff · R exact restore as a new entry"),
        Line::from(""),
        Line::from(Span::styled(
            "NOTE AUDITION",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("In Instrument views: A W S E D F T G Y H U J K plays C4–C5 transiently."),
        Line::from(
            "PipeWire MIDI Note On/Off targets the selected Instrument. Disconnect releases all notes.",
        ),
        Line::from(
            "Transient bypass and audition selection never edit source; B/settings/reorder are historical edits.",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "SAFETY",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "Invalid Candidate source is never made audible. The previous Active Revision continues.",
        ),
        Line::from(
            "Every Production View edit is a checked minimal source replacement and atomic publication.",
        ),
        Line::from(
            "External Agents use readable .crump files plus `crump --json` commands; no model is embedded.",
        ),
    ];
    frame.render_widget(
        Paragraph::new(help).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Keyboard-complete Help ")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let ratio = app.telemetry.engine.current_peak.clamp(0.0, 1.0) as f64;
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(24)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(Color::Cyan)),
            Span::styled(
                format!(" {} ", app.status),
                Style::default().fg(Color::White),
            ),
        ]))
        .block(Block::default().borders(Borders::TOP)),
        horizontal[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .title(" MONITOR PEAK ")
                    .borders(Borders::TOP),
            )
            .gauge_style(Style::default().fg(if ratio > 0.95 {
                Color::Red
            } else {
                Color::Green
            }))
            .ratio(ratio)
            .label(format!("{:.3}", app.telemetry.engine.current_peak)),
        horizontal[1],
    );
}

fn render_modal(frame: &mut Frame<'_>, modal: &Modal, area: Rect) {
    let popup = centered_rect(76, 50, area);
    frame.render_widget(Clear, popup);
    match modal {
        Modal::Input {
            title,
            prompt,
            value,
            ..
        } => {
            let body = Text::from(vec![
                Line::from(prompt.clone()),
                Line::from(""),
                Line::from(Span::styled(
                    value.clone(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Enter apply · Esc cancel · Backspace edit"),
            ]);
            frame.render_widget(
                Paragraph::new(body).wrap(Wrap { trim: false }).block(
                    Block::default()
                        .title(format!(" {title} "))
                        .borders(Borders::ALL)
                        .style(Style::default().bg(Color::Black)),
                ),
                popup,
            );
        }
        Modal::ConfirmRestore { entry_id } => {
            frame.render_widget(
                Paragraph::new(format!(
                    "Restore exact state from {entry_id}?\n\nCurrent source will be replaced atomically and the restored state appended as a new History Entry.\n\ny / Enter restore · n / Esc cancel"
                ))
                .wrap(Wrap { trim: true })
                .block(Block::default().title(" Confirm History Restore ").borders(Borders::ALL).style(Style::default().bg(Color::Black))),
                popup,
            );
        }
        Modal::Text {
            title,
            body,
            scroll,
        } => {
            frame.render_widget(
                Paragraph::new(body.as_str())
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .title(format!(" {title} · Esc close "))
                            .borders(Borders::ALL)
                            .style(Style::default().bg(Color::Black)),
                    ),
                popup,
            );
        }
    }
}

fn render_unavailable(frame: &mut Frame<'_>, area: Rect, message: &str) {
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn selected_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
