//! Keyboard shortcuts of the desktop window, in the conventions of desktop audio apps: Space
//! plays, Ctrl-Z undoes, Ctrl-S publishes, Ctrl-1–6 switch pages, arrows select and step, and
//! the computer keyboard plays notes once it is switched on.
//!
//! The window takes every key event before egui sees it (see [`take_keys`]), so a shortcut never
//! also moves widget focus or presses a focused button.

use eframe::egui::{Event, Key, Modifiers, RawInput};

use super::{Modal, MoveDirection, TapPoint, TimelineAction, UiState, View, Workspace};
use crate::workspace::Action;

/// Shortcut help as titled sections of (keys, what they do), for the Help page.
pub(super) const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "General",
        &[
            ("Ctrl-Z", "Undo the last change"),
            ("Ctrl-S", "Publish the Candidate"),
            ("Ctrl-R / F5", "Reload the Project from disk"),
            ("Ctrl-Q", "Quit"),
            (
                "Esc",
                "Cancel a dialog, a range selection or the note keyboard",
            ),
            ("F1 / ?", "Show this help"),
            ("F12", "Toggle performance readouts"),
        ],
    ),
    (
        "Pages",
        &[
            ("Ctrl-1", "Arrange"),
            (
                "Ctrl-2",
                "Mix (console or FX rack, whichever was last open)",
            ),
            ("Ctrl-3", "Edit (piano roll or automation)"),
            ("Ctrl-4", "Sound (faceplate, synth, parameters or graph)"),
            ("Ctrl-5", "Source"),
            ("Ctrl-6", "History"),
        ],
    ),
    (
        "Transport",
        &[
            ("Space", "Play / pause (in History, play the version shown)"),
            ("Shift-Space", "Stop"),
            ("Home / End", "Go to the start / end of the Song"),
            ("L", "Loop on / off"),
        ],
    ),
    (
        "Selection and values",
        &[
            ("Up / Down", "Select the previous / next row"),
            ("Page Up / Down", "Move the selection by eight rows"),
            ("Left / Right", "Step the selected value down / up"),
            ("Enter", "Type an exact value for the selection"),
            ("[ / ]", "Previous / next Pattern, Instrument or lane"),
            ("Delete", "Delete the selected note or automation point"),
        ],
    ),
    (
        "Arrange",
        &[
            ("Left / Right", "Move the cursor one grid step"),
            ("Ctrl-Left / Right", "Jump to the previous / next Clip edge"),
            ("Shift-Left / Right", "Select a range from the cursor"),
            ("Enter", "Play the selected range once"),
            ("L", "Loop the selected range (or loop on / off)"),
            ("[ / ]", "Coarser / finer grid"),
            ("+ / -", "Zoom in / out"),
            ("Ctrl-0", "Zoom to fit the Song"),
            ("Ctrl-G", "Go to an exact position"),
            ("Ctrl-F", "Find a Clip or Pattern"),
            ("F", "Follow the playhead on / off"),
            ("M", "Add a marker at the cursor"),
        ],
    ),
    (
        "Mix",
        &[
            ("M / S", "Mute / solo the selected channel"),
            ("C", "Cue the selected channel"),
            ("B", "Bypass the selected effect while listening"),
            ("Shift-B", "Bypass the selected effect in the Project"),
            ("Alt-Up / Down", "Move the selected effect earlier / later"),
        ],
    ),
    (
        "Sound",
        &[
            ("M", "Computer keyboard plays notes on / off"),
            (
                "A W S E D F T G Y H U J K",
                "Play C to C (pitched instruments)",
            ),
            ("A S D F", "Kick, snare, hat, clap (drum kits)"),
        ],
    ),
    (
        "History",
        &[
            ("Left / Right", "Older / newer version"),
            ("Space", "Play the version shown"),
            ("Ctrl-Z", "Undo the last change"),
        ],
    ),
    (
        "Source",
        &[
            ("Ctrl-E", "Open the file in your editor"),
            ("Home", "Scroll to the top"),
        ],
    ),
];

/// Views in which the computer keyboard can play notes.
pub(super) const NOTE_VIEWS: [View; 4] = [
    View::Faceplate,
    View::Instrument,
    View::Parameters,
    View::Graph,
];

/// Piano keys of the note keyboard for pitched Instruments, as (key, MIDI pitch) from C4 to C5.
pub(super) const KEYBOARD_NOTES: [(char, u8); 13] = [
    ('a', 60),
    ('w', 61),
    ('s', 62),
    ('e', 63),
    ('d', 64),
    ('f', 65),
    ('t', 66),
    ('g', 67),
    ('y', 68),
    ('h', 69),
    ('u', 70),
    ('j', 71),
    ('k', 72),
];

/// Pad keys of the note keyboard for stock drums, as (key, MIDI pitch): kick, snare, hat, clap.
pub(super) const DRUM_NOTES: [(char, u8); 8] = [
    ('a', 36),
    ('s', 38),
    ('d', 42),
    ('f', 39),
    ('z', 36),
    ('x', 38),
    ('c', 42),
    ('v', 39),
];

/// Takes this frame's keyboard events out of the window input and applies them as shortcuts.
///
/// An open input dialog keeps its text field: only Enter and Esc are taken, and Enter is
/// reported through `state.modal_submit` so the field applies the frame's typing first. A
/// focused inline text field (the Help filter, a parameter search) keeps every key.
pub(super) fn take_keys(
    raw_input: &mut RawInput,
    app: &mut Workspace,
    state: &mut UiState,
    inline_text_focused: bool,
) {
    let pages = [
        View::Timeline,
        state.last_mix_view,
        state.last_edit_view,
        state.last_sound_view,
        View::Source,
        View::History,
    ];
    raw_input.events.retain(|event| {
        if matches!(app.modal, Some(Modal::Input { .. })) {
            return match event {
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                } => {
                    state.modal_submit = true;
                    false
                }
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    app.dispatch(Action::ModalDismiss);
                    false
                }
                _ => true,
            };
        }
        if inline_text_focused {
            return true;
        }
        match event {
            Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
            } => {
                let press = KeyPress {
                    key: *key,
                    physical: physical_key.unwrap_or(*key),
                    pressed: *pressed,
                    repeat: *repeat,
                    modifiers: *modifiers,
                };
                for action in actions(app, &pages, press) {
                    app.dispatch(action);
                }
                false
            }
            // Keys arrive as key events; the text, copy and paste events they also produce
            // would otherwise reach a focused widget.
            Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_) => false,
            _ => true,
        }
    });
}

/// One key event as the shortcuts read it.
#[derive(Clone, Copy, Debug)]
pub(super) struct KeyPress {
    /// The key as the keyboard layout names it.
    pub(super) key: Key,
    /// The key at that position on a US layout, which places the note keyboard.
    pub(super) physical: Key,
    pub(super) pressed: bool,
    pub(super) repeat: bool,
    pub(super) modifiers: Modifiers,
}

impl KeyPress {
    /// A first press of `key` with `modifiers`.
    #[cfg(test)]
    pub(super) const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            physical: key,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
}

/// What a key event does in the current view; `pages` are the views Ctrl-1 to Ctrl-6 open.
pub(super) fn actions(app: &Workspace, pages: &[View; 6], press: KeyPress) -> Vec<Action> {
    let KeyPress {
        key,
        physical,
        pressed,
        repeat,
        modifiers,
    } = press;
    let note_keys = app.note_mode && NOTE_VIEWS.contains(&app.view);
    // A released key only ends the note it started.
    if !pressed {
        return note_keys
            .then(|| letter(physical))
            .flatten()
            .filter(|character| note_pitch(app, *character).is_some())
            .map(|key| Action::NoteOff { key })
            .into_iter()
            .collect();
    }
    if let Some(modal) = &app.modal {
        return modal_action(modal, key).into_iter().collect();
    }
    let ctrl = modifiers.command || modifiers.ctrl;
    if note_keys
        && !ctrl
        && !modifiers.alt
        && let Some(character) = letter(physical)
        && let Some(pitch) = note_pitch(app, character)
    {
        return if repeat {
            Vec::new()
        } else {
            vec![Action::NoteOn {
                key: character,
                pitch,
            }]
        };
    }
    let one = |action: Action| vec![action];
    if ctrl {
        return match key {
            Key::Z => one(Action::UndoHistory),
            Key::S => one(Action::OpenPublish),
            Key::R => one(Action::Reload),
            Key::Q => one(Action::Quit),
            Key::E if app.view == View::Source => one(Action::EditSource),
            Key::G if app.view == View::Timeline => one(TimelineAction::OpenGoto.into()),
            Key::F if app.view == View::Timeline => one(TimelineAction::OpenSearch.into()),
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 | Key::Num6 => {
                let index = usize::from(key.name().as_bytes()[0] - b'1');
                one(Action::ShowView(pages[index]))
            }
            Key::Num0 if app.view == View::Timeline => one(TimelineAction::Fit.into()),
            Key::Plus | Key::Equals if app.view == View::Timeline => {
                one(TimelineAction::Zoom(1).into())
            }
            Key::Minus if app.view == View::Timeline => one(TimelineAction::Zoom(-1).into()),
            Key::ArrowLeft if app.view == View::Timeline => {
                one(TimelineAction::MoveBoundary(-1).into())
            }
            Key::ArrowRight if app.view == View::Timeline => {
                one(TimelineAction::MoveBoundary(1).into())
            }
            _ => Vec::new(),
        };
    }
    let view = app.view;
    let timeline = view == View::Timeline;
    let range = timeline && app.timeline_state.visual();
    let step = |delta: isize| -> Vec<Action> {
        match view {
            View::Timeline if modifiers.shift && !range => vec![
                TimelineAction::BeginRange.into(),
                TimelineAction::MoveGrid(delta).into(),
            ],
            View::Timeline => one(TimelineAction::MoveGrid(delta).into()),
            // The rail runs oldest to newest left to right; the list is newest first.
            View::History => one(Action::MoveSelection(-delta)),
            _ => one(Action::AdjustSelected(delta)),
        }
    };
    match key {
        Key::Escape => one(Action::CancelMode),
        Key::F1 | Key::Questionmark => one(Action::ShowView(View::Help)),
        Key::F5 => one(Action::Reload),
        Key::F12 => one(Action::TogglePerformanceDiagnostics),
        Key::Space if modifiers.shift => one(Action::StopPlayback),
        Key::Space => one(play_action(app)),
        Key::Home if view == View::Source => one(Action::ScrollSourceToTop),
        Key::Home => one(TimelineAction::MoveSongEdge { end: false }.into()),
        Key::End => one(TimelineAction::MoveSongEdge { end: true }.into()),
        Key::ArrowUp if modifiers.alt && view == View::Processing => {
            one(Action::MoveSelectedProcessor(MoveDirection::Earlier))
        }
        Key::ArrowDown if modifiers.alt && view == View::Processing => {
            one(Action::MoveSelectedProcessor(MoveDirection::Later))
        }
        Key::ArrowUp => one(Action::MoveSelection(-1)),
        Key::ArrowDown => one(Action::MoveSelection(1)),
        Key::PageUp => one(Action::MoveSelection(-8)),
        Key::PageDown => one(Action::MoveSelection(8)),
        Key::ArrowLeft => step(-1),
        Key::ArrowRight => step(1),
        Key::Enter if range => one(TimelineAction::ApplyRange { looping: false }.into()),
        Key::Enter => one(Action::OpenExactEdit),
        Key::Delete | Key::Backspace => delete_action(app).into_iter().collect(),
        Key::OpenBracket if timeline => one(TimelineAction::ChangeGrid(-1).into()),
        Key::CloseBracket if timeline => one(TimelineAction::ChangeGrid(1).into()),
        Key::OpenBracket => one(Action::CycleContext(-1)),
        Key::CloseBracket => one(Action::CycleContext(1)),
        Key::Plus | Key::Equals if timeline => one(TimelineAction::Zoom(1).into()),
        Key::Minus if timeline => one(TimelineAction::Zoom(-1).into()),
        Key::L if range => one(TimelineAction::ApplyRange { looping: true }.into()),
        Key::L => one(Action::ToggleAuditionRangeLoop),
        Key::F if timeline => one(TimelineAction::ToggleFollow.into()),
        Key::M if timeline => one(TimelineAction::OpenLocator.into()),
        Key::M if view == View::Mixer => one(Action::ToggleMixerMute),
        Key::M if NOTE_VIEWS.contains(&view) && app.note_mode => one(Action::CancelMode),
        Key::M if NOTE_VIEWS.contains(&view) => one(Action::EnterNoteMode),
        Key::S if view == View::Mixer => one(Action::ToggleMixerSolo),
        Key::C if view == View::Mixer => one(Action::ToggleMixerCue(TapPoint::PostFader)),
        Key::B if view == View::Processing && modifiers.shift => {
            one(Action::ToggleDurableProcessorBypass)
        }
        Key::B if view == View::Processing => one(Action::ToggleTransientProcessorBypass),
        _ => Vec::new(),
    }
}

/// Space plays and pauses; in History, when nothing plays, it plays the version shown.
fn play_action(app: &Workspace) -> Action {
    let playing = app.player.is_some() || app.playback_requested;
    match app.history.get(app.selected_history) {
        Some(entry) if app.view == View::History && !playing => {
            Action::PlayHistoryVersion(entry.id.clone())
        }
        _ => Action::TogglePlayback,
    }
}

/// Delete removes the selected note or hit, or the selected automation point.
fn delete_action(app: &Workspace) -> Option<Action> {
    let song = &app.bundle.project.as_ref()?.song;
    match app.view {
        View::EventGrid => {
            let pattern = song.patterns.get(app.selected_pattern)?;
            (app.selected_event < pattern.events.len()).then(|| Action::DeletePatternEvent {
                pattern: pattern.id.clone(),
                event: app.selected_event,
            })
        }
        View::Automation => {
            let lane = song.automations.get(app.selected_automation)?;
            let point = lane.points.get(app.selected_automation_point)?;
            // A lane keeps its first point, as the point menu does.
            (lane.points.len() > 1 && point.at != crate::time::MusicalTime::ZERO).then(|| {
                Action::DeleteAutomationPoint {
                    lane: lane.id.clone(),
                    point: point.id.clone(),
                }
            })
        }
        _ => None,
    }
}

fn modal_action(modal: &Modal, key: Key) -> Option<Action> {
    match modal {
        Modal::Input { .. } => match key {
            Key::Escape => Some(Action::ModalDismiss),
            Key::Enter => Some(Action::ModalSubmit),
            _ => None,
        },
        Modal::ConfirmRestore { .. } => match key {
            Key::Enter | Key::Y => Some(Action::ModalSubmit),
            Key::Escape | Key::N => Some(Action::ModalDismiss),
            _ => None,
        },
        Modal::Text { .. } => match key {
            Key::Escape | Key::Enter => Some(Action::ModalDismiss),
            Key::ArrowUp => Some(Action::ModalScroll(-1)),
            Key::ArrowDown => Some(Action::ModalScroll(1)),
            Key::PageUp => Some(Action::ModalScroll(-10)),
            Key::PageDown => Some(Action::ModalScroll(10)),
            _ => None,
        },
    }
}

/// The lowercase letter a letter key names.
fn letter(key: Key) -> Option<char> {
    let mut name = key.name().chars();
    match (name.next(), name.next()) {
        (Some(character), None) if character.is_ascii_alphabetic() => {
            Some(character.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The MIDI pitch a note-keyboard letter plays on the selected Instrument.
fn note_pitch(app: &Workspace, character: char) -> Option<u8> {
    let notes: &[(char, u8)] = if app.selected_instrument_is_drums() {
        &DRUM_NOTES
    } else {
        &KEYBOARD_NOTES
    };
    notes
        .iter()
        .find_map(|(key, pitch)| (*key == character).then_some(*pitch))
}
