//! Ratatui terminal client over the client-independent [`Workspace`] session.
//!
//! This is the only part of the workspace that depends on a terminal library. It translates
//! key and mouse events into [`Action`] values, draws the session, and owns the event loop.

use std::{
    io::Stdout,
    process::Command,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Tabs, Wrap,
    },
};

use super::*;
use crate::diagnostic::Severity;

mod keymap;
mod rendering;
mod terminal;

use keymap::Keymap;
use rendering::render;
pub use terminal::run;

const TICK_RATE: Duration = Duration::from_millis(40);
const BACKGROUND_POLL_RATE: Duration = Duration::from_millis(5);
const REFRESH_RATE: Duration = Duration::from_millis(400);

#[cfg(test)]
mod tests;
