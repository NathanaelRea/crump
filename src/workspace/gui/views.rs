//! Every view except the Timeline: source-backed tables, the Event Grid roll, and inspection.

use super::widgets::{Cell, apply_row_click, context_combo, table, toolbar, unavailable};
use super::*;
use crate::{diagnostic::Severity, workspace::instrument::instrument_modulation_route_label};

pub(super) fn view(ui: &mut Ui, cx: &mut Cx<'_>) {
    match cx.app.view {
        View::Timeline => arrangement::timeline(ui, cx),
        View::Mixer => mixer(ui, cx),
        View::Processing => processing(ui, cx),
        View::EventGrid => event_grid(ui, cx),
        View::Faceplate => faceplate(ui, cx),
        View::Instrument => instrument_sections(ui, cx),
        View::Parameters => parameters(ui, cx),
        View::Automation => automation(ui, cx),
        View::Graph => graph(ui, cx),
        View::Source => source(ui, cx),
        View::Diagnostics => diagnostics(ui, cx),
        View::History => history(ui, cx),
        View::Help => help(ui),
    }
}

fn step_buttons(ui: &mut Ui, cx: &mut Cx<'_>) {
    toolbar(
        ui,
        cx,
        &[
            ("−", Action::AdjustSelected(-1), "One bounded step down · ←"),
            ("+", Action::AdjustSelected(1), "One bounded step up · →"),
            (
                "Exact…",
                Action::OpenExactEdit,
                "Enter an exact value · Enter",
            ),
        ],
    );
}

fn instrument_combo(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let names = project
        .song
        .instruments
        .iter()
        .map(|instrument| instrument.name.clone())
        .collect::<Vec<_>>();
    context_combo(
        ui,
        cx,
        "Instrument",
        &names,
        cx.app.selected_instrument,
        Action::CycleContext,
    );
}

fn mixer(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Mixer unavailable while Project source is unparseable");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        step_buttons(ui, cx);
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                ("Mute", Action::ToggleMixerMute, "Authored source Mute · m"),
                ("Solo", Action::ToggleMixerSolo, "Transient Solo · s"),
                (
                    "Cue post",
                    Action::ToggleMixerCue(TapPoint::PostFader),
                    "Transient post-fader cue · c",
                ),
                (
                    "Cue pre",
                    Action::ToggleMixerCue(TapPoint::PreFader),
                    "Transient pre-fader cue · C",
                ),
            ],
        );
        ui.separator();
        ui.label(
            RichText::new(format!(
                "{} Tracks · {} Groups · {} Returns · {} Sidechains",
                project.song.tracks.len(),
                project.song.groups.len(),
                project.song.returns.len(),
                project.song.sidechains.len()
            ))
            .color(palette::MUTED),
        );
    });
    let selected = clamp_index(app.selected_mixer_parameter, app.mixer_parameters.len());
    let click = table(
        ui,
        cx.state,
        "mixer",
        &[
            ("Stable target", 300.0),
            ("Base", 100.0),
            ("Legal domain", 190.0),
            ("Peak", 130.0),
            ("Average", 80.0),
            ("M/S/C", 60.0),
            ("A/M", 50.0),
        ],
        app.mixer_parameters.len(),
        Some(selected),
        |index| {
            let parameter = &app.mixer_parameters[index];
            let spec = &parameter.spec;
            let identity = parameter.channel_identity.as_ref();
            let meter = identity
                .and_then(|identity| app.mix_channel_indexes.get(identity).copied())
                .filter(|index| *index < app.telemetry.engine.channel_count)
                .map(|index| app.telemetry.engine.channel_activity[index]);
            let audition = identity.map_or_else(
                || "—".to_owned(),
                |identity| {
                    let mark = |on: bool, letter: char| if on { letter } else { '·' };
                    format!(
                        "{}{}{}",
                        mark(parameter.muted, 'M'),
                        mark(app.audition_plan.soloed_channels.contains(identity), 'S'),
                        mark(
                            app.audition_plan
                                .cue
                                .as_ref()
                                .is_some_and(|cue| cue.channel == *identity),
                            'C'
                        )
                    )
                },
            );
            vec![
                parameter.target.to_string().into(),
                spec.unit.format_value(spec.base).into(),
                format!(
                    "{} … {}",
                    spec.unit.format_value(spec.minimum),
                    spec.unit.format_value(spec.maximum)
                )
                .into(),
                meter.map_or_else(|| "—".into(), |meter| Cell::Meter(meter.current_peak)),
                meter
                    .map_or_else(|| "—".to_owned(), |meter| format!("{:.3}", meter.average))
                    .into(),
                audition.into(),
                format!(
                    "{}/{}",
                    if spec.automatable { "A" } else { "·" },
                    if spec.modulatable { "M" } else { "·" }
                )
                .into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

fn processing(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(graph) = app.processing_graph() else {
        unavailable(
            ui,
            "Processing inspection unavailable while the selected Project cannot compile",
        );
        return;
    };
    ui.horizontal_wrapped(|ui| {
        toolbar(
            ui,
            cx,
            &[
                (
                    "Target…",
                    Action::OpenAuditionTarget,
                    "Choose the audition target · A",
                ),
                (
                    "Path",
                    Action::CycleAuditionPath,
                    "Step the audition path · a",
                ),
                (
                    "Range…",
                    Action::OpenAuditionRange,
                    "Exact audition range · F",
                ),
                (
                    "Loop",
                    Action::ToggleAuditionRangeLoop,
                    "Loop the audition range · L",
                ),
            ],
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Bypass",
                    Action::ToggleTransientProcessorBypass,
                    "Transient bypass of the selected Processor · b",
                ),
                (
                    "Bypass in source",
                    Action::ToggleDurableProcessorBypass,
                    "Authored bypass of the selected Processor · B",
                ),
                (
                    "Setting…",
                    Action::OpenExactEdit,
                    "Edit a Processor setting · Enter",
                ),
                (
                    "Earlier",
                    Action::MoveSelectedProcessor(MoveDirection::Earlier),
                    "Move the Processor earlier in its chain · K",
                ),
                (
                    "Later",
                    Action::MoveSelectedProcessor(MoveDirection::Later),
                    "Move the Processor later in its chain · J",
                ),
            ],
        );
    });
    let status = app.audition_status.as_ref();
    let target = status.map_or_else(
        || format!("{:?}", app.audition_plan.target),
        |status| format!("{:?}", status.target),
    );
    let path = status.map_or_else(
        || format!("{:?}", app.audition_plan.path),
        |status| format!("{:?}", status.path),
    );
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
    ui.label(format!("Unified Audition · Target {target} · Path {path}"));
    ui.label(
        RichText::new(format!(
            "Active Revision {revision} · Range {range} · Transport {}{}",
            transport_label(app),
            if app.playback_paused { " paused" } else { "" }
        ))
        .color(palette::MUTED),
    );
    let selected = clamp_index(app.selected_processor, graph.processors.len());
    let click = table(
        ui,
        cx.state,
        "processing",
        &[
            ("#", 32.0),
            ("Owner / durable ID", 230.0),
            ("Kind", 100.0),
            ("Bypass D/T/E", 170.0),
            ("Input", 110.0),
            ("Output", 110.0),
            ("GR now/max", 90.0),
            ("Latency", 60.0),
            ("Resolved settings (* authored)", 300.0),
        ],
        graph.processors.len(),
        Some(selected),
        |index| {
            let processor = &graph.processors[index];
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
            let on_off = |value: bool| if value { "on" } else { "off" };
            let settings = processor
                .settings
                .iter()
                .map(|setting| {
                    let value = setting.choice.clone().unwrap_or_else(|| {
                        setting.unit.zip(setting.value).map_or_else(
                            || "—".to_owned(),
                            |(unit, value)| unit.format_value(value),
                        )
                    });
                    format!(
                        "{}={value}{}",
                        setting.key,
                        if setting.authored { "*" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            vec![
                processor.index.to_string().into(),
                format!("{} / {}", processor.owner, processor.id).into(),
                format!("{:?}", processor.kind).to_lowercase().into(),
                format!(
                    "D:{} T:{} E:{}{}",
                    on_off(processor.authored_bypass),
                    transient.map_or("—", on_off),
                    on_off(effective),
                    meter.map_or_else(String::new, |meter| format!(
                        " {:.0}%",
                        meter.bypass_mix * 100.0
                    ))
                )
                .into(),
                meter.map_or_else(|| "—".into(), |meter| Cell::Meter(meter.input_peak)),
                meter.map_or_else(|| "—".into(), |meter| Cell::Meter(meter.output_peak)),
                meter
                    .map_or_else(
                        || "—".to_owned(),
                        |meter| {
                            format!(
                                "{:.1}/{:.1}",
                                meter.gain_reduction_db, meter.maximum_gain_reduction_db
                            )
                        },
                    )
                    .into(),
                processor.output_latency_frames.to_string().into(),
                settings.into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

fn event_grid(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Event Grid unavailable");
        return;
    };
    let Some(pattern) = project.song.patterns.get(app.selected_pattern) else {
        unavailable(ui, "No Pattern");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        let names = project
            .song
            .patterns
            .iter()
            .map(|pattern| pattern.id.clone())
            .collect::<Vec<_>>();
        context_combo(
            ui,
            cx,
            "Pattern",
            &names,
            app.selected_pattern,
            Action::CycleContext,
        );
        ui.label(
            RichText::new(format!(
                "span {} · playhead {}",
                pattern.span,
                transport_label(app)
            ))
            .color(palette::MUTED),
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "−",
                    Action::AdjustSelected(-1),
                    "Previous note or voice · ←",
                ),
                ("+", Action::AdjustSelected(1), "Next note or voice · →"),
                (
                    "Pitch / Voice…",
                    Action::OpenExactEdit,
                    "Exact pitch or voice · Enter",
                ),
                ("Time…", Action::OpenEventTime, "Exact event time · t"),
                (
                    "Duration…",
                    Action::OpenNoteDuration,
                    "Exact note duration · u",
                ),
                ("Velocity…", Action::OpenEventVelocity, "Exact velocity · v"),
            ],
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Region…",
                    Action::OpenSampleRegion,
                    "Sample source region · x",
                ),
                ("Fades…", Action::OpenSampleFade, "Sample fades · f"),
                ("Gain…", Action::OpenSampleGain, "Sample gain · Alt-g"),
                ("Pan…", Action::OpenSamplePan, "Sample pan · p"),
            ],
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Audition",
                    Action::ArmSelectedAudition,
                    "Arm this Pattern as the audition target · A",
                ),
                (
                    "Loop Pattern",
                    Action::TogglePatternLoop,
                    "Loop exactly this Pattern · L",
                ),
            ],
        );
    });
    let selected = clamp_index(app.selected_event, pattern.events.len());
    if let Some(index) = event_roll(ui, app, pattern, selected) {
        cx.select(selected, index);
    }
    let procedure = pattern
        .variation
        .iter()
        .map(|variation| {
            format!(
                "Variation from {} · rotate {} · transpose {:+}st · velocity ×{} · reverse {} · probability {} · seed {}",
                variation.source,
                variation.rotate,
                variation.transpose_semitones,
                variation.velocity_scale,
                variation.reverse,
                variation.probability,
                variation.seed
            )
        })
        .chain(pattern.locks.iter().map(|lock| {
            format!(
                "Lock {} · {} · {} = {}",
                lock.id, lock.at, lock.parameter, lock.value
            )
        }))
        .collect::<Vec<_>>();
    if !procedure.is_empty() {
        egui::TopBottomPanel::bottom("crump-event-procedure")
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                ui.separator();
                ui.label(RichText::new("Procedural source & Parameter locks").strong());
                egui::ScrollArea::vertical()
                    .max_height(96.0)
                    .show(ui, |ui| {
                        for line in &procedure {
                            ui.label(line);
                        }
                    });
            });
    }
    let click = table(
        ui,
        cx.state,
        "event-grid",
        &[
            ("#", 44.0),
            ("At", 110.0),
            ("Kind", 80.0),
            ("Pitch / Voice", 150.0),
            ("Duration", 110.0),
            ("Velocity", 80.0),
        ],
        pattern.events.len(),
        Some(selected),
        |index| {
            let event = &pattern.events[index];
            let (kind, what, duration, velocity) = match event {
                PatternEvent::Note {
                    pitch,
                    duration,
                    velocity,
                    ..
                } => (
                    "note",
                    crate::model::format_pitch(*pitch),
                    duration.to_string(),
                    velocity,
                ),
                PatternEvent::Hit {
                    voice, velocity, ..
                } => (
                    "hit",
                    format!("{voice:?}").to_lowercase(),
                    "—".to_owned(),
                    velocity,
                ),
                PatternEvent::Sample {
                    id,
                    mapping,
                    duration,
                    velocity,
                    ..
                } => (
                    "sample",
                    format!("{id}:{mapping}"),
                    duration.to_string(),
                    velocity,
                ),
            };
            let cells = [
                index.to_string(),
                event.at().to_string(),
                kind.to_owned(),
                what,
                duration,
                format!("{velocity:.2}"),
            ];
            let active = event_is_active(app, &pattern.id, event);
            cells
                .into_iter()
                .map(|text| {
                    if active {
                        Cell::Colored(text, palette::GOOD)
                    } else {
                        Cell::Text(text)
                    }
                })
                .collect()
        },
    );
    apply_row_click(cx, click, selected);
}

/// Paints the Pattern as a roll: one row per pitch, drum voice, or sample mapping, with time
/// running left to right across the Pattern span. Returns the event the pointer clicked.
fn event_roll(
    ui: &mut Ui,
    app: &Workspace,
    pattern: &crate::model::Pattern,
    selected: usize,
) -> Option<usize> {
    /// The roll row of one event: notes by pitch, then drum voices, then sample mappings.
    fn row_key(event: &PatternEvent) -> (u8, String) {
        match event {
            PatternEvent::Note { pitch, .. } => (0, format!("{:03}", 127 - pitch.min(&127))),
            PatternEvent::Hit { voice, .. } => (1, format!("{voice:?}").to_lowercase()),
            PatternEvent::Sample { mapping, .. } => (2, mapping.clone()),
        }
    }
    fn row_label(event: &PatternEvent) -> String {
        match event {
            PatternEvent::Note { pitch, .. } => crate::model::format_pitch(*pitch),
            PatternEvent::Hit { voice, .. } => format!("{voice:?}").to_lowercase(),
            PatternEvent::Sample { mapping, .. } => mapping.clone(),
        }
    }

    const LABEL_WIDTH: f32 = 64.0;
    let mut rows = BTreeMap::new();
    for event in &pattern.events {
        rows.entry(row_key(event))
            .or_insert_with(|| row_label(event));
    }
    if rows.is_empty() {
        return None;
    }
    let row_height = (150.0 / rows.len() as f32).clamp(5.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), row_height * rows.len() as f32 + 4.0),
        Sense::click(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, palette::LANE);
    let roll = Rect::from_min_max(
        Pos2::new(rect.left() + LABEL_WIDTH, rect.top() + 2.0),
        Pos2::new(rect.right() - 2.0, rect.bottom() - 2.0),
    );
    let span = pattern.span.as_f64().max(f64::EPSILON);
    let x_of =
        |quarters: f64| roll.left() + (quarters / span).clamp(0.0, 1.0) as f32 * roll.width();
    for beat in 1..span.ceil() as u32 {
        painter.vline(
            x_of(f64::from(beat)),
            roll.y_range(),
            Stroke::new(1.0_f32, palette::GRID_BEAT),
        );
    }
    for (index, label) in rows.values().enumerate() {
        let top = roll.top() + row_height * index as f32;
        if index % 2 == 1 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), top),
                    Pos2::new(rect.right(), top + row_height),
                ),
                0.0,
                palette::LANE_ALTERNATE,
            );
        }
        if row_height >= 11.0 {
            painter.text(
                Pos2::new(rect.left() + 4.0, top + row_height / 2.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(10.0),
                palette::MUTED,
            );
        }
    }
    let pointer = response
        .clicked()
        .then(|| response.interact_pointer_pos())
        .flatten();
    let mut clicked = None;
    for (index, event) in pattern.events.iter().enumerate() {
        let row = rows.keys().position(|key| *key == row_key(event))?;
        let duration = match event {
            PatternEvent::Note { duration, .. } | PatternEvent::Sample { duration, .. } => {
                duration.as_f64()
            }
            PatternEvent::Hit { .. } => 0.0,
        };
        let left = x_of(event.at().as_f64());
        let top = roll.top() + row_height * row as f32;
        let block = Rect::from_min_max(
            Pos2::new(left, top + 1.0),
            Pos2::new(
                x_of(event.at().as_f64() + duration).max(left + 4.0),
                top + row_height - 1.0,
            ),
        );
        let color = if event_is_active(app, &pattern.id, event) {
            palette::GOOD
        } else if matches!(event, PatternEvent::Sample { .. }) {
            palette::WARN
        } else {
            palette::ACCENT
        };
        painter.rect_filled(block, 2.0, color);
        if index == selected {
            painter.rect_stroke(
                block.expand(1.0),
                2.0,
                Stroke::new(1.5_f32, Color32::WHITE),
                StrokeKind::Outside,
            );
        }
        if pointer.is_some_and(|pointer| block.expand(2.0).contains(pointer)) {
            clicked = Some(index);
        }
    }
    ui.add_space(4.0);
    clicked
}

fn faceplate(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Instrument Faceplate unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        instrument_combo(ui, cx, project);
        ui.label(
            RichText::new(format!(
                "{} · polyphony {}",
                instrument.kind, instrument.polyphony
            ))
            .color(palette::MUTED),
        );
    });
    if instrument.faceplate.is_empty() {
        unavailable(ui, "This Instrument has no authored Faceplate controls");
        return;
    }
    let selected = clamp_index(app.selected_parameter, instrument.faceplate.len());
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (index, control) in instrument.faceplate.iter().enumerate() {
                let parameter = instrument
                    .parameters
                    .iter()
                    .find(|parameter| parameter.key == control.parameter);
                let mut frame = egui::Frame::group(ui.style());
                if index == selected {
                    frame = frame
                        .stroke(Stroke::new(1.5_f32, palette::ACCENT))
                        .fill(ui.visuals().faint_bg_color);
                }
                let card = frame.show(ui, |ui| {
                    ui.set_width(200.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&control.label).strong());
                        ui.label(
                            RichText::new(parameter.map_or_else(
                                || "missing".to_owned(),
                                |parameter| parameter.unit.format_value(parameter.value),
                            ))
                            .size(20.0)
                            .color(palette::WARN),
                        );
                        if let Some(parameter) = parameter {
                            ui.label(
                                RichText::new(format!(
                                    "{} … {}",
                                    parameter.unit.format_value(parameter.minimum),
                                    parameter.unit.format_value(parameter.maximum)
                                ))
                                .small()
                                .color(palette::MUTED),
                            );
                        }
                        ui.label(RichText::new(&control.explanation).small());
                        ui.horizontal(|ui| {
                            for (label, action) in [
                                ("−", Action::AdjustSelected(-1)),
                                ("+", Action::AdjustSelected(1)),
                                ("Exact…", Action::OpenExactEdit),
                            ] {
                                if ui.button(label).clicked() {
                                    cx.select(selected, index);
                                    cx.send(action);
                                }
                            }
                        });
                    });
                });
                if card.response.interact(Sense::click()).clicked() {
                    cx.select(selected, index);
                }
            }
        });
    });
}

fn instrument_sections(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Instrument sections unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };
    let sections = app.instrument_sections();
    ui.horizontal_wrapped(|ui| {
        instrument_combo(ui, cx, project);
        ui.label(
            RichText::new(format!("{} · {} sections", instrument.kind, sections.len()))
                .color(palette::MUTED),
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Edit / Source",
                    Action::OpenExactEdit,
                    "Edit the selected item or open its source · Enter",
                ),
                (
                    "Route bypass",
                    Action::ToggleInstrumentRouteBypass,
                    "Toggle bypass of the selected modulation route · b",
                ),
            ],
        );
    });
    let Some(section) = sections.get(app.selected_instrument_section) else {
        unavailable(ui, "No generated Instrument sections");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for (index, section) in sections.iter().enumerate() {
            if ui
                .selectable_label(index == app.selected_instrument_section, &section.name)
                .clicked()
                && index != app.selected_instrument_section
            {
                cx.send(Action::AdjustSelected(
                    index as isize - app.selected_instrument_section as isize,
                ));
            }
        }
    });
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
    let selected = clamp_index(app.selected_instrument_row, section.rows.len());
    let click = table(
        ui,
        cx.state,
        "instrument",
        &[
            ("Type", 90.0),
            ("Identity", 200.0),
            ("Authoritative contract / control activity", 520.0),
            ("Exact source", 220.0),
        ],
        section.rows.len(),
        Some(selected),
        |index| {
            let row = &section.rows[index];
            let detail = match route_activity.get(&row.id) {
                Some(activity) if row.kind == Some(PresentationItemKind::Route) => format!(
                    "{} · {} {activity}",
                    row.detail,
                    if live_control {
                        "live"
                    } else {
                        "Candidate preview"
                    }
                ),
                _ => row.detail.clone(),
            };
            vec![
                row.kind
                    .map_or_else(|| "policy".to_owned(), |kind| format!("{kind:?}"))
                    .into(),
                row.id.clone().into(),
                detail.into(),
                row.source.clone().into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

fn parameters(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Parameters unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        instrument_combo(ui, cx, project);
        ui.separator();
        step_buttons(ui, cx);
    });
    let selected = clamp_index(app.selected_parameter, instrument.parameters.len());
    let click = table(
        ui,
        cx.state,
        "parameters",
        &[
            ("Parameter", 190.0),
            ("Key", 150.0),
            ("Unit", 90.0),
            ("Value", 120.0),
            ("Legal domain", 200.0),
            ("Default", 120.0),
            ("Automate", 70.0),
            ("Modulate", 70.0),
        ],
        instrument.parameters.len(),
        Some(selected),
        |index| {
            let parameter = &instrument.parameters[index];
            let yes_no = |value: bool| if value { "yes" } else { "no" };
            vec![
                parameter.name.clone().into(),
                parameter.key.clone().into(),
                parameter.unit.to_string().into(),
                Cell::Colored(parameter.unit.format_value(parameter.value), palette::WARN),
                format!(
                    "{} … {}",
                    parameter.unit.format_value(parameter.minimum),
                    parameter.unit.format_value(parameter.maximum)
                )
                .into(),
                parameter.unit.format_value(parameter.default).into(),
                yes_no(parameter.automatable).into(),
                yes_no(parameter.modulatable).into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

fn automation(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(
            ui,
            "Automation unavailable while Project source is unparseable",
        );
        return;
    };
    let Some(lane) = project.song.automations.get(app.selected_automation) else {
        unavailable(
            ui,
            "No Song Automation lanes · use the CLI or readable source to create one",
        );
        return;
    };
    ui.horizontal_wrapped(|ui| {
        let names = project
            .song
            .automations
            .iter()
            .map(|lane| format!("{} -> {}", lane.id, lane.target))
            .collect::<Vec<_>>();
        context_combo(
            ui,
            cx,
            "Lane",
            &names,
            app.selected_automation,
            Action::CycleContext,
        );
        ui.separator();
        step_buttons(ui, cx);
        toolbar(
            ui,
            cx,
            &[
                ("Time…", Action::OpenAutomationTime, "Exact point time · t"),
                ("Curve…", Action::OpenAutomationCurve, "Point curve · c"),
            ],
        );
    });
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
                "base {base}  ->  automation {automation}  +  modulation {} [{}]  ->  effective {}{}",
                value.unit.format_value(value.modulation),
                if routes.is_empty() { "none" } else { &routes },
                value.unit.format_value(value.effective),
                if value.clamped { "  CLAMPED" } else { "" }
            )
        },
    );
    ui.label(format!(
        "Lane {} · target {} · {} points · source {}:{}",
        lane.id,
        lane.target,
        lane.points.len(),
        lane.location.file.display(),
        lane.location.line
    ));
    ui.label(
        RichText::new(format!(
            "{} frame {frame_position} · {provenance}",
            if live_control {
                "live"
            } else {
                "Candidate preview"
            }
        ))
        .color(palette::MUTED),
    );
    let selected = clamp_index(app.selected_automation_point, lane.points.len());
    let click = table(
        ui,
        cx.state,
        "automation",
        &[
            ("Point", 180.0),
            ("Song time", 130.0),
            ("Physical value", 150.0),
            ("Curve out", 120.0),
            ("Source", 220.0),
        ],
        lane.points.len(),
        Some(selected),
        |index| {
            let point = &lane.points[index];
            vec![
                point.id.clone().into(),
                point.at.to_string().into(),
                Cell::Colored(point.value.clone(), palette::WARN),
                point.interpolation.to_string().into(),
                format!("{}:{}", point.location.file.display(), point.location.line).into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

fn graph(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Graph unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        instrument_combo(ui, cx, project);
        ui.separator();
        toolbar(
            ui,
            cx,
            &[(
                "Open source",
                Action::OpenExactEdit,
                "Show the selected item in Source · Enter",
            )],
        );
    });
    let Some(voice) = &instrument.graph else {
        unavailable(
            ui,
            "Stock synthesized percussion · kick · snare · hat · clap · no editable topology",
        );
        return;
    };

    // The sections below appear in the order the workspace numbers Graph items.
    let location =
        |location: &SourceLocation| format!("{}:{}", location.file.display(), location.line);
    let node_rows = |graph: &Graph, order: &[usize]| {
        order
            .iter()
            .map(|index| {
                let node = &graph.nodes[*index];
                let settings = node
                    .settings
                    .iter()
                    .map(|setting| format!("{}={:?}", setting.key, setting.value))
                    .collect::<Vec<_>>()
                    .join("  ");
                format!(
                    "{:<16} {:<12} {settings}  · {}",
                    node.id,
                    format!("{:?}", node.kind).to_lowercase(),
                    location(&node.location)
                )
            })
            .collect::<Vec<_>>()
    };
    let bus_rows = |graph: &Graph, order: &[usize]| {
        order
            .iter()
            .map(|index| {
                let connection = &graph.connections[*index];
                format!(
                    "{:<16} {}.{}  ->  {}.{}  · {}",
                    connection.id,
                    connection.from.node,
                    connection.from.port,
                    connection.to.node,
                    connection.to.port,
                    location(&connection.location)
                )
            })
            .collect::<Vec<_>>()
    };
    let activity = |processing: bool| if processing { "processing" } else { "idle" };
    let voice_order = graph_processing_node_indexes(instrument, voice);
    let mut sections = vec![
        (
            format!(
                "VOICE NODES  {} / {} declared ceiling · {}",
                voice.nodes.len(),
                voice.max_nodes,
                activity(app.candidate_is_audible() && app.telemetry.engine.active_voices > 0)
            ),
            node_rows(voice, &voice_order),
        ),
        (
            "BUSES".to_owned(),
            bus_rows(voice, &graph_processing_bus_indexes(voice, &voice_order)),
        ),
    ];
    if let Some(shared) = &instrument.shared_graph {
        let shared_order = graph_processing_node_indexes(instrument, shared);
        sections.push((
            format!(
                "SHARED POST-VOICE NODES  {} / {} declared ceiling · one voice_sum.audio crossing · {}",
                shared.nodes.len(),
                shared.max_nodes,
                activity(app.candidate_is_audible())
            ),
            node_rows(shared, &shared_order),
        ));
        sections.push((
            "SHARED BUSES".to_owned(),
            bus_rows(shared, &graph_processing_bus_indexes(shared, &shared_order)),
        ));
    }
    sections.push((
        "PARAMETERS".to_owned(),
        instrument
            .parameters
            .iter()
            .map(|parameter| {
                format!(
                    "{:<16} {} = {}  [{} … {}]  · {}",
                    parameter.key,
                    parameter.unit,
                    parameter.unit.format_value(parameter.value),
                    parameter.unit.format_value(parameter.minimum),
                    parameter.unit.format_value(parameter.maximum),
                    location(&parameter.location)
                )
            })
            .collect(),
    ));
    sections.push((
        "FACEPLATE MAPPINGS".to_owned(),
        instrument
            .faceplate
            .iter()
            .map(|control| {
                format!(
                    "{:<16} {}  ->  ${}  · {}",
                    control.key,
                    control.label,
                    control.parameter,
                    location(&control.location)
                )
            })
            .collect(),
    ));

    let selected = app.selected_graph_node;
    let follow = cx.state.scrolled.insert("graph", selected) != Some(selected);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut index = 0;
            for (heading, rows) in sections {
                ui.add_space(6.0);
                ui.label(RichText::new(heading).strong().color(palette::ACCENT));
                for row in rows {
                    let response =
                        ui.selectable_label(index == selected, RichText::new(row).monospace());
                    if index == selected && follow {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        cx.select(selected, index);
                    }
                    if response.double_clicked() {
                        cx.send(Action::OpenExactEdit);
                    }
                    index += 1;
                }
            }
        });
}

fn source(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some((_, text)) = app.bundle.sources.iter().nth(app.selected_source) else {
        unavailable(ui, "No source file");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        let names = app
            .bundle
            .sources
            .keys()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        context_combo(
            ui,
            cx,
            "File",
            &names,
            app.selected_source,
            Action::AdjustSelected,
        );
        toolbar(
            ui,
            cx,
            &[(
                "Open in editor",
                Action::EditSource,
                "Open this file with the desktop's default application · e",
            )],
        );
        ui.label(RichText::new("Authoritative Source · read-only here").color(palette::MUTED));
    });
    let lines = text.lines().collect::<Vec<_>>();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
    // Keyboard and go-to-definition scrolling arrive as a session line number; the scroll wheel
    // otherwise owns the position.
    if cx.state.source_scroll.replace(app.source_scroll) != Some(app.source_scroll) {
        area = area.vertical_scroll_offset(
            app.source_scroll as f32 * (row_height + ui.spacing().item_spacing.y),
        );
    }
    area.show_rows(ui, row_height, lines.len(), |ui, rows| {
        for index in rows {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{:>5} ", index + 1))
                        .monospace()
                        .color(if index == app.source_scroll {
                            palette::ACCENT
                        } else {
                            palette::MUTED
                        }),
                );
                ui.label(RichText::new(lines[index]).monospace());
            });
        }
    });
}

fn diagnostics(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let diagnostics = app.bundle.diagnostics.as_slice();
    if diagnostics.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("Candidate has no diagnostics and may publish").color(palette::GOOD),
            );
        });
        return;
    }
    ui.label(format!("{} finding(s)", diagnostics.len()));
    let selected = clamp_index(app.selected_diagnostic, diagnostics.len());
    let click = table(
        ui,
        cx.state,
        "diagnostics",
        &[
            ("Severity", 80.0),
            ("Code", 220.0),
            ("Location", 220.0),
            ("Message", 520.0),
        ],
        diagnostics.len(),
        Some(selected),
        |index| {
            let diagnostic = &diagnostics[index];
            let color = if diagnostic.severity == Severity::Error {
                palette::BAD
            } else {
                palette::WARN
            };
            let mut message = diagnostic.message.clone();
            if let Some(help) = &diagnostic.help {
                message.push_str(" · help: ");
                message.push_str(help);
            }
            vec![
                Cell::Colored(format!("{:?}", diagnostic.severity), color),
                Cell::Colored(diagnostic.code.to_string(), color),
                diagnostic
                    .location
                    .as_ref()
                    .map_or_else(String::new, |location| {
                        format!(
                            "{}:{}:{}",
                            location.file.display(),
                            location.line,
                            location.column
                        )
                    })
                    .into(),
                message.into(),
            ]
        },
    );
    if let Some(widgets::RowClick::Single(index) | widgets::RowClick::Double(index)) = click {
        cx.select(selected, index);
    }
}

fn history(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    ui.horizontal_wrapped(|ui| {
        toolbar(
            ui,
            cx,
            &[
                (
                    "Diff",
                    Action::OpenHistoryDiff,
                    "Source diff of the selected entry · d",
                ),
                (
                    "Restore…",
                    Action::OpenRestore,
                    "Restore the selected entry as a new entry · R",
                ),
            ],
        );
        ui.label(
            RichText::new(format!(
                "Append-only Project History · {} entries",
                app.history.len()
            ))
            .color(palette::MUTED),
        );
    });
    let selected = clamp_index(app.selected_history, app.history.len());
    let click = table(
        ui,
        cx.state,
        "history",
        &[
            ("", 24.0),
            ("Entry", 150.0),
            ("Origin", 80.0),
            ("Actor", 130.0),
            ("Summary", 480.0),
            ("Files", 50.0),
            ("Created UTC", 200.0),
        ],
        app.history.len(),
        Some(selected),
        |index| {
            let entry = &app.history[index];
            vec![
                if app.active_entry.as_ref() == Some(&entry.id) {
                    Cell::Colored("✔".to_owned(), palette::GOOD)
                } else {
                    "".into()
                },
                entry.id.clone().into(),
                format!("{:?}", entry.origin.kind).to_lowercase().into(),
                entry.origin.name.clone().into(),
                entry.summary.clone().into(),
                entry.changes.len().to_string().into(),
                entry.created_at.clone().into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

/// Pointer help shown above the shared keyboard help.
const POINTER_HELP: &[&str] = &[
    "Click a view tab, table row, or toolbar button; double-click a row for its exact edit",
    "Timeline: click places the cursor and selects a Track · drag selects a range",
    "Timeline: Ctrl-scroll zooms · Shift-scroll or sideways scroll pans · right-click for more",
    "Timeline: double-click a Clip to open its Pattern in the Event Grid",
    "Instrument views: press the Note Audition keys below the view while playback runs",
    "Every keyboard command below works in this window too; closing the window exits",
];

fn help(ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let sections =
                std::iter::once(("POINTER", POINTER_HELP)).chain(keymap::HELP.iter().copied());
            for (heading, lines) in sections {
                ui.add_space(8.0);
                ui.label(RichText::new(heading).strong().color(palette::ACCENT));
                for line in lines {
                    ui.label(widgets::legible(line));
                }
            }
        });
}
