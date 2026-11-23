//! Keyboard bindings shared by the client adapters.
//!
//! A client converts its own key events into [`KeyInput`]; [`Keymap`] translates those into
//! workspace [`Action`] values. Keeping one binding table means every client is keyboard-complete
//! in the same way. This module depends on no terminal or windowing library.

use super::*;

/// One client-neutral key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyCode {
    /// A printable character, already shifted (`G`, `?`, `{`).
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    PageUp,
    PageDown,
    /// A function key.
    F(u8),
    /// A key no binding uses.
    Other,
}

/// Modifier keys held with a [`KeyCode`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct KeyModifiers {
    pub(super) control: bool,
    pub(super) alt: bool,
    pub(super) shift: bool,
}

impl KeyModifiers {
    pub(super) const NONE: Self = Self {
        control: false,
        alt: false,
        shift: false,
    };
    pub(super) const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    pub(super) const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
}

/// Whether a key went down, auto-repeated, or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyPhase {
    Press,
    Repeat,
    Release,
}

/// One client-neutral key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyInput {
    pub(super) code: KeyCode,
    pub(super) modifiers: KeyModifiers,
    pub(super) phase: KeyPhase,
}

/// Keyboard help, as titled sections of lines, for a client's Help view.
pub(super) const HELP: &[(&str, &[&str])] = &[
    (
        "WORKSPACE",
        &[
            "1–9/0 views · gt/gT or Tab/Shift-Tab cycle · H History · ? Help",
            "j/k or ↑/↓ select · gg/G first/last · Ctrl-u/Ctrl-d move 4 rows",
            "Space start/pause/resume · S stop · P publish · Ctrl-C is the only exit",
            "Esc or Ctrl-[ cancels a modal/mode and returns to NORMAL; it never exits",
        ],
    ),
    (
        "TIMELINE MOTION",
        &[
            "h/l or </> grid step · w/b next/previous boundary · e Clip end · % matching edge",
            "^/$ first/last Track material · g0/g$ Song edges · */# matching Pattern Clip",
            "{/} previous/next Locator · [/] grid size · : exact jump · / search · n/N repeat",
            "ma set transient mark · 'a jump · '' return · Ctrl-o/Ctrl-i jump history",
        ],
    ),
    (
        "TIMELINE RANGE / VIEWPORT",
        &[
            "V RANGE mode · h/l or </> move edge · o swap edge · L loop · Enter one-shot",
            "gv restore prior range · zf fit range · zz center · zs/ze align cursor",
            "zh/zl or ,/. pan · Ctrl-b/Ctrl-f full viewport · +/- zoom · Home fit Song",
            "f follow playhead · L toggles armed loop · M creates a durable Locator",
        ],
    ),
    (
        "TIMELINE CONTEXT",
        &[
            "gd Pattern/Event Grid · gf source · gi Instrument · gm Mixer · gp Processing",
            "A arms selected Track · Enter exact level · p exact pan",
        ],
    ),
    (
        "OTHER PRODUCTION VIEWS",
        &[
            "Processing: A target · a path · F range · L loop · b/B bypass · K/J reorder",
            "Event Grid: A audition · t/u/v time/duration/velocity · x/f/Alt-g/p sample edits",
            "Faceplate/Parameters: ←/→ bounded step · Enter exact physical value",
            "Instrument: ←/→ section · Enter edit/source · b route bypass",
            "Automation: [/] lane · ←/→ value · Enter exact · t time · c curve",
            "Graph: ←/→ Instrument · Enter source · Source: ←/→ file · e external editor",
            "History: Enter/d diff · R restore as new entry",
        ],
    ),
    (
        "NOTE MODE",
        &[
            "Press i in Instrument views; A W S E D F T G Y H U J K plays C4–C5",
            "Esc releases held notes and returns to NORMAL; j/k navigate outside NOTE mode",
        ],
    ),
    (
        "SAFETY",
        &[
            "Cursor, range, marks, search, Solo/cue/bypass, and loops are transient Audition State",
            "M and other Production View edits are checked source edits recorded in History",
            "Invalid Candidate source never replaces the prior audible Active Revision",
        ],
    ),
];

/// Number of lines a client shows when it renders [`HELP`] with one blank line between sections.
pub(super) fn help_line_count() -> usize {
    HELP.iter().map(|(_, lines)| lines.len() + 1).sum::<usize>() + HELP.len() - 1
}

/// Views in which Note Audition is available.
pub(super) const NOTE_VIEWS: [View; 4] = [
    View::Faceplate,
    View::Instrument,
    View::Parameters,
    View::Graph,
];

/// A key that waits for a second key to complete a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    G,
    Z,
    SetMark,
    JumpMark,
}

/// Stateful key translator shared by the client adapters.
///
/// Bindings depend on the session's view and mode, which the keymap only reads; the one piece
/// of state it owns is a pending key prefix.
#[derive(Debug, Default)]
pub(super) struct Keymap {
    prefix: Option<Prefix>,
}

impl Keymap {
    /// Translates one key event and applies the resulting action, if any.
    pub(super) fn handle(&mut self, app: &mut Workspace, key: KeyInput) {
        if let Some(action) = self.translate(app, key) {
            app.dispatch(action);
        }
    }

    fn translate(&mut self, app: &Workspace, key: KeyInput) -> Option<Action> {
        let control = key.modifiers.control;
        if key.phase == KeyPhase::Release {
            return audition_action(app, key).flatten();
        }
        if control && matches!(key.code, KeyCode::Char('c')) {
            return Some(Action::Quit);
        }
        if control && matches!(key.code, KeyCode::Char('[')) {
            if app.modal.is_some() {
                return Some(Action::CancelModal);
            }
            self.prefix = None;
            return Some(Action::CancelMode);
        }
        if let Some(modal) = &app.modal {
            return modal_action(modal, key);
        }
        if key.code == KeyCode::Esc {
            self.prefix = None;
            return Some(Action::CancelMode);
        }
        if app.note_mode {
            return audition_action(app, key).unwrap_or_else(|| {
                Some(Action::SetStatus(
                    "NOTE mode · use piano keys or Esc to return to Normal mode".to_owned(),
                ))
            });
        }
        if let Some(prefix) = self.prefix.take() {
            return Some(prefixed_action(app, prefix, key));
        }
        if NOTE_VIEWS.contains(&app.view) && matches!(key.code, KeyCode::Char('i')) {
            return Some(Action::EnterNoteMode);
        }
        let timeline = app.view == View::Timeline;
        if control {
            match key.code {
                KeyCode::Char('o') => return Some(TimelineAction::JumpHistory(-1).into()),
                KeyCode::Char('i') => return Some(TimelineAction::JumpHistory(1).into()),
                KeyCode::Char('d') => return Some(Action::MoveSelection(4)),
                KeyCode::Char('u') => return Some(Action::MoveSelection(-4)),
                KeyCode::Char('b') if timeline => {
                    return Some(TimelineAction::PanWindow(-1).into());
                }
                KeyCode::Char('f') if timeline => {
                    return Some(TimelineAction::PanWindow(1).into());
                }
                _ => {}
            }
        }
        let range = timeline && app.timeline_state.visual();
        let action = match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => {
                Action::SetStatus("Ctrl-C is the only workspace exit shortcut".to_owned())
            }
            (KeyCode::Char('?'), _) => Action::ShowView(View::Help),
            (KeyCode::Char('H'), _) => Action::ShowView(View::History),
            (KeyCode::F(12), _) => Action::TogglePerformanceDiagnostics,
            (KeyCode::Tab, KeyModifiers::SHIFT) | (KeyCode::BackTab, _) => Action::CycleView(-1),
            (KeyCode::Tab, _) => Action::CycleView(1),
            (KeyCode::Char(character @ '1'..='9'), _) => {
                Action::ShowView(VIEWS[usize::from(character as u8 - b'1')])
            }
            (KeyCode::Char('0'), _) => Action::ShowView(VIEWS[9]),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => Action::MoveSelection(1),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => Action::MoveSelection(-1),
            (KeyCode::Char('G'), _) => Action::MoveSelectionToEdge { first: false },
            (KeyCode::PageUp, _) => Action::MoveSelection(-8),
            (KeyCode::PageDown, _) => Action::MoveSelection(8),
            (KeyCode::Char('g'), KeyModifiers::NONE) => return self.begin(Prefix::G),
            (KeyCode::Char('z'), _) if timeline => return self.begin(Prefix::Z),
            (KeyCode::Char('m'), _) if timeline => return self.begin(Prefix::SetMark),
            (KeyCode::Char('\''), _) if timeline => return self.begin(Prefix::JumpMark),
            (KeyCode::Char(':'), _) if timeline => TimelineAction::OpenGoto.into(),
            (KeyCode::Char('/'), _) if timeline => TimelineAction::OpenSearch.into(),
            (KeyCode::Char('n'), _) if timeline => TimelineAction::RepeatSearch(1).into(),
            (KeyCode::Char('N'), _) if timeline => TimelineAction::RepeatSearch(-1).into(),
            (KeyCode::Char('h') | KeyCode::Char('<'), _) if timeline => {
                TimelineAction::MoveGrid(-1).into()
            }
            (KeyCode::Char('l') | KeyCode::Char('>'), _) if timeline => {
                TimelineAction::MoveGrid(1).into()
            }
            (KeyCode::Char('w'), _) if timeline => TimelineAction::MoveBoundary(1).into(),
            (KeyCode::Char('b'), _) if timeline => TimelineAction::MoveBoundary(-1).into(),
            (KeyCode::Char('e'), _) if timeline => TimelineAction::MoveClipEnd.into(),
            (KeyCode::Char('%'), _) if timeline => TimelineAction::ToggleClipEdge.into(),
            (KeyCode::Char('^'), _) if timeline => {
                TimelineAction::MoveTrackEdge { end: false }.into()
            }
            (KeyCode::Char('$'), _) if timeline => {
                TimelineAction::MoveTrackEdge { end: true }.into()
            }
            (KeyCode::Char('*'), _) if timeline => TimelineAction::MoveMatchingPattern(1).into(),
            (KeyCode::Char('#'), _) if timeline => TimelineAction::MoveMatchingPattern(-1).into(),
            (KeyCode::Char('{'), _) if timeline => TimelineAction::MoveLocator(-1).into(),
            (KeyCode::Char('}'), _) if timeline => TimelineAction::MoveLocator(1).into(),
            (KeyCode::Char('['), _) if timeline => TimelineAction::ChangeGrid(-1).into(),
            (KeyCode::Char(']'), _) if timeline => TimelineAction::ChangeGrid(1).into(),
            (KeyCode::Char('V'), _) if timeline => TimelineAction::BeginRange.into(),
            (KeyCode::Char('o'), _) if range => TimelineAction::SwapRangeEdge.into(),
            (KeyCode::Char('L'), _) if range => TimelineAction::ApplyRange { looping: true }.into(),
            (KeyCode::Char('L'), _) if timeline => Action::ToggleAuditionRangeLoop,
            (KeyCode::Enter, _) if range => TimelineAction::ApplyRange { looping: false }.into(),
            (KeyCode::Char('f'), _) if timeline => TimelineAction::ToggleFollow.into(),
            (KeyCode::Char('M'), _) if timeline => TimelineAction::OpenLocator.into(),
            (KeyCode::Char('+') | KeyCode::Char('='), _) if timeline => {
                TimelineAction::Zoom(1).into()
            }
            (KeyCode::Char('-'), _) if timeline => TimelineAction::Zoom(-1).into(),
            (KeyCode::Char(','), _) if timeline => TimelineAction::Pan(-1).into(),
            (KeyCode::Char('.'), _) if timeline => TimelineAction::Pan(1).into(),
            (KeyCode::Home, _) if timeline => TimelineAction::Fit.into(),
            (KeyCode::Char('r'), _) => Action::Reload,
            (KeyCode::Char('P'), _) => Action::OpenPublish,
            (KeyCode::Char(' '), _) => Action::TogglePlayback,
            (KeyCode::Char('S'), _) => Action::StopPlayback,
            (KeyCode::Char('L'), _) if app.view == View::EventGrid => Action::TogglePatternLoop,
            (KeyCode::Char('A'), _) if matches!(app.view, View::Timeline | View::EventGrid) => {
                Action::ArmSelectedAudition
            }
            (KeyCode::Char('a'), _) if app.view == View::Processing => Action::CycleAuditionPath,
            (KeyCode::Char('A'), _) if app.view == View::Processing => Action::OpenAuditionTarget,
            (KeyCode::Char('F'), _) if app.view == View::Processing => Action::OpenAuditionRange,
            (KeyCode::Char('L'), _) if app.view == View::Processing => {
                Action::ToggleAuditionRangeLoop
            }
            (KeyCode::Char('b'), _) if app.view == View::Processing => {
                Action::ToggleTransientProcessorBypass
            }
            (KeyCode::Char('b'), _) if app.view == View::Instrument => {
                Action::ToggleInstrumentRouteBypass
            }
            (KeyCode::Char('B'), _) if app.view == View::Processing => {
                Action::ToggleDurableProcessorBypass
            }
            (KeyCode::Char('K'), _) if app.view == View::Processing => {
                Action::MoveSelectedProcessor(MoveDirection::Earlier)
            }
            (KeyCode::Char('J'), _) if app.view == View::Processing => {
                Action::MoveSelectedProcessor(MoveDirection::Later)
            }
            (KeyCode::Char('m'), _) if app.view == View::Mixer => Action::ToggleMixerMute,
            (KeyCode::Char('s'), _) if app.view == View::Mixer => Action::ToggleMixerSolo,
            (KeyCode::Char('c'), _) if app.view == View::Mixer => {
                Action::ToggleMixerCue(TapPoint::PostFader)
            }
            (KeyCode::Char('C'), _) if app.view == View::Mixer => {
                Action::ToggleMixerCue(TapPoint::PreFader)
            }
            (KeyCode::Char('t'), _) if app.view == View::EventGrid => Action::OpenEventTime,
            (KeyCode::Char('v'), _) if app.view == View::EventGrid => Action::OpenEventVelocity,
            (KeyCode::Char('u'), _) if app.view == View::EventGrid => Action::OpenNoteDuration,
            (KeyCode::Char('x'), _) if app.view == View::EventGrid => Action::OpenSampleRegion,
            (KeyCode::Char('f'), _) if app.view == View::EventGrid => Action::OpenSampleFade,
            (KeyCode::Char('g'), KeyModifiers::ALT) if app.view == View::EventGrid => {
                Action::OpenSampleGain
            }
            (KeyCode::Char('p'), _) if app.view == View::EventGrid => Action::OpenSamplePan,
            (KeyCode::Char('t'), _) if app.view == View::Automation => Action::OpenAutomationTime,
            (KeyCode::Char('c'), _) if app.view == View::Automation => Action::OpenAutomationCurve,
            (KeyCode::Char('['), _) => Action::CycleContext(-1),
            (KeyCode::Char(']'), _) => Action::CycleContext(1),
            (KeyCode::Left, _) => Action::AdjustSelected(-1),
            (KeyCode::Right, _) => Action::AdjustSelected(1),
            (KeyCode::Enter, _) => Action::OpenExactEdit,
            (KeyCode::Char('p'), _) if timeline => Action::OpenTrackPan,
            (KeyCode::Char('R'), _) if app.view == View::History => Action::OpenRestore,
            (KeyCode::Char('d'), _) if app.view == View::History => Action::OpenHistoryDiff,
            (KeyCode::Char('e'), _) if app.view == View::Source => Action::EditSource,
            (KeyCode::Home, _) if app.view == View::Source => Action::ScrollSourceToTop,
            _ => return None,
        };
        Some(action)
    }

    fn begin(&mut self, prefix: Prefix) -> Option<Action> {
        self.prefix = Some(prefix);
        None
    }
}

fn prefixed_action(app: &Workspace, prefix: Prefix, key: KeyInput) -> Action {
    let timeline = app.view == View::Timeline;
    let status = |message: &str| Action::SetStatus(message.to_owned());
    match prefix {
        Prefix::G => match key.code {
            KeyCode::Char('g') => Action::MoveSelectionToEdge { first: true },
            KeyCode::Char('t') => Action::CycleView(1),
            KeyCode::Char('T') => Action::CycleView(-1),
            KeyCode::Char('0') if timeline => TimelineAction::MoveSongEdge { end: false }.into(),
            KeyCode::Char('$') if timeline => TimelineAction::MoveSongEdge { end: true }.into(),
            KeyCode::Char('d') if timeline => TimelineAction::GoDefinition.into(),
            KeyCode::Char('f') if timeline => TimelineAction::GoSource.into(),
            KeyCode::Char('i') if timeline => TimelineAction::GoInstrument.into(),
            KeyCode::Char('m') if timeline => TimelineAction::GoMixer.into(),
            KeyCode::Char('p') if timeline => TimelineAction::GoProcessing.into(),
            KeyCode::Char('v') if timeline => TimelineAction::RestoreRange.into(),
            _ => status("Unknown g command"),
        },
        Prefix::Z if timeline => match key.code {
            KeyCode::Char('z') => TimelineAction::CenterCursor.into(),
            KeyCode::Char('s') => TimelineAction::AlignCursor { end: false }.into(),
            KeyCode::Char('e') => TimelineAction::AlignCursor { end: true }.into(),
            KeyCode::Char('h') => TimelineAction::Pan(-1).into(),
            KeyCode::Char('l') => TimelineAction::Pan(1).into(),
            KeyCode::Char('f') => TimelineAction::FitRange.into(),
            _ => status("Unknown z command"),
        },
        Prefix::SetMark if timeline => match key.code {
            KeyCode::Char(mark) if mark.is_ascii_alphabetic() => {
                TimelineAction::SetMark(mark).into()
            }
            _ => status("Timeline marks use letters a–z"),
        },
        Prefix::JumpMark if timeline => match key.code {
            KeyCode::Char(mark) if mark.is_ascii_alphabetic() || mark == '\'' => {
                TimelineAction::JumpMark(mark).into()
            }
            _ => status("Jump to a Timeline mark with 'a or return with ''"),
        },
        _ => status("Key prefix is unavailable in this view"),
    }
}

fn modal_action(modal: &Modal, key: KeyInput) -> Option<Action> {
    let control = key.modifiers.control;
    match modal {
        Modal::Input { .. } => match key.code {
            KeyCode::Esc => Some(Action::ModalDismiss),
            KeyCode::Enter => Some(Action::ModalSubmit),
            KeyCode::Backspace => Some(Action::ModalBackspace),
            KeyCode::Char('a') if control => Some(Action::ModalSelectAll),
            KeyCode::Char(character) if !control => Some(Action::ModalInsert(character)),
            _ => None,
        },
        Modal::ConfirmRestore { .. } => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => Some(Action::ModalSubmit),
            KeyCode::Esc | KeyCode::Char('n') => Some(Action::ModalDismiss),
            _ => None,
        },
        Modal::Text { .. } => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Some(Action::ModalDismiss),
            KeyCode::Up => Some(Action::ModalScroll(-1)),
            KeyCode::Down => Some(Action::ModalScroll(1)),
            KeyCode::PageUp => Some(Action::ModalScroll(-10)),
            KeyCode::PageDown => Some(Action::ModalScroll(10)),
            _ => None,
        },
    }
}

/// Translates a piano key in Note Audition mode.
///
/// The outer `None` means the key is not a piano key here; `Some(None)` means it is one but
/// this event (a key repeat) changes nothing.
fn audition_action(app: &Workspace, key: KeyInput) -> Option<Option<Action>> {
    if !app.note_mode || !NOTE_VIEWS.contains(&app.view) {
        return None;
    }
    let KeyCode::Char(character) = key.code else {
        return None;
    };
    let pitch = if app.selected_instrument_is_drums() {
        drum_pitch(character)
    } else {
        keyboard_pitch(character)
    }?;
    Some(match key.phase {
        KeyPhase::Press => Some(Action::NoteOn {
            key: character,
            pitch,
        }),
        KeyPhase::Release => Some(Action::NoteOff { key: character }),
        KeyPhase::Repeat => None,
    })
}

/// Piano keys of Note Audition for pitched Instruments, as (key, MIDI pitch) from C4 to C5.
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

/// Pad keys of Note Audition for stock drums, as (key, MIDI pitch): kick, snare, hat, clap.
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

fn drum_pitch(character: char) -> Option<u8> {
    note_pitch(&DRUM_NOTES, character)
}

fn keyboard_pitch(character: char) -> Option<u8> {
    note_pitch(&KEYBOARD_NOTES, character)
}

fn note_pitch(notes: &[(char, u8)], character: char) -> Option<u8> {
    let character = character.to_ascii_lowercase();
    notes
        .iter()
        .find_map(|(key, pitch)| (*key == character).then_some(*pitch))
}
