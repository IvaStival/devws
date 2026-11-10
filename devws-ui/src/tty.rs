//! The terminal, as fzf uses it: drawn on and read from /dev/tty, so stdin
//! can bring the choices and stdout carry the answer. The cursor stays
//! hidden; the alternate screen leaves a pane as it was once we are done.
//! Everything is put back on the way out, panics included (the guard drops
//! while unwinding).

use crate::input::{self, Key};
use crate::view::Geometry;
use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::Rect;
use std::fs::{File, OpenOptions};

struct Guard {
    tty: File,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = execute!(self.tty, DisableMouseCapture, LeaveAlternateScreen, Show);
        let _ = disable_raw_mode();
    }
}

/// Runs a screen until `press` answers: `draw` paints the state and says
/// where its list is; clicks on the first row go to `ClickTab` when
/// `tabs_on_top`, clicks on the list to `Click`, the wheel moves.
pub fn run<State, Answer>(
    state: &mut State,
    tabs_on_top: bool,
    mut draw: impl FnMut(&State, Rect, &mut Buffer) -> Geometry,
    mut press: impl FnMut(&mut State, Key) -> Option<Answer>,
) -> Result<Answer> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("opening /dev/tty")?;
    let mut guard = Guard {
        tty: tty.try_clone().context("opening /dev/tty")?,
    };
    enable_raw_mode().context("raw mode on /dev/tty")?;
    execute!(guard.tty, EnterAlternateScreen, EnableMouseCapture, Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(tty))?;
    let mut geometry = Geometry::default();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            geometry = draw(state, area, frame.buffer_mut());
        })?;
        let key = match event::read()? {
            Event::Key(key) => input::from_key(key),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if tabs_on_top && mouse.row == 0 => {
                    Key::ClickTab(mouse.column)
                }
                MouseEventKind::Down(MouseButton::Left) => geometry
                    .row_at(mouse.row)
                    .map(Key::Click)
                    .unwrap_or(Key::None),
                MouseEventKind::ScrollUp => Key::Up,
                MouseEventKind::ScrollDown => Key::Down,
                _ => Key::None,
            },
            // A resize just draws again.
            _ => Key::None,
        };
        if let Some(answer) = press(state, key) {
            drop(terminal);
            drop(guard);
            return Ok(answer);
        }
    }
}
