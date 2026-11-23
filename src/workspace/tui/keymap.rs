//! Conversion from terminal key events to the shared, client-neutral keymap.

use super::*;
use crate::workspace::keymap::{self, KeyCode as Code, KeyInput, KeyModifiers as Mods, KeyPhase};

/// Key translator for the terminal client.
#[derive(Debug, Default)]
pub(super) struct Keymap(keymap::Keymap);

impl Keymap {
    /// Translates one terminal key event and applies the resulting action, if any.
    pub(super) fn handle(&mut self, app: &mut Workspace, key: KeyEvent) {
        self.0.handle(app, key_input(key));
    }
}

fn key_input(key: KeyEvent) -> KeyInput {
    let code = match key.code {
        KeyCode::Char(character) => Code::Char(character),
        KeyCode::Enter => Code::Enter,
        KeyCode::Esc => Code::Esc,
        KeyCode::Tab => Code::Tab,
        KeyCode::BackTab => Code::BackTab,
        KeyCode::Backspace => Code::Backspace,
        KeyCode::Up => Code::Up,
        KeyCode::Down => Code::Down,
        KeyCode::Left => Code::Left,
        KeyCode::Right => Code::Right,
        KeyCode::Home => Code::Home,
        KeyCode::PageUp => Code::PageUp,
        KeyCode::PageDown => Code::PageDown,
        KeyCode::F(number) => Code::F(number),
        _ => Code::Other,
    };
    KeyInput {
        code,
        modifiers: Mods {
            control: key.modifiers.contains(KeyModifiers::CONTROL),
            alt: key.modifiers.contains(KeyModifiers::ALT),
            shift: key.modifiers.contains(KeyModifiers::SHIFT),
        },
        phase: match key.kind {
            KeyEventKind::Press => KeyPhase::Press,
            KeyEventKind::Repeat => KeyPhase::Repeat,
            KeyEventKind::Release => KeyPhase::Release,
        },
    }
}
