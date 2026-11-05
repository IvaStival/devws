//! `devagentics board <task>`: the board in a terminal, kept up to date, with
//! the dispatcher running beside it. `--once` prints it as text instead.

use crate::board::Board;
use crate::board_usage::UsageReader;
use crate::board_view;
use crate::devws::Devws;
use crate::home::Home;
use crate::roles::Roles;
use crate::task::Task;
use crate::watch::{self, Dispatcher};
use anyhow::Result;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
};
use ratatui::crossterm::execute;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often the board reads the team again.
const REFRESH: Duration = Duration::from_secs(2);
/// How often the dispatcher looks at the team.
const DISPATCH: Duration = Duration::from_secs(1);
/// How old a reading of an agent's usage may be.
const USAGE_EVERY: Duration = Duration::from_secs(30);
const KEY_WAIT: Duration = Duration::from_millis(250);
/// Rows one turn of the mouse wheel scrolls.
const WHEEL: u16 = 3;

/// A move through the board or the log.
enum Scroll {
    /// Towards the top of the board, the older lines of the log.
    Up(u16),
    Down(u16),
    Top,
    Bottom,
}

/// The new scroll offset. The board counts rows down from its top, the log
/// rows back from its newest line, so they move opposite ways.
fn scrolled(scroll: u16, max: u16, log: bool, by: Scroll) -> u16 {
    let next = match (by, log) {
        (Scroll::Up(rows), false) | (Scroll::Down(rows), true) => scroll.saturating_sub(rows),
        (Scroll::Down(rows), false) | (Scroll::Up(rows), true) => scroll.saturating_add(rows),
        (Scroll::Top, false) | (Scroll::Bottom, true) => 0,
        (Scroll::Bottom, false) | (Scroll::Top, true) => max,
    };
    next.min(max)
}

fn now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

/// The board as it is now. The roles are read again each time, so a role file
/// added while the team runs shows up.
fn load(task: &Task, devws: &dyn Devws, home: &Home, usage: &mut UsageReader) -> Result<Board> {
    let worktree = task
        .config()?
        .repos
        .first()
        .map(|repo| repo.worktree.clone());
    let roles = Roles::load(home, worktree.as_deref())?;
    Board::build(task, devws, &roles, usage.of_team(task, devws)?)
}

/// The board as text, for a script or a look: `height` rows, or as many as
/// the whole board takes.
pub fn once(
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
    width: u16,
    height: u16,
) -> Result<String> {
    let board = load(task, devws, home, &mut UsageReader::new(Duration::ZERO))?;
    let height = if height == 0 {
        (board_view::full_height(&board, width) + 1).max(board_view::COMPACT_BELOW)
    } else {
        height
    };
    Ok(board_view::plain(&board_view::screen(
        &board,
        width,
        height,
        0,
        &now(),
    )))
}

/// Dispatches until told to stop, unless another dispatcher already does.
fn dispatch(task: &Task, devws: &dyn Devws, stop: &AtomicBool, problem: &Mutex<Option<String>>) {
    let Ok(_lock) = watch::lock(task) else {
        return;
    };
    let mut dispatcher = Dispatcher::new(task, devws, watch::STALL_GRACE);
    while !stop.load(Ordering::Relaxed) {
        let outcome = dispatcher.tick().err().map(|error| format!("{error:#}"));
        if let Ok(mut slot) = problem.lock() {
            *slot = outcome;
        }
        let until = Instant::now() + DISPATCH;
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn tmux(arguments: &[&str]) {
    let program = std::env::var("DEVAGENTICS_TMUX").unwrap_or_else(|_| "tmux".to_string());
    let _ = Command::new(program).args(arguments).status();
}

pub fn run(task: &Task, devws: &dyn Devws, home: &Home) -> Result<()> {
    let stop = AtomicBool::new(false);
    let problem = Mutex::new(None);
    std::thread::scope(|scope| {
        scope.spawn(|| dispatch(task, devws, &stop, &problem));
        let result = show(task, devws, home, &problem);
        stop.store(true, Ordering::Relaxed);
        result
    })
}

fn show(
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
    problem: &Mutex<Option<String>>,
) -> Result<()> {
    let mut usage = UsageReader::new(USAGE_EVERY);
    let mut board = load(task, devws, home, &mut usage)?;
    let mut refreshed = Instant::now();
    let (mut scroll, mut log): (u16, bool) = (0, false);
    // `p` waits for the number of the agent to peek.
    let mut peeking = false;
    // The log's length at the last draw: scrolled back, the view stays on the
    // same lines as new ones come in.
    let mut log_length = 0;
    let mut terminal = ratatui::init();
    // The wheel comes to the board instead of putting tmux in copy mode.
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let result = (|| -> Result<()> {
        loop {
            let notice = problem.lock().ok().and_then(|slot| slot.clone());
            let lines = if log {
                task.log_lines().unwrap_or_default()
            } else {
                Vec::new()
            };
            if log && scroll > 0 && lines.len() > log_length {
                scroll = scroll.saturating_add((lines.len() - log_length) as u16);
            }
            log_length = lines.len();
            terminal.draw(|frame| {
                let area = frame.area();
                let screen = if log {
                    board_view::log_screen(
                        &lines,
                        area.width,
                        area.height,
                        scroll,
                        notice.as_deref(),
                    )
                } else {
                    board_view::screen(&board, area.width, area.height, scroll, &now())
                };
                for y in 0..area.height {
                    for x in 0..area.width {
                        frame.buffer_mut()[(x, y)] = screen[(x, y)].clone();
                    }
                }
            })?;
            if !event::poll(KEY_WAIT)? {
                refresh_when_due(&mut refreshed, task, devws, home, &mut usage, &mut board);
                continue;
            }
            let size = terminal.size()?;
            let page = size.height.saturating_sub(3).max(1);
            let max = if log {
                board_view::log_scroll_max(log_length, size.height)
            } else {
                board_view::board_scroll_max(&board, size.width, size.height)
            };
            let by = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc if peeking => {
                        peeking = false;
                        None
                    }
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('p') => {
                        peeking = true;
                        None
                    }
                    KeyCode::Char(digit @ '1'..='9') => {
                        if let Some(pane) = pane_of(&board, digit) {
                            let shown = if peeking {
                                devws.peek(&pane)
                            } else {
                                devws.go(&pane)
                            };
                            if let Err(error) = shown {
                                board.summary = format!("board: {error:#}");
                            }
                        }
                        peeking = false;
                        None
                    }
                    KeyCode::Char('r') => {
                        refreshed = Instant::now() - REFRESH;
                        None
                    }
                    KeyCode::Char('l') => {
                        (log, scroll) = (!log, 0);
                        None
                    }
                    KeyCode::Char('z') => {
                        tmux(&["resize-pane", "-Z"]);
                        None
                    }
                    KeyCode::Enter => {
                        tmux(&["select-pane", "-L"]);
                        None
                    }
                    KeyCode::Char('k') | KeyCode::Up => Some(Scroll::Up(1)),
                    KeyCode::Char('j') | KeyCode::Down => Some(Scroll::Down(1)),
                    KeyCode::PageUp | KeyCode::Char('b') => Some(Scroll::Up(page)),
                    KeyCode::PageDown | KeyCode::Char(' ') => Some(Scroll::Down(page)),
                    KeyCode::Char('g') | KeyCode::Home => Some(Scroll::Top),
                    KeyCode::Char('G') | KeyCode::End => Some(Scroll::Bottom),
                    _ => None,
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => Some(Scroll::Up(WHEEL)),
                    MouseEventKind::ScrollDown => Some(Scroll::Down(WHEEL)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(by) = by {
                scroll = scrolled(scroll, max, log, by);
            }
            refresh_when_due(&mut refreshed, task, devws, home, &mut usage, &mut board);
        }
    })();
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// The pane of the agent a number key names on the board.
fn pane_of(board: &Board, digit: char) -> Option<String> {
    let number = digit.to_digit(10)? as u8;
    board
        .roles
        .iter()
        .flat_map(|row| &row.agents)
        .find(|agent| agent.number == Some(number))
        .map(|agent| agent.pane.clone())
}

/// Reads the team again when it is time.
fn refresh_when_due(
    refreshed: &mut Instant,
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
    usage: &mut UsageReader,
    board: &mut Board,
) {
    if refreshed.elapsed() >= REFRESH {
        *refreshed = Instant::now();
        match load(task, devws, home, usage) {
            Ok(fresh) => *board = fresh,
            Err(error) => board.summary = format!("board: {error:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_goes_to_the_boards_top_and_to_the_logs_older_lines() {
        // The board: rows down from its top.
        assert_eq!(scrolled(5, 10, false, Scroll::Up(1)), 4);
        assert_eq!(
            scrolled(5, 10, false, Scroll::Down(9)),
            10,
            "stops at the end"
        );
        assert_eq!(scrolled(5, 10, false, Scroll::Top), 0);
        assert_eq!(scrolled(5, 10, false, Scroll::Bottom), 10);
        // The log: rows back from its newest line.
        assert_eq!(scrolled(5, 10, true, Scroll::Up(1)), 6);
        assert_eq!(
            scrolled(5, 10, true, Scroll::Down(9)),
            0,
            "stops at the newest"
        );
        assert_eq!(scrolled(5, 10, true, Scroll::Top), 10, "the first line");
        assert_eq!(scrolled(5, 10, true, Scroll::Bottom), 0);
    }
}
