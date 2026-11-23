use super::timeline::{TimelineCell, TimelineGridMark, TimelineWindow};
use super::*;
use ratatui::widgets::{HighlightSpacing, ListState};

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
        View::Instrument => render_instrument_sections(frame, app, regions[2]),
        View::Parameters => render_parameters(frame, app, regions[2]),
        View::Automation => render_automation(frame, app, regions[2]),
        View::Graph => render_graph(frame, app, regions[2]),
        View::Source => render_source(frame, app, regions[2]),
        View::Diagnostics => render_diagnostics(frame, app, regions[2]),
        View::History => render_history(frame, app, regions[2]),
        View::Help => render_help(frame, app, regions[2]),
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
        .map_or("Unparsable Project", |project| project.name.as_str());
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
    let input_mode = if app.note_mode {
        "  NOTE MODE"
    } else if app.timeline_state.visual() && app.view == View::Timeline {
        "  RANGE MODE"
    } else {
        ""
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
            "  C:{}  A:{}{input_mode}  │{}",
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
    let detail_height = if area.height >= 14 {
        5
    } else if area.height >= 10 {
        3
    } else {
        0
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(detail_height)])
        .split(area);
    let table_area = regions[0];
    // Border (2), permanent selection symbol (2), and Table column spacing (1).
    let available = table_area.width.saturating_sub(5);
    let label_width = match available {
        90.. => 28,
        62.. => 22,
        38.. => 16,
        _ => available.saturating_div(2).max(6),
    }
    .min(available);
    let grid_width = available.saturating_sub(label_width);
    let window = app.timeline_viewport.window(project.song.end);
    let projection = app
        .timeline_cache
        .project(project, window, usize::from(grid_width));
    let columns = usize::from(grid_width);
    let playhead = timeline_playhead_column(app, projection.window, columns);
    let cursor = timeline_cursor_column(app, projection.window, columns);
    let visual_range = timeline_visual_columns(app, projection.window, columns);
    let ruler = timeline_ruler_line(&projection, playhead, cursor, app.timeline_ascii);
    let rows = project.song.tracks.iter().map(|track| {
        let lane = projection
            .lanes
            .iter()
            .find(|lane| lane.track_id == track.id);
        let arrangement = lane.map_or_else(Line::default, |lane| {
            Line::from(
                lane.cells
                    .iter()
                    .enumerate()
                    .map(|(column, cell)| {
                        timeline_cell_span(
                            *cell,
                            projection.grid[column],
                            projection.locator_columns.contains(&column),
                            visual_range
                                .is_some_and(|(start, end)| start <= column && column < end),
                            cursor == Some(column),
                            playhead == Some(column),
                            app.timeline_ascii,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let summary = format!(
            "{} · {:+.1}dB · {:+.2}",
            track.name,
            track.channel.level_db,
            track.channel.panner.primary_position()
        );
        Row::new(vec![Cell::from(summary), Cell::from(arrangement)])
    });
    let mode = if app.timeline_viewport.is_fit() {
        "FIT".to_owned()
    } else {
        format!("{}×", app.timeline_viewport.zoom_factor())
    };
    let interaction = if app.timeline_state.visual() {
        "RANGE"
    } else {
        "NORMAL"
    };
    let cursor_label = timeline_time_label(app.timeline_position().at, project.song.meter);
    let follow = if app.timeline_state.follow_playhead() {
        " · FOLLOW"
    } else {
        ""
    };
    let warning = app
        .timeline_cache
        .warning()
        .map_or_else(String::new, |warning| format!(" · {warning}"));
    let table = Table::new(rows, [Constraint::Length(label_width), Constraint::Min(1)])
        .header(
            Row::new(vec![
                Cell::from(format!("Track · {mode}")),
                Cell::from(ruler),
            ])
            .style(Style::default().fg(Color::Cyan)),
        )
        .block(
            Block::default()
                .title(format!(
                    " Timeline · {interaction} · cursor {cursor_label} · grid {}{follow} · {}–{} · playhead {} · {} BPM · {}{} ",
                    app.timeline_state.grid_label(),
                    timeline_time_label(projection.window.start, project.song.meter),
                    timeline_time_label(projection.window.end, project.song.meter),
                    transport_label(app),
                    project.song.tempo_bpm,
                    project.song.meter,
                    warning
                ))
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(if app.timeline_ascii { "> " } else { "▶ " })
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = TableState::default().with_selected(Some(app.selected_track));
    frame.render_stateful_widget(table, table_area, &mut state);

    if detail_height > 0 {
        render_timeline_details(frame, app, regions[1]);
    }
}

fn timeline_cell_span(
    cell: TimelineCell,
    grid: TimelineGridMark,
    locator: bool,
    in_visual_range: bool,
    cursor: bool,
    playhead: bool,
    ascii: bool,
) -> Span<'static> {
    const UNICODE_LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    const ASCII_LEVELS: [char; 8] = ['.', ':', '-', '=', '+', '*', '#', '@'];
    let levels = if ascii {
        &ASCII_LEVELS
    } else {
        &UNICODE_LEVELS
    };
    let clip_edge = cell.clip_start || cell.clip_end;
    let clip_background = if clip_edge && cell.clip_depth > 1 {
        Some(Color::Rgb(76, 30, 76))
    } else if clip_edge {
        Some(Color::Rgb(18, 48, 72))
    } else if cell.clip_depth > 1 {
        Some(Color::Rgb(54, 24, 54))
    } else if cell.clip_depth == 1 {
        Some(Color::Rgb(12, 30, 48))
    } else {
        None
    };
    let edge_color = if cell.clip_depth > 1 {
        Color::Magenta
    } else {
        Color::Blue
    };
    // Clip edges win over activity so every authored half-open range remains
    // legible even when an onset lands at exactly the same position.
    let (mut glyph, foreground) = if cell.clip_start && cell.clip_end {
        (if ascii { '#' } else { '■' }, edge_color)
    } else if cell.clip_start {
        (if ascii { '[' } else { '▌' }, edge_color)
    } else if cell.clip_end {
        (if ascii { ']' } else { '▐' }, edge_color)
    } else if cell.sample_active {
        let level = ((cell.sample_peak.clamp(0.0, 1.0) * 7.0).ceil() as usize).min(7);
        (levels[level], Color::Yellow)
    } else if cell.activity > 0.0 {
        let normalized = (cell.activity / (1.0 + cell.activity)).sqrt();
        let level = ((normalized * 7.0).ceil() as usize).min(7);
        (levels[level], Color::Cyan)
    } else if cell.sustained > 0.0 {
        (if ascii { '=' } else { '━' }, Color::Cyan)
    } else if cell.clip_depth > 1 {
        (if ascii { '#' } else { '▓' }, Color::Magenta)
    } else if cell.clip_depth == 1 {
        (if ascii { '-' } else { '─' }, Color::Blue)
    } else if locator {
        (if ascii { '^' } else { '◆' }, Color::Yellow)
    } else {
        match grid {
            TimelineGridMark::Bar => (if ascii { '|' } else { '│' }, Color::DarkGray),
            TimelineGridMark::Beat => (if ascii { ':' } else { '┊' }, Color::DarkGray),
            TimelineGridMark::None => (' ', Color::Reset),
        }
    };
    let mut style = Style::default().fg(foreground);
    if let Some(background) = clip_background {
        style = style.bg(background);
    }
    if in_visual_range {
        style = style.bg(Color::Rgb(20, 48, 42));
    }
    if cursor {
        style = style.bg(Color::Cyan).add_modifier(Modifier::BOLD);
    }
    if playhead {
        glyph = if ascii { '|' } else { '│' };
        style = style
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD);
    }
    Span::styled(glyph.to_string(), style)
}

fn timeline_ruler_line(
    projection: &timeline::TimelineProjection,
    playhead: Option<usize>,
    cursor: Option<usize>,
    ascii: bool,
) -> Line<'static> {
    let mut characters = projection.ruler.chars().collect::<Vec<_>>();
    for (column, character) in characters.iter_mut().enumerate() {
        if *character == ' ' {
            *character = match projection.grid[column] {
                TimelineGridMark::Bar => {
                    if ascii {
                        '|'
                    } else {
                        '│'
                    }
                }
                TimelineGridMark::Beat => {
                    if ascii {
                        ':'
                    } else {
                        '┊'
                    }
                }
                TimelineGridMark::None => ' ',
            };
        }
    }
    for column in &projection.locator_columns {
        if let Some(character) = characters.get_mut(*column) {
            *character = if ascii { '^' } else { '◆' };
        }
    }
    if let Some(column) = cursor
        && let Some(character) = characters.get_mut(column)
    {
        *character = if ascii { 'v' } else { '▽' };
    }
    if let Some(column) = playhead
        && let Some(character) = characters.get_mut(column)
    {
        *character = if ascii { 'v' } else { '▼' };
    }
    Line::from(
        characters
            .into_iter()
            .enumerate()
            .map(|(column, character)| {
                let style = if playhead == Some(column) {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else if cursor == Some(column) {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if projection.locator_columns.contains(&column) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                Span::styled(character.to_string(), style)
            })
            .collect::<Vec<_>>(),
    )
}

fn timeline_cursor_column(
    app: &Workspace,
    window: TimelineWindow,
    columns: usize,
) -> Option<usize> {
    if columns == 0 {
        return None;
    }
    let at = app.timeline_position().at;
    timeline::time_column(at, window, columns)
        .or_else(|| (at == window.end && at >= window.start).then_some(columns.saturating_sub(1)))
}

fn timeline_visual_columns(
    app: &Workspace,
    window: TimelineWindow,
    columns: usize,
) -> Option<(usize, usize)> {
    let range = app.timeline_state.visual_range()?;
    timeline::time_span_columns(range.start, range.end, window, columns)
}

fn timeline_playhead_column(
    app: &Workspace,
    window: TimelineWindow,
    columns: usize,
) -> Option<usize> {
    if columns == 0 || !app.candidate_is_audible() {
        return None;
    }
    let project = app.bundle.publishable()?;
    let start = window
        .start
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .ok()?;
    let end = window
        .end
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .ok()?;
    timeline::frame_column(app.telemetry.engine.frame, start, end, columns)
}

fn render_timeline_details(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        return;
    };
    let Some(track) = project.song.tracks.get(app.selected_track) else {
        return;
    };
    let clip_details = track
        .clips
        .iter()
        .map(|clip| {
            format!(
                "{} {}+{} {:?} off={} tr={:+}st gain={:+.1}db fade={}/{}{}",
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
    let locators = project
        .song
        .locators
        .iter()
        .map(|locator| format!("{}={} ({})", locator.id, locator.at, locator.name))
        .collect::<Vec<_>>()
        .join(", ");
    let grooves = project
        .song
        .grooves
        .iter()
        .map(|groove| groove.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let context = format!(
        "Locators: {} · Grooves: {}",
        if locators.is_empty() {
            "—"
        } else {
            &locators
        },
        if grooves.is_empty() { "—" } else { &grooves }
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "{} · Instrument {} · {:.1} dB · pan {:+.2} · {} Clip{}",
                track.name,
                track.instrument,
                track.channel.level_db,
                track.channel.panner.primary_position(),
                track.clips.len(),
                if track.clips.len() == 1 { "" } else { "s" }
            )),
            Line::from(if clip_details.is_empty() {
                "No arranged Clips".to_owned()
            } else {
                clip_details
            }),
            Line::from(context),
        ])
        .block(
            Block::default()
                .title(" Selected Track · green cursor/range · blue Clip · cyan Event · yellow sample waveform · red playhead ")
                .borders(Borders::ALL),
        ),
        area,
    );
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
    let selected = app
        .selected_mixer_parameter
        .min(app.mixer_parameters.len().saturating_sub(1));
    let row_capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let window = selection_window(app.mixer_parameters.len(), selected, row_capacity);
    let rows = app.mixer_parameters[window.clone()]
        .iter()
        .map(|parameter| {
            let target = &parameter.target;
            let spec = &parameter.spec;
            let identity = parameter.channel_identity.as_ref();
            let meter = identity
                .and_then(|identity| app.mix_channel_indexes.get(identity).copied())
                .filter(|index| *index < app.telemetry.engine.channel_count)
                .map(|index| app.telemetry.engine.channel_activity[index]);
            let audition = identity.map_or_else(
                || "—".to_owned(),
                |identity| {
                    format!(
                        "{}{}{}",
                        if parameter.muted { 'M' } else { '·' },
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
    let mut state = TableState::default()
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_processing(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(graph) = app.processing_graph() else {
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

    let selected = app
        .selected_processor
        .min(graph.processors.len().saturating_sub(1));
    let row_capacity = usize::from(regions[1].height.saturating_sub(3)).max(1);
    let window = selection_window(graph.processors.len(), selected, row_capacity);
    let rows = graph.processors[window.clone()].iter().map(|processor| {
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
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
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
    let selected = app
        .selected_event
        .min(pattern.events.len().saturating_sub(1));
    let row_capacity = usize::from(regions[0].height.saturating_sub(3)).max(1);
    let window = selection_window(pattern.events.len(), selected, row_capacity);
    let rows = pattern.events[window.clone()]
        .iter()
        .enumerate()
        .map(|(offset, event)| {
            let index = window.start + offset;
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
    let mut state = TableState::default()
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
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

fn render_instrument_sections(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Instrument sections unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let sections = app.instrument_sections();
    let Some(section) = sections.get(app.selected_instrument_section) else {
        render_unavailable(frame, area, "No generated Instrument sections");
        return;
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(4),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{} · {} · {} sections · ←/→ section · ↑/↓ item · Enter exact edit/source",
            instrument.name,
            instrument.kind,
            sections.len()
        ))
        .block(
            Block::default()
                .title(" Generated Instrument Sections ")
                .borders(Borders::ALL),
        ),
        regions[0],
    );
    frame.render_widget(
        Tabs::new(
            sections
                .iter()
                .map(|section| Line::from(section.name.clone()))
                .collect::<Vec<_>>(),
        )
        .select(app.selected_instrument_section)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" │ ")
        .block(Block::default().borders(Borders::BOTTOM)),
        regions[1],
    );
    let (control_program, frame_position, live_control) = app.control_inspection();
    let route_activity = control_program
        .map(|inspector| {
            inspector
                .borrow_mut()
                .snapshot_instrument_routes_at(frame_position, &instrument.id)
                .into_iter()
                .map(|(unit, route)| {
                    (
                        route.route.clone(),
                        instrument_modulation_route_label(&route, unit),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let rows = section.rows.iter().map(|row| {
        let detail = if row.kind == Some(PresentationItemKind::Route) {
            route_activity.get(&row.id).map_or_else(
                || row.detail.clone(),
                |activity| {
                    format!(
                        "{} · {} {activity}",
                        row.detail,
                        if live_control {
                            "live"
                        } else {
                            "Candidate preview"
                        }
                    )
                },
            )
        } else {
            row.detail.clone()
        };
        Row::new(vec![
            row.kind
                .map_or_else(|| "policy".to_owned(), |kind| format!("{kind:?}")),
            row.id.clone(),
            detail,
            row.source.clone(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(22),
            Constraint::Min(32),
            Constraint::Length(28),
        ],
    )
    .header(
        Row::new([
            "Type",
            "Identity",
            "Authoritative contract / control activity",
            "Exact source",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(" {} · {} ", section.name, section.id))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_instrument_row));
    frame.render_stateful_widget(table, regions[2], &mut state);
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
    let selected = app
        .selected_parameter
        .min(instrument.parameters.len().saturating_sub(1));
    let row_capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let window = selection_window(instrument.parameters.len(), selected, row_capacity);
    let rows = instrument.parameters[window.clone()]
        .iter()
        .map(|parameter| {
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
    let mut state = TableState::default()
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
    frame.render_stateful_widget(table, area, &mut state);
}

fn instrument_modulation_route_label(
    route: &crate::engine::control::InstrumentModulationValueSnapshot,
    unit: crate::model::Unit,
) -> String {
    let (scope, contribution) = match route.scope {
        crate::model::InstrumentModulationScope::Global => (
            "global",
            route.contribution.map_or_else(
                || "unavailable".to_owned(),
                |contribution| unit.format_value(contribution),
            ),
        ),
        crate::model::InstrumentModulationScope::Instrument => {
            ("instrument/per-track", "per-track".to_owned())
        }
        crate::model::InstrumentModulationScope::PerVoice => ("per_voice", "per-voice".to_owned()),
    };
    format!(
        "instrument:{}[{scope}]{}={contribution}",
        route.route,
        if route.bypass { "(bypass)" } else { "" },
    )
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
    let (control_program, frame_position, live_control) = app.control_inspection();
    let snapshot = control_program.and_then(|inspector| {
        inspector
            .borrow_mut()
            .snapshot_target_at(frame_position, &lane.target)
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
                .instrument_modulation_routes
                .iter()
                .map(|route| instrument_modulation_route_label(route, value.unit))
                .chain(
                    value
                        .modulation_routes
                        .iter()
                        .map(|route| format!("song:{}", route.route)),
                )
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
            Line::from(format!(
                "{} frame {frame_position}  {provenance}",
                if live_control {
                    "live"
                } else {
                    "Candidate preview"
                }
            )),
        ])
        .block(
            Block::default()
                .title(" Effective Parameter Provenance ")
                .borders(Borders::ALL),
        ),
        sections[0],
    );
    let selected = app
        .selected_automation_point
        .min(lane.points.len().saturating_sub(1));
    let row_capacity = usize::from(sections[1].height.saturating_sub(3)).max(1);
    let window = selection_window(lane.points.len(), selected, row_capacity);
    let rows = lane.points[window.clone()].iter().map(|point| {
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
    let mut state = TableState::default()
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
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
        let processing = app.candidate_is_audible() && app.telemetry.engine.active_voices > 0;
        let selected = app.selected_graph_node;
        let node_order = graph_processing_node_indexes(instrument, graph);
        let bus_order = graph_processing_bus_indexes(graph, &node_order);
        let mut lines = vec![Line::from(Span::styled(
            format!(
                "VOICE NODES  {} / {} declared ceiling  ·  {}",
                graph.nodes.len(),
                graph.max_nodes,
                if processing { "processing" } else { "idle" }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.extend(node_order.iter().enumerate().map(|(index, node_index)| {
            let node = &graph.nodes[*node_index];
            let settings = node
                .settings
                .iter()
                .map(|setting| format!("{}={:?}", setting.key, setting.value))
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
            bus_order
                .iter()
                .enumerate()
                .map(|(index, connection_index)| {
                    let connection = &graph.connections[*connection_index];
                    let line = Line::from(format!(
                        "  {:<16} {}.{}  ──▶  {}.{}  · {}:{}",
                        connection.id,
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

        let voice_item_count = connection_offset + graph.connections.len();
        let shared_item_count = if let Some(shared) = &instrument.shared_graph {
            let shared_processing = app.candidate_is_audible();
            let shared_node_order = graph_processing_node_indexes(instrument, shared);
            let shared_bus_order = graph_processing_bus_indexes(shared, &shared_node_order);
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(
                    "SHARED POST-VOICE NODES  {} / {} declared ceiling  ·  one voice_sum.audio crossing  ·  {}",
                    shared.nodes.len(),
                    shared.max_nodes,
                    if shared_processing { "processing" } else { "idle" }
                ),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.extend(
                shared_node_order
                    .iter()
                    .enumerate()
                    .map(|(index, node_index)| {
                        let node = &shared.nodes[*node_index];
                        let settings = node
                            .settings
                            .iter()
                            .map(|setting| format!("{}={:?}", setting.key, setting.value))
                            .collect::<Vec<_>>()
                            .join("  ");
                        let line = Line::from(vec![
                            Span::styled(
                                if shared_processing { " ● " } else { " ○ " },
                                Style::default().fg(if shared_processing {
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
                        if voice_item_count + index == selected {
                            line.style(selected_style())
                        } else {
                            line
                        }
                    }),
            );
            let shared_bus_offset = voice_item_count + shared.nodes.len();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "SHARED BUSES",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.extend(
                shared_bus_order
                    .iter()
                    .enumerate()
                    .map(|(index, connection_index)| {
                        let connection = &shared.connections[*connection_index];
                        let line = Line::from(format!(
                            "  {:<16} {}.{}  ──▶  {}.{}  · {}:{}",
                            connection.id,
                            connection.from.node,
                            connection.from.port,
                            connection.to.node,
                            connection.to.port,
                            connection.location.file.display(),
                            connection.location.line
                        ));
                        if shared_bus_offset + index == selected {
                            line.style(selected_style())
                        } else {
                            line
                        }
                    }),
            );
            shared.nodes.len() + shared.connections.len()
        } else {
            0
        };

        let parameter_offset = voice_item_count + shared_item_count;
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
        let voice_items = graph.nodes.len() + graph.connections.len();
        let shared_nodes = instrument
            .shared_graph
            .as_ref()
            .map_or(0, |graph| graph.nodes.len());
        let shared_buses = instrument
            .shared_graph
            .as_ref()
            .map_or(0, |graph| graph.connections.len());
        if selected < graph.nodes.len() {
            1 + selected
        } else if selected < voice_items {
            3 + selected
        } else if selected < voice_items + shared_nodes {
            5 + selected
        } else if selected < voice_items + shared_nodes + shared_buses {
            7 + selected
        } else if selected < voice_items + shared_nodes + shared_buses + instrument.parameters.len()
        {
            if instrument.shared_graph.is_some() {
                9 + selected
            } else {
                5 + selected
            }
        } else if instrument.shared_graph.is_some() {
            11 + selected
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
    let list = List::new(items)
        .block(
            Block::default()
                .title(format!(
                    " Candidate Diagnostics · {} finding(s) ",
                    diagnostics.len()
                ))
                .borders(Borders::ALL),
        )
        .highlight_style(selected_style())
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(
        (!diagnostics.is_empty()).then_some(
            app.selected_diagnostic
                .min(diagnostics.len().saturating_sub(1)),
        ),
    );
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_history(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let selected = app
        .selected_history
        .min(app.history.len().saturating_sub(1));
    let row_capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let window = selection_window(app.history.len(), selected, row_capacity);
    let rows = app.history[window.clone()].iter().map(|entry| {
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
    let mut state = TableState::default()
        .with_selected((!window.is_empty()).then_some(selected.saturating_sub(window.start)));
    frame.render_stateful_widget(table, area, &mut state);
}

fn help_lines() -> Vec<Line<'static>> {
    let heading = |text: &'static str| {
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    };
    vec![
        heading("WORKSPACE"),
        Line::from("1–9/0 views · gt/gT or Tab/Shift-Tab cycle · H History · ? Help"),
        Line::from("j/k or ↑/↓ select · gg/G first/last · Ctrl-u/Ctrl-d move 4 rows"),
        Line::from("Space start/pause/resume · S stop · P publish · Ctrl-C is the only exit"),
        Line::from("Esc or Ctrl-[ cancels a modal/mode and returns to NORMAL; it never exits"),
        Line::from(""),
        heading("TIMELINE MOTION"),
        Line::from(
            "h/l or </> grid step · w/b next/previous boundary · e Clip end · % matching edge",
        ),
        Line::from("^/$ first/last Track material · g0/g$ Song edges · */# matching Pattern Clip"),
        Line::from(
            "{/} previous/next Locator · [/] grid size · : exact jump · / search · n/N repeat",
        ),
        Line::from("ma set transient mark · 'a jump · '' return · Ctrl-o/Ctrl-i jump history"),
        Line::from(""),
        heading("TIMELINE RANGE / VIEWPORT"),
        Line::from("V RANGE mode · h/l or </> move edge · o swap edge · L loop · Enter one-shot"),
        Line::from("gv restore prior range · zf fit range · zz center · zs/ze align cursor"),
        Line::from("zh/zl or ,/. pan · Ctrl-b/Ctrl-f full viewport · +/- zoom · Home fit Song"),
        Line::from("f follow playhead · L toggles armed loop · M creates a durable Locator"),
        Line::from(""),
        heading("TIMELINE CONTEXT"),
        Line::from("gd Pattern/Event Grid · gf source · gi Instrument · gm Mixer · gp Processing"),
        Line::from("A arms selected Track · Enter exact level · p exact pan"),
        Line::from(""),
        heading("OTHER PRODUCTION VIEWS"),
        Line::from("Processing: A target · a path · F range · L loop · b/B bypass · K/J reorder"),
        Line::from(
            "Event Grid: A audition · t/u/v time/duration/velocity · x/f/Alt-g/p sample edits",
        ),
        Line::from("Faceplate/Parameters: ←/→ bounded step · Enter exact physical value"),
        Line::from("Instrument: ←/→ section · Enter edit/source · b route bypass"),
        Line::from("Automation: [/] lane · ←/→ value · Enter exact · t time · c curve"),
        Line::from("Graph: ←/→ Instrument · Enter source · Source: ←/→ file · e external editor"),
        Line::from("History: Enter/d diff · R restore as new entry"),
        Line::from(""),
        heading("NOTE MODE"),
        Line::from("Press i in Instrument views; A W S E D F T G Y H U J K plays C4–C5"),
        Line::from("Esc releases held notes and returns to NORMAL; j/k navigate outside NOTE mode"),
        Line::from(""),
        heading("SAFETY"),
        Line::from(
            "Cursor, range, marks, search, Solo/cue/bypass, and loops are transient Audition State",
        ),
        Line::from(
            "M and other Production View edits are checked source edits recorded in History",
        ),
        Line::from("Invalid Candidate source never replaces the prior audible Active Revision"),
    ]
}

pub(super) fn help_line_count() -> usize {
    help_lines().len()
}

fn render_help(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    frame.render_widget(
        Paragraph::new(help_lines())
            .scroll((app.help_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
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
    let mut status = vec![Line::from(vec![
        Span::styled(" ", Style::default().bg(Color::Cyan)),
        Span::styled(
            format!(" {} ", app.status),
            Style::default().fg(Color::White),
        ),
    ])];
    if let Some(performance) = app.performance.footer() {
        status.push(Line::from(Span::styled(
            format!(" {performance}"),
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(
        Paragraph::new(status).block(Block::default().borders(Borders::TOP)),
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

fn selection_window(total: usize, selected: usize, capacity: usize) -> std::ops::Range<usize> {
    if total == 0 {
        return 0..0;
    }
    let capacity = capacity.max(1).min(total);
    let selected = selected.min(total - 1);
    let start = selected.saturating_sub(capacity / 2).min(total - capacity);
    start..start + capacity
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

#[cfg(test)]
mod tests {
    use super::{instrument_modulation_route_label, selection_window};
    use crate::{
        engine::control::InstrumentModulationValueSnapshot,
        model::{InstrumentModulationScope, Unit},
    };

    fn route(scope: InstrumentModulationScope) -> InstrumentModulationValueSnapshot {
        InstrumentModulationValueSnapshot {
            route: "motion".to_owned(),
            source: "source".to_owned(),
            scope,
            order: 0,
            bypass: false,
            normalized: None,
            auxiliary: None,
            depth: 0.5,
            contribution: (scope == InstrumentModulationScope::Global).then_some(0.25),
        }
    }

    #[test]
    fn selection_windows_are_bounded_and_keep_the_selection_visible() {
        assert_eq!(selection_window(0, 0, 10), 0..0);
        assert_eq!(selection_window(4, 3, 10), 0..4);
        assert_eq!(selection_window(100, 0, 10), 0..10);
        assert_eq!(selection_window(100, 50, 10), 45..55);
        assert_eq!(selection_window(100, 99, 10), 90..100);
        assert_eq!(selection_window(100, usize::MAX, 10), 90..100);
    }

    #[test]
    fn instrument_modulation_labels_preserve_global_track_and_voice_scope() {
        assert!(
            instrument_modulation_route_label(
                &route(InstrumentModulationScope::Global),
                Unit::Ratio
            )
            .contains("[global]")
        );
        assert!(
            instrument_modulation_route_label(
                &route(InstrumentModulationScope::Instrument),
                Unit::Ratio,
            )
            .contains("[instrument/per-track]")
        );
        assert!(
            instrument_modulation_route_label(
                &route(InstrumentModulationScope::PerVoice),
                Unit::Ratio,
            )
            .contains("[per_voice]")
        );
    }
}
