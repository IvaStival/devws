//! `devagentics board <task>`: the board in a terminal, kept up to date, with
//! the dispatcher running beside it. `--once` prints it as text instead.

use crate::board::Board;
use crate::board_usage::UsageReader;
use crate::board_view::{self, Navigation, View};
use crate::devws::Devws;
use crate::home::Home;
use crate::leader;
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
/// How often the leader's account is looked at for a handover (leader.rs).
const LEADER_EVERY: Duration = Duration::from_secs(60);
const KEY_WAIT: Duration = Duration::from_millis(250);
/// Rows one turn of the mouse wheel scrolls.
const WHEEL: u16 = 3;

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
    view: View,
) -> Result<String> {
    let board = load(task, devws, home, &mut UsageReader::new(Duration::ZERO))?;
    let log = if view == View::Activity {
        task.log_lines()?
    } else {
        Vec::new()
    };
    let entries = board_view::entries(&board, view, false, &log, None);
    let mut nav = Navigation::new(view);
    nav.reconcile(&entries);
    // As tall as the whole list takes: the header, every row and the keys.
    let height = if height == 0 {
        let header = board_view::header_rows(&board, &nav, width, u16::MAX) as usize;
        (header + entries.len() + 1).clamp(8, u16::MAX as usize) as u16
    } else {
        height
    };
    Ok(board_view::plain(&board_view::screen(
        &board, &nav, &entries, width, height, None,
    )))
}

/// Dispatches until told to stop, unless another dispatcher already does;
/// hands the leader over when asked, and now and then when its account ran
/// out.
fn dispatch(
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
    stop: &AtomicBool,
    problem: &Mutex<Option<String>>,
) {
    let Ok(_lock) = watch::lock(task) else {
        return;
    };
    let mut dispatcher = Dispatcher::new(task, devws, watch::STALL_GRACE);
    let command = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "devagentics".to_string());
    let mut leader_seen: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let mut outcome = dispatcher.tick().err().map(|error| format!("{error:#}"));
        // A move that was asked for is made as soon as the leader's turn is
        // over; the accounts are read only now and then.
        let asked = leader::switch_pending(task).unwrap_or(false);
        if asked || leader_seen.is_none_or(|at| at.elapsed() >= LEADER_EVERY) {
            leader_seen = Some(Instant::now());
            if let Err(error) = leader::hand_over(home, task, devws, &command) {
                outcome.get_or_insert(format!("leader handover: {error:#}"));
            }
        }
        if let Ok(mut slot) = problem.lock() {
            *slot = outcome;
        }
        let until = Instant::now() + DISPATCH;
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn tmux(arguments: &[&str]) -> Result<String> {
    let program = std::env::var("DEVAGENTICS_TMUX").unwrap_or_else(|_| "tmux".to_string());
    let output = Command::new(program).args(arguments).output()?;
    anyhow::ensure!(
        output.status.success(),
        "tmux: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

/// A menu request targets this pane, never another dispatcher process.
fn take_view_request() -> Result<Option<View>> {
    let Ok(pane) = std::env::var("TMUX_PANE") else {
        return Ok(None);
    };
    let value = tmux(&[
        "show-options",
        "-p",
        "-qv",
        "-t",
        &pane,
        "@devagentics_board_view",
    ])?;
    if value.is_empty() {
        return Ok(None);
    }
    tmux(&[
        "set-option",
        "-p",
        "-u",
        "-t",
        &pane,
        "@devagentics_board_view",
    ])?;
    View::parse(&value)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("unknown board view: {value}"))
}

pub fn run(task: &Task, devws: &dyn Devws, home: &Home, view: View) -> Result<()> {
    let stop = AtomicBool::new(false);
    let problem = Mutex::new(None);
    std::thread::scope(|scope| {
        scope.spawn(|| dispatch(task, devws, home, &stop, &problem));
        let result = show(task, devws, home, &problem, view);
        stop.store(true, Ordering::Relaxed);
        result
    })
}

#[derive(Debug, PartialEq)]
enum Action {
    None,
    Quit,
    Refresh,
    Zoom,
    Leader,
    Visit { pane: String, peek: bool },
}

struct Interaction {
    nav: Navigation,
    peeking: bool,
    help: bool,
}

const HELP: &str = "Board navigation\n\nTab / Shift-Tab: next / previous view\nj/k or arrows: select row; scroll details\nPgUp/PgDn: move a page\ng/G or Home/End: first / last\nEnter: full details\nEsc: close details or cancel peek\no: go to selected agent\nv: peek selected agent\n1–9: go to numbered agent\np then 1–9: peek numbered agent\nL: return to leader\nl: toggle Activity\nc: toggle completed steps or inactive role catalog\nr: refresh\nz: zoom\n?: help\nq: close details, or quit board (dispatcher stops)";

impl Interaction {
    /// One event from the terminal: a key, or the wheel, which moves `WHEEL`
    /// rows a turn.
    fn apply(
        &mut self,
        event: Event,
        board: &Board,
        entries: &[board_view::Entry],
        page: u16,
    ) -> Action {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                self.key(key.code, board, entries, page)
            }
            Event::Mouse(mouse) => {
                let code = match mouse.kind {
                    MouseEventKind::ScrollUp => KeyCode::Up,
                    MouseEventKind::ScrollDown => KeyCode::Down,
                    _ => return Action::None,
                };
                for _ in 0..WHEEL {
                    self.key(code, board, entries, page);
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    fn new(view: View) -> Self {
        Self {
            nav: Navigation::new(view),
            peeking: false,
            help: false,
        }
    }

    fn key(
        &mut self,
        code: KeyCode,
        board: &Board,
        entries: &[board_view::Entry],
        page: u16,
    ) -> Action {
        match code {
            KeyCode::Esc if self.peeking => self.peeking = false,
            KeyCode::Esc | KeyCode::Char('q') if self.help || self.nav.detail => {
                self.help = false;
                self.nav.detail = false;
                self.nav.detail_scroll = 0;
            }
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc => {}
            KeyCode::Char('?') => {
                self.help = !self.help;
                self.nav.detail_scroll = 0;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.nav
                    .change_view(self.nav.view.next(code == KeyCode::BackTab));
                self.help = false;
                self.peeking = false;
            }
            KeyCode::Char('l') => {
                self.nav.change_view(if self.nav.view == View::Activity {
                    View::Overview
                } else {
                    View::Activity
                });
                self.help = false;
            }
            KeyCode::Char('c') if matches!(self.nav.view, View::Steps | View::Agents) => {
                self.nav.expanded = !self.nav.expanded;
                self.nav.detail = false;
            }
            KeyCode::Enter if !self.help && !entries.is_empty() => {
                self.nav.detail = true;
                self.nav.detail_scroll = 0;
            }
            KeyCode::Char('p') => self.peeking = true,
            KeyCode::Char(digit @ '1'..='9') => {
                let peek = self.peeking;
                self.peeking = false;
                if let Some(pane) = pane_of(board, digit) {
                    return Action::Visit { pane, peek };
                }
            }
            KeyCode::Char('o') | KeyCode::Char('v') if !self.help => {
                if let Some(pane) = entries
                    .get(self.nav.index)
                    .and_then(|entry| entry.pane.clone())
                {
                    return Action::Visit {
                        pane,
                        peek: code == KeyCode::Char('v'),
                    };
                }
            }
            KeyCode::Char('L') => return Action::Leader,
            KeyCode::Char('z') => return Action::Zoom,
            KeyCode::Char('r') => return Action::Refresh,
            KeyCode::Char('g') | KeyCode::Home | KeyCode::Char('G') | KeyCode::End => {
                let end = matches!(code, KeyCode::Char('G') | KeyCode::End);
                if self.nav.detail || self.help {
                    self.nav.detail_scroll = if end { u16::MAX } else { 0 };
                } else {
                    self.nav
                        .move_by(entries, if end { isize::MAX } else { isize::MIN });
                }
            }
            code => {
                let amount = match code {
                    KeyCode::Char('k') | KeyCode::Up => -1,
                    KeyCode::Char('j') | KeyCode::Down => 1,
                    KeyCode::PageUp | KeyCode::Char('b') => -(page as isize),
                    KeyCode::PageDown | KeyCode::Char(' ') => page as isize,
                    _ => 0,
                };
                if self.nav.detail || self.help {
                    self.nav.detail_scroll = self.nav.detail_scroll.saturating_add_signed(
                        amount.clamp(i16::MIN as isize, i16::MAX as isize) as i16,
                    );
                } else {
                    self.nav.move_by(entries, amount);
                }
            }
        }
        Action::None
    }
}

/// How often the loader looks whether it is time to read, or to stop.
const LOADER_TICK: Duration = Duration::from_millis(50);

/// The board's reading, on a thread of its own: reading a team takes seconds
/// (each agent's usage, each account's limits), and the keys and the wheel
/// must not wait for it. The screen takes each new reading when it is ready.
struct Loader {
    fresh: Mutex<Option<Result<Board, String>>>,
    now: AtomicBool,
    stop: AtomicBool,
}

impl Loader {
    fn new() -> Self {
        Self {
            fresh: Mutex::new(None),
            now: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        }
    }

    /// Reads with `read` every `every`, or at once when asked, until stopped.
    fn run(&self, every: Duration, mut read: impl FnMut() -> Result<Board>) {
        let mut last = Instant::now();
        while !self.stop.load(Ordering::Relaxed) {
            if last.elapsed() < every && !self.now.swap(false, Ordering::Relaxed) {
                std::thread::sleep(LOADER_TICK);
                continue;
            }
            last = Instant::now();
            let reading = read().map_err(|error| format!("Refresh failed: {error:#}"));
            if let Ok(mut fresh) = self.fresh.lock() {
                *fresh = Some(reading);
            }
        }
    }

    /// The reading that came since the last call, if one did.
    fn take(&self) -> Option<Result<Board, String>> {
        self.fresh.lock().ok()?.take()
    }

    fn refresh(&self) {
        self.now.store(true, Ordering::Relaxed);
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Unchanged histories are not read or reformatted on a board refresh.
#[derive(Default)]
struct ActivityReader {
    stamp: Option<(u64, std::time::SystemTime)>,
    loaded: bool,
    lines: Vec<String>,
}

impl ActivityReader {
    fn refresh(&mut self, task: &Task) -> Result<bool> {
        let stamp = match std::fs::metadata(task.dir().join("log.jsonl")) {
            Ok(meta) => Some((meta.len(), meta.modified()?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        self.update(stamp, || task.log_lines())
    }

    fn update(
        &mut self,
        stamp: Option<(u64, std::time::SystemTime)>,
        read: impl FnOnce() -> Result<Vec<String>>,
    ) -> Result<bool> {
        if self.loaded && self.stamp == stamp {
            return Ok(false);
        }
        let lines = read()?;
        self.lines = lines;
        self.stamp = stamp;
        self.loaded = true;
        Ok(true)
    }
}

fn show(
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
    problem: &Mutex<Option<String>>,
    view: View,
) -> Result<()> {
    let mut usage = UsageReader::new(USAGE_EVERY);
    // The first reading before the screen opens, so it never starts empty;
    // the loader reads every one after it.
    let board = load(task, devws, home, &mut usage)?;
    let loader = Loader::new();
    std::thread::scope(|scope| {
        scope.spawn(|| loader.run(REFRESH, || load(task, devws, home, &mut usage)));
        let result = screen_loop(task, devws, problem, view, board, &loader);
        loader.stop();
        result
    })
}

/// The screen: draws the latest reading and answers keys and the wheel; it
/// never reads the team itself (Loader does).
fn screen_loop(
    task: &Task,
    devws: &dyn Devws,
    problem: &Mutex<Option<String>>,
    view: View,
    mut board: Board,
    loader: &Loader,
) -> Result<()> {
    let mut refreshed = Instant::now();
    let mut ui = Interaction::new(view);
    let mut local_problem = None;
    let mut activity = ActivityReader::default();
    let mut entries = Vec::new();
    let mut entries_dirty = true;
    let mut entries_key = None;
    let help_entries = vec![board_view::Entry {
        id: "help".into(),
        label: "Help".into(),
        detail: HELP.into(),
        pane: None,
        attention: false,
        kind: board_view::Kind::Row,
        figures: String::new(),
        tail: String::new(),
    }];
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        execute!(std::io::stdout(), EnableMouseCapture)?;
        loop {
            match loader.take() {
                Some(Ok(fresh)) => {
                    if board != fresh && ui.nav.view != View::Activity {
                        entries_dirty = true;
                    }
                    board = fresh;
                    local_problem = None;
                }
                Some(Err(failure)) => local_problem = Some(failure),
                None => {}
            }
            let refresh_due = refreshed.elapsed() >= REFRESH;
            if refresh_due {
                refreshed = Instant::now();
                match take_view_request() {
                    Ok(Some(view)) => {
                        ui.nav = Navigation::new(view);
                        ui.help = false;
                        ui.peeking = false;
                    }
                    Ok(None) => {}
                    Err(error) => local_problem = Some(format!("Menu request failed: {error:#}")),
                }
            }
            let entering_activity =
                ui.nav.view == View::Activity
                    && entries_key.as_ref().is_none_or(
                        |previous: &(View, bool, Option<String>)| previous.0 != View::Activity,
                    );
            if ui.nav.view == View::Activity && (refresh_due || entering_activity) {
                match activity.refresh(task) {
                    Ok(changed) => entries_dirty |= changed,
                    Err(error) => local_problem = Some(format!("Activity failed: {error:#}")),
                }
            }
            let dispatcher_problem = problem.lock().ok().and_then(|slot| slot.clone());
            let notice = local_problem.as_deref().or(dispatcher_problem.as_deref());
            let key = (ui.nav.view, ui.nav.expanded, notice.map(str::to_string));
            if entries_dirty || entries_key.as_ref() != Some(&key) {
                entries = board_view::entries(
                    &board,
                    ui.nav.view,
                    ui.nav.expanded,
                    &activity.lines,
                    notice,
                );
                entries_key = Some(key);
                entries_dirty = false;
            }
            ui.nav.reconcile(&entries);
            let size = terminal.size()?;
            let detail = if ui.help {
                Some(HELP)
            } else if ui.nav.detail {
                entries.get(ui.nav.index).map(|entry| entry.detail.as_str())
            } else {
                None
            };
            if let Some(detail) = detail {
                let header = board_view::header_rows(&board, &ui.nav, size.width, size.height);
                let rows = ratatui::widgets::Paragraph::new(detail)
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .line_count(size.width.max(1));
                ui.nav.detail_scroll = ui.nav.detail_scroll.min(
                    rows.saturating_sub(size.height.saturating_sub(header + 1) as usize)
                        .min(u16::MAX as usize) as u16,
                );
            }
            terminal.draw(|frame| {
                let area = frame.area();
                let screen = if ui.help {
                    let mut help_nav = Navigation::new(ui.nav.view);
                    help_nav.detail = true;
                    help_nav.detail_scroll = ui.nav.detail_scroll;
                    board_view::screen(
                        &board,
                        &help_nav,
                        &help_entries,
                        area.width,
                        area.height,
                        notice,
                    )
                } else {
                    board_view::screen(&board, &ui.nav, &entries, area.width, area.height, notice)
                };
                *frame.buffer_mut() = screen;
                if ui.peeking && area.height > 0 {
                    use ratatui::widgets::Widget;
                    ratatui::text::Line::raw("Peek: choose 1–9 · Esc cancel").render(
                        ratatui::layout::Rect::new(0, area.height - 1, area.width, 1),
                        frame.buffer_mut(),
                    );
                }
            })?;
            if !event::poll(KEY_WAIT)? {
                continue;
            }
            let page = size.height.saturating_sub(6).max(1);
            // Every event already waiting, then one draw: a fast wheel turn
            // sends many, and drawing between each made it lag behind.
            let mut actions = Vec::new();
            loop {
                let action = ui.apply(event::read()?, &board, &entries, page);
                if action != Action::None {
                    actions.push(action);
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            for action in actions {
                let outcome = match action {
                    Action::None => Ok(()),
                    Action::Quit => return Ok(()),
                    Action::Refresh => {
                        loader.refresh();
                        refreshed = Instant::now() - REFRESH;
                        Ok(())
                    }
                    Action::Zoom => tmux(&["resize-pane", "-Z"]).map(|_| ()),
                    Action::Leader => board
                        .leader_pane
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Leader pane is unavailable"))
                        .and_then(|pane| devws.go(pane)),
                    Action::Visit { pane, peek } => {
                        if peek {
                            devws.peek(&pane)
                        } else {
                            devws.go(&pane)
                        }
                    }
                };
                if let Err(error) = outcome {
                    local_problem = Some(format!("Navigation failed: {error:#}"));
                }
            }
        }
    })();
    let mouse_result = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result?;
    mouse_result?;
    Ok(())
}

fn pane_of(board: &Board, digit: char) -> Option<String> {
    let number = digit.to_digit(10)? as u8;
    board
        .roles
        .iter()
        .flat_map(|row| &row.agents)
        .find(|agent| agent.number == Some(number) && agent.state != crate::devws::PaneState::Gone)
        .map(|agent| agent.pane.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_open_details_and_navigate_without_a_nine_agent_limit() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let entries: Vec<_> = (0..15)
            .map(|index| board_view::Entry {
                id: format!("agent:{index}"),
                label: index.to_string(),
                detail: "full text".into(),
                pane: Some(format!("%{index}")),
                attention: false,
                kind: board_view::Kind::Row,
                figures: String::new(),
                tail: String::new(),
            })
            .collect();
        let mut ui = Interaction::new(View::Agents);
        ui.nav.reconcile(&entries);
        ui.key(KeyCode::End, &board, &entries, 5);
        assert_eq!(
            ui.key(KeyCode::Char('o'), &board, &entries, 5),
            Action::Visit {
                pane: "%14".into(),
                peek: false
            }
        );
        ui.key(KeyCode::Enter, &board, &entries, 5);
        assert!(ui.nav.detail);
        ui.key(KeyCode::Esc, &board, &entries, 5);
        assert!(!ui.nav.detail);
        ui.key(KeyCode::Tab, &board, &entries, 5);
        assert_eq!(ui.nav.view, View::Steps);
        ui.key(KeyCode::BackTab, &board, &entries, 5);
        assert_eq!(ui.nav.view, View::Agents);
        assert_eq!(
            ui.key(KeyCode::Char('L'), &board, &entries, 5),
            Action::Leader
        );
    }
    #[test]
    fn a_fast_wheel_turn_moves_by_every_notch() {
        use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let entries: Vec<_> = (0..40)
            .map(|index| board_view::Entry {
                id: format!("step:{index}"),
                label: index.to_string(),
                detail: String::new(),
                pane: None,
                attention: false,
                kind: board_view::Kind::Row,
                figures: String::new(),
                tail: String::new(),
            })
            .collect();
        let mut ui = Interaction::new(View::Steps);
        ui.nav.reconcile(&entries);
        let notch = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })
        };
        for _ in 0..6 {
            ui.apply(notch(MouseEventKind::ScrollDown), &board, &entries, 10);
        }
        assert_eq!(ui.nav.index, 6 * WHEEL as usize);
        ui.apply(notch(MouseEventKind::ScrollUp), &board, &entries, 10);
        assert_eq!(ui.nav.index, 5 * WHEEL as usize);
    }

    #[test]
    fn a_slow_reading_never_holds_the_screen_and_a_failed_one_keeps_the_last() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = || load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO));
        let loader = Loader::new();
        let mut readings = 0;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                loader.run(Duration::from_secs(3600), || {
                    readings += 1;
                    // As slow as one agent's usage with a long conversation.
                    std::thread::sleep(Duration::from_millis(600));
                    if readings == 2 {
                        anyhow::bail!("devws is gone");
                    }
                    board()
                })
            });
            loader.refresh();
            let asked = Instant::now();
            assert!(loader.take().is_none(), "no reading is ready yet");
            assert!(
                asked.elapsed() < Duration::from_millis(100),
                "taking a reading waited for it"
            );
            let wait_for = |loader: &Loader| loop {
                if let Some(reading) = loader.take() {
                    return reading;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            assert!(wait_for(&loader).is_ok());
            loader.refresh();
            let failed = wait_for(&loader).err().unwrap();
            assert!(failed.contains("devws is gone"), "{failed}");
            loader.stop();
        });
    }

    #[test]
    fn unchanged_activity_does_not_read_history_and_failures_are_retried() {
        let mut reader = ActivityReader::default();
        let stamp = Some((12, std::time::SystemTime::UNIX_EPOCH));
        assert!(reader.update(stamp, || Ok(vec!["first".into()])).unwrap());
        assert!(
            !reader
                .update(stamp, || panic!("unchanged log must not be read"))
                .unwrap()
        );
        let next = Some((20, std::time::SystemTime::UNIX_EPOCH));
        assert!(
            reader
                .update(next, || anyhow::bail!("read failed"))
                .is_err()
        );
        assert_eq!(reader.lines, ["first"]);
        assert!(reader.update(next, || Ok(vec!["replaced".into()])).unwrap());
        assert_eq!(reader.lines, ["replaced"]);
        assert!(reader.update(None, || Ok(Vec::new())).unwrap());
        assert!(reader.lines.is_empty());
    }
}
