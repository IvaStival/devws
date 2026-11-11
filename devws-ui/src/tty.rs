//! The terminal, as fzf uses it: drawn on and read from /dev/tty, so stdin
//! can bring the choices and stdout carry the answer. The cursor stays
//! hidden; the alternate screen leaves a pane as it was once we are done.
//! Everything is put back on the way out, panics included (the guard drops
//! while unwinding).

use crate::input::{self, Key};
use crate::view::Geometry;
use anyhow::{Context, Result, bail};
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
use std::os::fd::AsRawFd;
use std::time::Duration;

/// How long a wait for a key lasts before the terminal is asked whether it is
/// still there.
const HANGUP_CHECK: Duration = Duration::from_secs(1);

/// Whether the terminal hung up: its popup or pane closed under us. crossterm
/// cannot tell; its read waits on such a terminal forever, a core busy, as it
/// keeps saying there is something to read and gives nothing.
fn hung_up(tty: &File) -> bool {
    let mut watched = libc::pollfd {
        fd: tty.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, valid for the call; a zero timeout never blocks.
    let ready = unsafe { libc::poll(&mut watched, 1, 0) };
    ready > 0 && says_hung_up(watched.revents)
}

/// Only a hang-up says the terminal is gone. macOS answers "invalid"
/// (POLLNVAL) for /dev/tty however the terminal is, so taking that for a
/// hang-up closed every menu after a second without a key.
fn says_hung_up(revents: libc::c_short) -> bool {
    revents & libc::POLLHUP != 0
}

/// The terminal's own device (/dev/ttys012), which poll() can watch, from the
/// first of stderr, stdout and stdin that is one; /dev/tty when none is.
fn terminal_to_watch(tty: &File) -> Result<File> {
    for fd in [libc::STDERR_FILENO, libc::STDOUT_FILENO, libc::STDIN_FILENO] {
        // SAFETY: isatty only reads the descriptor number.
        if unsafe { libc::isatty(fd) } != 1 {
            continue;
        }
        let Ok(name) = tty_name(fd) else { continue };
        if name == "/dev/tty" {
            continue;
        }
        if let Ok(device) = File::open(&name) {
            return Ok(device);
        }
    }
    tty.try_clone().context("opening /dev/tty")
}

/// ttyname(3) for <fd>.
fn tty_name(fd: libc::c_int) -> std::io::Result<String> {
    let mut name = [0 as libc::c_char; 256];
    // SAFETY: the buffer and its length are valid for the call.
    let failed = unsafe { libc::ttyname_r(fd, name.as_mut_ptr(), name.len()) };
    if failed != 0 {
        return Err(std::io::Error::from_raw_os_error(failed));
    }
    // SAFETY: ttyname_r wrote a NUL-terminated name into the buffer.
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
    Ok(name.to_string_lossy().into_owned())
}

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
    let watched = terminal_to_watch(&guard.tty)?;
    let mut geometry = Geometry::default();
    let mut redraw = true;
    loop {
        if redraw {
            terminal.draw(|frame| {
                let area = frame.area();
                geometry = draw(state, area, frame.buffer_mut());
            })?;
        }
        if !event::poll(HANGUP_CHECK)? {
            // No key for a while: nothing to draw, unless the terminal is gone.
            if hung_up(&watched) {
                bail!("the terminal hung up");
            }
            redraw = false;
            continue;
        }
        redraw = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;

    /// A pseudo-terminal: the side a program reads (a popup's) and the side
    /// that drives it (tmux's).
    fn pty() -> (File, File) {
        let (mut driver, mut terminal) = (0, 0);
        // SAFETY: openpty writes the two descriptors; no name, settings or size.
        let made = unsafe {
            libc::openpty(
                &mut driver,
                &mut terminal,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(made, 0, "openpty failed");
        // SAFETY: both were just opened and nothing else owns them.
        unsafe { (File::from_raw_fd(driver), File::from_raw_fd(terminal)) }
    }

    #[test]
    fn a_terminal_whose_popup_closed_has_hung_up_and_an_open_one_has_not() {
        let (driver, terminal) = pty();
        assert!(!hung_up(&terminal), "the popup is open");
        drop(driver);
        assert!(hung_up(&terminal), "the popup closed");
    }

    #[test]
    fn only_a_hang_up_says_the_terminal_is_gone() {
        assert!(!says_hung_up(libc::POLLNVAL), "macOS's /dev/tty, open");
        assert!(!says_hung_up(0), "nothing to read");
        assert!(!says_hung_up(libc::POLLIN), "a key");
        assert!(says_hung_up(libc::POLLIN | libc::POLLHUP), "closed");
        assert!(says_hung_up(libc::POLLHUP | libc::POLLNVAL), "closed");
    }

    #[test]
    fn the_watched_terminal_is_the_device_and_reads_open() {
        let (_driver, terminal) = pty();
        let watched = terminal_to_watch(&terminal).expect("a terminal to watch");
        assert!(!hung_up(&watched), "an open terminal is watched as open");
    }
}
