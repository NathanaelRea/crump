//! Conversion from window key events to the shared, client-neutral keymap.

use eframe::egui::{Event, Key, Modifiers, RawInput};

use super::{Modal, Workspace};
use crate::workspace::keymap::{KeyCode, KeyInput, KeyModifiers, KeyPhase, Keymap};

/// Takes this frame's keyboard events out of the window input and applies them to the workspace.
///
/// The keyboard belongs to the keymap, so egui never sees these events and cannot also move
/// widget focus or press a focused button. The one exception is an open input modal, whose text
/// field keeps the keyboard: only Enter and Esc are taken, and Enter is reported through
/// `modal_submit` so the text field can apply the frame's typing first.
pub(super) fn take_keys(
    raw_input: &mut RawInput,
    app: &mut Workspace,
    keymap: &mut Keymap,
    modal_submit: &mut bool,
    inline_text_focused: bool,
) {
    let alt_chord = raw_input.events.iter().any(|event| {
        matches!(event, Event::Key { pressed: true, modifiers, .. } if modifiers.alt && !modifiers.ctrl)
    });
    raw_input.events.retain(|event| {
        if matches!(app.modal, Some(Modal::Input { .. })) {
            return match event {
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                } => {
                    *modal_submit = true;
                    false
                }
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    keymap.handle(app, press(KeyCode::Esc, KeyModifiers::NONE));
                    false
                }
                _ => true,
            };
        }
        if inline_text_focused {
            return true;
        }
        match event {
            Event::Text(text) => {
                if !alt_chord {
                    for character in text.chars() {
                        keymap.handle(app, press(KeyCode::Char(character), KeyModifiers::NONE));
                    }
                }
                false
            }
            Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } => {
                if let Some(input) = key_input(*key, *pressed, *modifiers) {
                    keymap.handle(app, input);
                }
                false
            }
            // The window system reports Ctrl-C as a copy request rather than as a key.
            Event::Copy => {
                keymap.handle(
                    app,
                    press(
                        KeyCode::Char('c'),
                        KeyModifiers {
                            control: true,
                            ..KeyModifiers::NONE
                        },
                    ),
                );
                false
            }
            Event::Cut | Event::Paste(_) => false,
            _ => true,
        }
    });
}

const fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyInput {
    KeyInput {
        code,
        modifiers,
        phase: KeyPhase::Press,
    }
}

/// Translates one window key event, or `None` when a text event already carries it.
///
/// Printable keys arrive twice: as a layout-aware text event and as a key event. The text event
/// is authoritative for plain typing, so the key event is used only for keys that produce no
/// text, for Ctrl and Alt chords, and for releases, which end Note Audition notes.
pub(super) fn key_input(key: Key, pressed: bool, modifiers: Modifiers) -> Option<KeyInput> {
    let chord = modifiers.ctrl || modifiers.alt || modifiers.command;
    let code = match key {
        Key::Enter => KeyCode::Enter,
        Key::Escape => KeyCode::Esc,
        Key::Tab if modifiers.shift => KeyCode::BackTab,
        Key::Tab => KeyCode::Tab,
        Key::Backspace => KeyCode::Backspace,
        Key::ArrowUp => KeyCode::Up,
        Key::ArrowDown => KeyCode::Down,
        Key::ArrowLeft => KeyCode::Left,
        Key::ArrowRight => KeyCode::Right,
        Key::Home => KeyCode::Home,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::F12 => KeyCode::F(12),
        Key::Space => return None,
        _ => {
            let mut name = key.symbol_or_name().chars();
            match (name.next(), name.next()) {
                (Some(character), None) if chord || !pressed => {
                    KeyCode::Char(character.to_ascii_lowercase())
                }
                (Some(_), None) => return None,
                _ => KeyCode::Other,
            }
        }
    };
    if !pressed && !matches!(code, KeyCode::Char(_)) {
        return None;
    }
    Some(KeyInput {
        code,
        modifiers: KeyModifiers {
            control: modifiers.ctrl || modifiers.command,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        phase: if pressed {
            KeyPhase::Press
        } else {
            KeyPhase::Release
        },
    })
}
