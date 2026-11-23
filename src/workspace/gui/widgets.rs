//! Widgets shared by the views: selectable tables, action toolbars, and the Note Audition strip.

use egui_extras::{Column, TableBuilder};

use super::*;
use crate::workspace::keymap::{DRUM_NOTES, KEYBOARD_NOTES};

/// Workspace text with the symbols egui's bundled fonts lack replaced by ones they have.
///
/// Status lines, prompts, and the shared keyboard help are written for both clients and use a
/// few arrows and marks that only a terminal font is sure to carry.
pub(super) fn legible(text: &str) -> std::borrow::Cow<'_, str> {
    const SUBSTITUTES: [(char, char); 6] = [
        ('←', '⬅'),
        ('→', '➡'),
        ('↑', '⬆'),
        ('↓', '⬇'),
        ('●', '⏺'),
        ('│', '|'),
    ];
    if !text.contains(|character| SUBSTITUTES.iter().any(|(from, _)| *from == character)) {
        return text.into();
    }
    text.chars()
        .map(|character| {
            SUBSTITUTES
                .iter()
                .find_map(|(from, to)| (*from == character).then_some(*to))
                .unwrap_or(character)
        })
        .collect::<String>()
        .into()
}

/// One table cell.
pub(super) enum Cell {
    Text(String),
    Colored(String, Color32),
    /// A level from zero to one, drawn as a bar with its value.
    Meter(f32),
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

/// A level from zero to one as a bar followed by its value; the bar turns red near full scale.
pub(super) fn meter(ui: &mut Ui, level: f32, width: f32) -> egui::Response {
    let bar_width = (width - 46.0).max(12.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, 8.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        let mut filled = rect;
        filled.set_width(rect.width() * level.clamp(0.0, 1.0));
        painter.rect_filled(
            filled,
            2.0,
            if level > 0.95 {
                palette::BAD
            } else {
                palette::GOOD
            },
        );
        ui.label(
            RichText::new(format!("{level:.3}"))
                .monospace()
                .small()
                .color(palette::MUTED),
        );
    })
    .response
}

/// A pointer click on a table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowClick {
    Single(usize),
    Double(usize),
}

/// A selectable, virtually scrolled table. `columns` gives each title and initial width; the last
/// column takes the remaining width. `row` is called only for visible rows.
pub(super) fn table(
    ui: &mut Ui,
    state: &mut UiState,
    id: &'static str,
    columns: &[(&str, f32)],
    rows: usize,
    selected: Option<usize>,
    mut row: impl FnMut(usize) -> Vec<Cell>,
) -> Option<RowClick> {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let mut builder = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .auto_shrink([false, false])
        .cell_layout(Layout::left_to_right(Align::Center));
    for (index, (_, width)) in columns.iter().enumerate() {
        builder = builder.column(if index + 1 == columns.len() {
            Column::remainder().clip(true)
        } else {
            Column::initial(*width).at_least(24.0).clip(true)
        });
    }
    if let Some(selected) = selected.filter(|selected| *selected < rows)
        && state.scrolled.insert(id, selected) != Some(selected)
    {
        builder = builder.scroll_to_row(selected, None);
    }
    let mut click = None;
    builder
        .header(row_height, |mut header| {
            for (title, _) in columns {
                header.col(|ui| {
                    ui.label(RichText::new(*title).strong().color(palette::ACCENT));
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows, |mut table_row| {
                let index = table_row.index();
                table_row.set_selected(selected == Some(index));
                for cell in row(index) {
                    table_row.col(|ui| match cell {
                        Cell::Text(text) => {
                            ui.label(text);
                        }
                        Cell::Colored(text, color) => {
                            ui.label(RichText::new(text).color(color));
                        }
                        Cell::Meter(level) => {
                            meter(ui, level, ui.available_width().min(160.0));
                        }
                    });
                }
                let response = table_row.response();
                if response.double_clicked() {
                    click = Some(RowClick::Double(index));
                } else if response.clicked() {
                    click = Some(RowClick::Single(index));
                }
            });
        });
    click
}

/// Applies a row click: select the row, and open its exact edit on a double click.
pub(super) fn apply_row_click(cx: &mut Cx<'_>, click: Option<RowClick>, current: usize) {
    match click {
        Some(RowClick::Single(index)) => cx.select(current, index),
        Some(RowClick::Double(index)) => {
            cx.select(current, index);
            cx.send(Action::OpenExactEdit);
        }
        None => {}
    }
}

/// A row of buttons, each sending one action. The third field is the hover text, by convention
/// what the button does and its key.
pub(super) fn toolbar(ui: &mut Ui, cx: &mut Cx<'_>, buttons: &[(&str, Action, &str)]) {
    for (label, action, hint) in buttons {
        if ui.button(*label).on_hover_text(legible(hint)).clicked() {
            cx.send(action.clone());
        }
    }
}

/// A drop-down over a view's secondary context, such as its Pattern or Instrument. `step` turns
/// a signed distance from the current entry into the action that moves there.
pub(super) fn context_combo(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    names: &[String],
    current: usize,
    step: impl Fn(isize) -> Action,
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(names.get(current).map_or("—", String::as_str))
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                if ui.selectable_label(index == current, name).clicked() && index != current {
                    cx.send(step(index as isize - current as isize));
                }
            }
        });
}

/// A centred message for a view that has nothing to show.
pub(super) fn unavailable(ui: &mut Ui, message: &str) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(message).color(palette::MUTED));
    });
}

/// Clickable Note Audition keys for the selected Instrument.
pub(super) fn note_strip(ui: &mut Ui, cx: &mut Cx<'_>) {
    const DRUM_NAMES: [&str; 4] = ["Kick", "Snare", "Hat", "Clap"];
    let app = cx.app;
    let drums = app.selected_instrument_is_drums();
    let notes: &[(char, u8)] = if drums {
        &DRUM_NOTES[..DRUM_NAMES.len()]
    } else {
        &KEYBOARD_NOTES
    };
    let mut pressed = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Note Audition").strong())
            .on_hover_text("Transient only · needs playback running · i enters keyboard NOTE mode");
        for (index, (key, pitch)) in notes.iter().enumerate() {
            let name = if drums {
                DRUM_NAMES[index].to_owned()
            } else {
                crate::model::format_pitch(*pitch)
            };
            let sharp = name.contains('#');
            let held = app.held_notes.contains_key(key);
            let text =
                RichText::new(format!("{name}\n{}", key.to_ascii_uppercase())).color(if sharp {
                    Color32::from_gray(220)
                } else {
                    Color32::from_gray(24)
                });
            let response = ui.add(
                egui::Button::new(text)
                    .min_size(Vec2::new(if drums { 64.0 } else { 44.0 }, 40.0))
                    .fill(if held {
                        palette::ACCENT
                    } else if sharp {
                        Color32::from_gray(48)
                    } else {
                        Color32::from_gray(214)
                    }),
            );
            if response.is_pointer_button_down_on() {
                pressed = Some((*key, *pitch));
            }
        }
    });
    ui.add_space(4.0);
    let held = cx.state.piano_key;
    if held != pressed.map(|(key, _)| key) {
        if let Some(key) = held {
            cx.send(Action::NoteOff { key });
        }
        if let Some((key, pitch)) = pressed {
            cx.send(Action::NoteOn { key, pitch });
        }
        cx.state.piano_key = pressed.map(|(key, _)| key);
    }
}
