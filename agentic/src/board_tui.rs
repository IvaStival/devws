//! `devagentics board <task>`: the board in a terminal, kept up to date, with
//! the dispatcher running beside it. `--once` prints it as text instead.

use crate::board::Board;
use crate::board_usage::UsageReader;
use crate::board_view::{self, Navigation, ScreenMap, View};
use crate::devws::Devws;
use crate::home::Home;
use crate::leader;
use crate::roles::{Group, Roles};
use crate::task::Task;
use crate::watch::{self, Dispatcher};
use anyhow::Result;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
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
    // As tall as the whole list takes: the header, every row (in its card on
    // the Overview) and the keys.
    let height = if height == 0 {
        let header = board_view::header_rows(&board, &nav, width, u16::MAX) as usize;
        (header + board_view::list_rows(&entries) + 1).clamp(8, u16::MAX as usize) as u16
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
    if let Err(error) = crate::runner::prune_at_start(task)
        && let Ok(mut slot) = problem.lock()
    {
        *slot = Some(format!("pruning: {error:#}"));
    }
    if let Err(error) = crate::preview::expire(task)
        && let Ok(mut slot) = problem.lock()
    {
        *slot = Some(format!("preview: {error:#}"));
    }
    let mut dispatcher = Dispatcher::new(task, devws, home, watch::STALL_GRACE);
    let command = crate::executable::own()
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
    Visit {
        pane: String,
        peek: bool,
    },
    /// Turn this account on or off for the task.
    ToggleAccount(String),
    /// An account's switch for one worker group: `(group, account)`.
    ToggleGroupAccount(Group, String),
    ToggleOpenCode(String),
    /// Open the model picker of a group's agents on an account.
    ChooseModel(Group, String),
    /// The model picked for a group's agents on an account, for this task;
    /// None: the default again.
    SetModel(Group, String, Option<String>),
    /// What the user wrote to the leader in a dialog, about a step or not.
    TellLeader {
        about: crate::leader::About,
        text: String,
    },
}

/// The list a group's model on an account is picked from, under its row: the
/// default, the models known for the account's program (for opencode, what
/// `opencode models` lists), then one typed by hand.
#[derive(Debug, PartialEq)]
struct ModelPicker {
    group: Group,
    account: String,
    /// What the default is: each role's own model, or the home-wide OpenCode one.
    default: String,
    models: Vec<String>,
    /// The one picked for this task; None: the default.
    current: Option<String>,
    index: usize,
    /// The model being typed, after "Type a model…".
    typing: Option<String>,
    /// Why the list may be short: listing the account's models failed.
    note: Option<String>,
}

/// The lines above the choices in the picker.
const PICKER_HEADER: u16 = 3;

impl ModelPicker {
    fn new(
        group: Group,
        account: &str,
        default: String,
        models: Vec<String>,
        current: Option<String>,
    ) -> Self {
        // On the current choice: the default, a listed model, or the typed one.
        let index = match &current {
            None => 0,
            Some(model) => models
                .iter()
                .position(|listed| listed == model)
                .map_or(models.len() + 1, |position| position + 1),
        };
        Self {
            group,
            account: account.to_string(),
            default,
            models,
            current,
            index,
            typing: None,
            note: None,
        }
    }

    /// The default, the models, then "Type a model…".
    fn choices(&self) -> usize {
        self.models.len() + 2
    }

    fn is_typing_choice(&self) -> bool {
        self.index == self.choices() - 1
    }

    /// A key while the picker is open: the action, or None to keep it open.
    fn key(&mut self, code: KeyCode) -> Option<Action> {
        if let Some(text) = self.typing.as_mut() {
            match code {
                KeyCode::Char(character) => text.push(character),
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter if !text.trim().is_empty() => {
                    let model = text.trim().to_string();
                    return Some(self.set(Some(model)));
                }
                _ => {}
            }
            return None;
        }
        match code {
            KeyCode::Char('k') | KeyCode::Up => self.index = self.index.saturating_sub(1),
            KeyCode::Char('j') | KeyCode::Down => {
                self.index = (self.index + 1).min(self.choices() - 1)
            }
            KeyCode::Enter if self.is_typing_choice() => {
                self.typing = Some(self.current.clone().unwrap_or_default());
            }
            KeyCode::Enter => {
                let model = self
                    .index
                    .checked_sub(1)
                    .map(|index| self.models[index].clone());
                return Some(self.set(model));
            }
            KeyCode::Esc | KeyCode::Char('q') => return Some(Action::None),
            _ => {}
        }
        None
    }

    fn set(&self, model: Option<String>) -> Action {
        Action::SetModel(self.group, self.account.clone(), model)
    }

    fn content(&self) -> String {
        let mut text = format!(
            "The model {} agents start on {} with, for this task.",
            self.group.name(),
            self.account
        );
        if let Some(note) = &self.note {
            text.push_str(&format!(" {note}"));
        }
        text.push_str(if self.typing.is_some() {
            "\nType it below · Enter: choose · Esc: back to the list\n"
        } else {
            "\nj/k: move · Enter: choose · Esc: cancel\n"
        });
        let default = format!("Default: {}", self.default);
        let typed = match self.current.as_ref() {
            Some(model) if !self.models.contains(model) => format!("Type a model… ({model})"),
            _ => "Type a model…".to_string(),
        };
        let choices = std::iter::once(default.as_str())
            .chain(self.models.iter().map(String::as_str))
            .chain([typed.as_str()]);
        for (index, choice) in choices.enumerate() {
            let mark = if index == self.index { "▸" } else { " " };
            let is_current = match &self.current {
                None => index == 0,
                Some(model) => match index
                    .checked_sub(1)
                    .and_then(|index| self.models.get(index))
                {
                    Some(listed) => listed == model,
                    None => index > 0 && !self.models.contains(model),
                },
            };
            let now = if is_current { "  ✓ now" } else { "" };
            text.push_str(&format!("\n{mark} {choice}{now}"));
        }
        text
    }
}

/// The group and account of a LEADER/SENIOR/FLOOR ACCOUNTS row
/// (`group:floor:codex`).
fn group_row(id: &str) -> Option<(Group, &str)> {
    let (group, account) = id.strip_prefix("group:")?.split_once(':')?;
    crate::accounts::GROUPS
        .into_iter()
        .find(|worker_group| worker_group.name() == group)
        .map(|group| (group, account))
}

struct Interaction {
    nav: Navigation,
    peeking: bool,
    help: bool,
    /// The open model picker, over everything else.
    picker: Option<ModelPicker>,
    /// What the last drawn screen had where, for clicks.
    map: ScreenMap,
}

const HELP: &str = "Board navigation\n\nTab / Shift-Tab: next / previous view\nj/k or arrows: select row; scroll details\nPgUp/PgDn: move a page\ng/G or Home/End: first / last\nEnter: full details (JSON laid out, lists one item per line); a NEEDS YOU row you opened turns grey, one you answered shows ✉\nIn the details of a NEEDS YOU row, a step or the leader: Enter, m, a click in its box or a paste starts a message to the leader (Enter sends it, nothing when empty; Esc cancels)\nSpace on an account: turn it on/off for this task (ACCOUNTS) or for a group (SENIOR/FLOOR ACCOUNTS)\nEnter on an account under SENIOR/FLOOR ACCOUNTS (opencode too): choose the model the group runs on it for this task, or type one\nClick: select a row; click it again for details\nClick a view's name: show that view\nClick outside details: close them\nEsc: close details or cancel peek\no: go to selected agent\nv: peek selected agent\n1–9: go to numbered agent\np then 1–9: peek numbered agent\nL: return to leader\nl: toggle Activity\nc: toggle completed steps or inactive role catalog\nr: refresh\nz: zoom\n?: help\nq: close details, or quit board (dispatcher stops)";

impl Interaction {
    /// One event from the terminal: a key, or the wheel, which moves `WHEEL`
    /// rows a turn.
    fn apply(
        &mut self,
        event: Event,
        board: &Board,
        entries: &[board_view::Entry],
        size: Rect,
    ) -> Action {
        self.clamp_scroll(entries, size);
        let page = if let Some(content) = self.dialog_content(entries) {
            board_view::dialog_body(content, size.width, size.height, self.dialog_input(entries))
                .height
        } else {
            size.height.saturating_sub(
                board_view::header_rows(board, &self.nav, size.width, size.height) + 1,
            )
        }
        .max(1);
        let action = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                self.key(key.code, board, entries, page)
            }
            Event::Paste(text) => {
                self.paste(&text, entries);
                Action::None
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.click(mouse.column, mouse.row, entries);
                Action::None
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
        };
        self.clamp_scroll(entries, size);
        action
    }

    /// What the open dialog shows: the help, or the selected row's details.
    /// Whether the open dialog has the input box: the details of a row a
    /// message to the leader can be about.
    fn dialog_input(&self, entries: &[board_view::Entry]) -> bool {
        !self.help
            && self.nav.detail
            && entries
                .get(self.nav.index)
                .is_some_and(|entry| board_view::message_about(&entry.id).is_some())
    }

    fn dialog_content<'a>(&self, entries: &'a [board_view::Entry]) -> Option<&'a str> {
        if self.help {
            Some(HELP)
        } else if self.nav.detail {
            entries
                .get(self.nav.index)
                .map(|entry| entry.detail.as_str())
        } else {
            None
        }
    }

    /// A left click: outside an open dialog closes it; on a view's name
    /// shows that view; on a row selects it, and on the selected row opens
    /// its details, as Enter does. Anything else does nothing.
    /// Whether the open details are of a row the user can write about.
    fn takes_message(&self, entries: &[board_view::Entry]) -> bool {
        self.nav.detail
            && !self.help
            && entries
                .get(self.nav.index)
                .is_some_and(|entry| board_view::message_about(&entry.id).is_some())
    }

    /// Pasted text goes into the message, which it starts when none is open.
    fn paste(&mut self, text: &str, entries: &[board_view::Entry]) {
        if self.nav.compose.is_none() && !self.takes_message(entries) {
            return;
        }
        let message = self.nav.compose.get_or_insert_with(String::new);
        // One line: the box sends on Enter, so line breaks become spaces.
        message.push_str(&text.replace(['\r', '\n'], " "));
    }

    fn click(&mut self, column: u16, row: u16, entries: &[board_view::Entry]) {
        let position = ratatui::layout::Position::new(column, row);
        if self.picker.is_some() {
            if !self.map.dialog.is_some_and(|area| area.contains(position)) {
                self.picker = None;
            }
            return;
        }
        if self.map.input.is_some_and(|area| area.contains(position)) && self.takes_message(entries)
        {
            self.nav.compose.get_or_insert_with(String::new);
            return;
        }
        if self.help || self.nav.detail {
            let inside = self
                .map
                .dialog
                .is_some_and(|area| area.contains(ratatui::layout::Position::new(column, row)));
            if !inside {
                self.help = false;
                self.nav.detail = false;
                self.nav.detail_scroll = 0;
            }
            return;
        }
        self.peeking = false;
        if let Some(view) = self.map.view_at(column, row) {
            self.nav.change_view(view);
            return;
        }
        let Some(index) = self.map.entry_at(row) else {
            return;
        };
        if self.nav.selected.is_some() && self.nav.index == index {
            self.nav.detail = true;
            self.nav.detail_scroll = 0;
        } else {
            self.nav.select_row(entries, index);
        }
    }

    fn clamp_scroll(&mut self, entries: &[board_view::Entry], size: Rect) {
        if let Some(content) = self.dialog_content(entries) {
            self.nav.detail_scroll = self.nav.detail_scroll.min(board_view::dialog_scroll_max(
                content,
                size.width,
                size.height,
                self.dialog_input(entries),
            ));
        }
    }

    fn screen(
        &self,
        board: &Board,
        entries: &[board_view::Entry],
        width: u16,
        height: u16,
        notice: Option<&str>,
    ) -> (ratatui::buffer::Buffer, ScreenMap) {
        let (mut screen, mut map) =
            board_view::screen_map(board, &self.nav, entries, width, height, notice);
        if self.help {
            map.dialog = Some(board_view::draw_dialog(
                &mut screen,
                "Help",
                HELP,
                self.nav.detail_scroll,
                None,
                board_view::Message::None,
            ));
        }
        if let Some(picker) = &self.picker {
            let content = picker.content();
            let typing = picker.typing.is_some();
            let body = board_view::dialog_body(&content, width, height, typing).height;
            // Scrolled so the chosen line stays in sight.
            let scroll = (PICKER_HEADER + picker.index as u16 + 1).saturating_sub(body.max(1));
            // It drops down under the row it is for.
            let anchor = map
                .rows
                .iter()
                .find(|(_, index)| *index == self.nav.index)
                .map(|(row, _)| *row);
            let message = match &picker.typing {
                Some(text) => board_view::Message::Field {
                    title: "Model",
                    text,
                },
                None => board_view::Message::None,
            };
            let area = board_view::draw_dialog(
                &mut screen,
                &format!("{} model · {}", picker.group.label(), picker.account),
                &content,
                scroll,
                anchor,
                message,
            );
            map.dialog = Some(area);
        }
        (screen, map)
    }

    fn new(view: View) -> Self {
        Self {
            nav: Navigation::new(view),
            peeking: false,
            help: false,
            picker: None,
            map: ScreenMap::default(),
        }
    }

    fn key(
        &mut self,
        code: KeyCode,
        board: &Board,
        entries: &[board_view::Entry],
        page: u16,
    ) -> Action {
        self.nav.sent = false;
        if let Some(picker) = self.picker.as_mut() {
            let Some(action) = picker.key(code) else {
                return Action::None;
            };
            self.picker = None;
            return action;
        }
        if let Some(text) = self.nav.compose.as_mut() {
            // Writing to the leader: every key is text, but these.
            match code {
                KeyCode::Char(character) => text.push(character),
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Esc => self.nav.compose = None,
                KeyCode::Enter if !text.trim().is_empty() => {
                    let text = text.trim().to_string();
                    self.nav.compose = None;
                    let about = entries
                        .get(self.nav.index)
                        .and_then(|entry| board_view::message_about(&entry.id))
                        .unwrap_or(crate::leader::About::General);
                    return Action::TellLeader { about, text };
                }
                _ => {}
            }
            return Action::None;
        }
        match code {
            // In details that take a message, Enter (or m) starts writing it;
            // the next Enter sends it.
            KeyCode::Char('m') | KeyCode::Enter if self.takes_message(entries) => {
                self.nav.compose = Some(String::new());
            }
            KeyCode::Esc | KeyCode::Char('q') if self.help || self.nav.detail => {
                self.help = false;
                self.nav.detail = false;
                self.nav.detail_scroll = 0;
            }
            KeyCode::Esc if self.peeking => self.peeking = false,
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc => {}
            KeyCode::Char('?') => {
                self.help = !self.help;
                self.nav.detail = false;
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
            KeyCode::Enter
                if !self.help
                    && !self.nav.detail
                    && self.nav.selected.as_deref() == Some("opencode:floor") =>
            {
                return Action::ChooseModel(Group::Floor, board.opencode.account.clone());
            }
            KeyCode::Enter
                if !self.help
                    && !self.nav.detail
                    && self.nav.selected.as_deref().and_then(group_row).is_some() =>
            {
                let (group, account) = self.nav.selected.as_deref().and_then(group_row).unwrap();
                return Action::ChooseModel(group, account.to_string());
            }
            KeyCode::Enter if !self.help && self.nav.selected.is_some() => {
                self.nav.detail = true;
                self.nav.detail_scroll = 0;
            }
            KeyCode::Char(' ') if !self.help && !self.nav.detail => {
                if let Some(account) = self
                    .nav
                    .selected
                    .as_deref()
                    .and_then(|id| id.strip_prefix("account:"))
                {
                    return Action::ToggleAccount(account.to_string());
                }
                if let Some((group, account)) = self.nav.selected.as_deref().and_then(group_row) {
                    return Action::ToggleGroupAccount(group, account.to_string());
                }
                if let Some(setting) = self
                    .nav
                    .selected
                    .as_deref()
                    .and_then(|id| id.strip_prefix("opencode:"))
                {
                    return Action::ToggleOpenCode(setting.to_string());
                }
                self.nav.move_by(entries, page as isize);
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
        let result = screen_loop(task, devws, home, problem, view, board, &loader);
        loader.stop();
        result
    })
}

/// The screen: draws the latest reading and answers keys and the wheel; it
/// never reads the team itself (Loader does).
fn screen_loop(
    task: &Task,
    devws: &dyn Devws,
    home: &Home,
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
    let mut marks = crate::seen::Marks::load(task.dir())?;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        // A pane left on the alternate screen (a board that was killed) keeps
        // its old text there; the first frame would draw only over its own
        // cells and leave the rest showing through.
        terminal.clear()?;
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
                marks.apply(&mut entries);
                entries_key = Some(key);
                entries_dirty = false;
            }
            ui.nav.reconcile(&entries);
            let size = terminal.size()?;
            ui.clamp_scroll(&entries, Rect::new(0, 0, size.width, size.height));
            let mut drawn = ScreenMap::default();
            terminal.draw(|frame| {
                let area = frame.area();
                let (screen, map) = ui.screen(&board, &entries, area.width, area.height, notice);
                *frame.buffer_mut() = screen;
                drawn = map;
                if ui.peeking && area.height > 0 {
                    use ratatui::widgets::Widget;
                    ratatui::text::Line::raw("Peek: choose 1–9 · Esc cancel").render(
                        ratatui::layout::Rect::new(0, area.height - 1, area.width, 1),
                        frame.buffer_mut(),
                    );
                }
            })?;
            ui.map = drawn;
            if !event::poll(KEY_WAIT)? {
                continue;
            }
            // Every event already waiting, then one draw: a fast wheel turn
            // sends many, and drawing between each made it lag behind.
            let mut actions = Vec::new();
            loop {
                let action = ui.apply(
                    event::read()?,
                    &board,
                    &entries,
                    Rect::new(0, 0, size.width, size.height),
                );
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
                    Action::TellLeader { about, text } => crate::leader::tell(task, &about, &text)
                        .and_then(|()| {
                            ui.nav.sent = true;
                            match selected_entry(&ui.nav, &entries) {
                                Some(entry) => {
                                    marks.told(entry).map(|added| entries_dirty |= added)
                                }
                                None => Ok(()),
                            }
                        }),
                    Action::ToggleAccount(account) => toggle_account(task, &mut board, &account)
                        .map(|()| {
                            entries_dirty = true;
                            loader.refresh();
                            refreshed = Instant::now() - REFRESH;
                        }),
                    Action::ToggleGroupAccount(group, account) => toggle_group_account(
                        home, task, devws, &mut board, group, &account,
                    )
                    .map(|()| {
                        entries_dirty = true;
                        loader.refresh();
                        refreshed = Instant::now() - REFRESH;
                    }),
                    Action::ChooseModel(group, account) => devws.agent_kind(&account).map(|kind| {
                        ui.picker = Some(model_picker(&board, devws, group, &account, kind));
                    }),
                    Action::SetModel(group, account, model) => {
                        crate::accounts::set_group_model(
                            task,
                            devws,
                            &account,
                            group,
                            model.clone(),
                        )
                        .map(|()| {
                            // Shown at once, as the switches.
                            let models = board.group_models.entry(group).or_default();
                            match model {
                                Some(model) => {
                                    models.insert(account, model.trim().to_string());
                                }
                                None => {
                                    models.remove(&account);
                                }
                            }
                            entries_dirty = true;
                            loader.refresh();
                            refreshed = Instant::now() - REFRESH;
                        })
                    }
                    Action::ToggleOpenCode(setting) => {
                        let home = Home::from_env()?;
                        let mut settings = crate::opencode::OpencodeSettings::load(&home)?;
                        match setting.as_str() {
                            "global" => settings.enabled = !settings.enabled,
                            // The senior switch stays in the devws menu: the
                            // board offers OpenCode to the floor only.
                            "floor" => settings.floor.enabled = !settings.floor.enabled,
                            _ => anyhow::bail!("unknown OpenCode switch: {setting}"),
                        }
                        settings.save(&home)?;
                        board.opencode = settings;
                        entries_dirty = true;
                        loader.refresh();
                        refreshed = Instant::now() - REFRESH;
                        Ok(())
                    }
                };
                if let Err(error) = outcome {
                    local_problem = Some(format!("Board action failed: {error:#}"));
                }
            }
            // Opening a NEEDS YOU row's details is reading it.
            if ui.nav.detail {
                if let Some(entry) = selected_entry(&ui.nav, &entries) {
                    match marks.opened(entry) {
                        Ok(added) => entries_dirty |= added,
                        Err(error) => {
                            local_problem = Some(format!("Board action failed: {error:#}"))
                        }
                    }
                }
            }
        }
    })();
    let mouse_result = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste
    );
    ratatui::restore();
    result?;
    mouse_result?;
    Ok(())
}

/// The picker for `group`'s model on `account`: the models devws knows for
/// its program; an opencode account's are what `opencode models` lists.
fn model_picker(
    board: &Board,
    devws: &dyn Devws,
    group: Group,
    account: &str,
    kind: crate::providers::Kind,
) -> ModelPicker {
    let known = || -> Vec<String> {
        crate::roles::Setting::Model
            .choices(kind)
            .iter()
            .map(|model| model.to_string())
            .collect()
    };
    let current = board_view::group_model(board, group, account).map(str::to_string);
    // "sonnet* (each role's own)": what Default starts the group's agents on.
    let role_defaults = || -> String {
        let defaults = board_view::default_models(board, group, account);
        if defaults.is_empty() {
            "each role's own model".into()
        } else {
            let models: Vec<_> = defaults.iter().map(|model| format!("{model}*")).collect();
            format!("{} (each role's own)", models.join(", "))
        }
    };
    let listed = |default: String| match devws.models(account) {
        Ok(models) if !models.is_empty() => {
            ModelPicker::new(group, account, default, models, current.clone())
        }
        Ok(_) => ModelPicker::new(group, account, default, known(), current.clone()),
        Err(error) => {
            let mut picker = ModelPicker::new(group, account, default, known(), current.clone());
            picker.note = Some(format!("Listing its models failed: {error:#}."));
            picker
        }
    };
    let provider = kind.provider();
    if !provider.lists_models() {
        return ModelPicker::new(group, account, role_defaults(), known(), current);
    }
    // Its account lists its models; one on its own switches defaults to the
    // devws menu's model.
    if !provider.own_switches() {
        return listed(role_defaults());
    }
    let default = match board
        .opencode
        .group(group)
        .and_then(|settings| settings.model.as_deref())
    {
        Some(model) => format!("{model} (devws menu, every task)"),
        None => "none set in the devws menu".into(),
    };
    listed(default)
}

/// The row the selection is on.
fn selected_entry<'a>(
    nav: &Navigation,
    entries: &'a [board_view::Entry],
) -> Option<&'a board_view::Entry> {
    let id = nav.selected.as_deref()?;
    entries.iter().find(|entry| entry.id == id)
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

/// The accounts the board shows: a press asks devws nothing.
fn shown_accounts(board: &Board) -> Vec<String> {
    board
        .switches
        .iter()
        .map(|switch| switch.name.clone())
        .collect()
}

/// Space on a global switch: the task's state, and the board at once. The
/// reload after it asks every account for its limits; a switch that waited
/// for it got pressed twice.
fn toggle_account(task: &Task, board: &mut Board, account: &str) -> Result<()> {
    let enabled = board
        .switches
        .iter()
        .any(|switch| switch.name == account && switch.enabled);
    crate::accounts::set_enabled_among(task, &shown_accounts(board), account, !enabled)?;
    if let Some(switch) = board
        .switches
        .iter_mut()
        .find(|switch| switch.name == account)
    {
        switch.enabled = !enabled;
    }
    Ok(())
}

/// Space on a group's switch, shown at once as the global one.
/// The leader's: among the accounts that can lead, and off for the one it
/// leads on, it moves once its turn is over (`leader::set_leader_account`).
fn toggle_group_account(
    home: &Home,
    task: &Task,
    devws: &dyn Devws,
    board: &mut Board,
    group: Group,
    account: &str,
) -> Result<()> {
    let on = !board
        .switches
        .iter()
        .any(|switch| switch.name == account && switch.group_on(group));
    match group {
        Group::Cto => {
            let leading: Vec<String> = board
                .switches
                .iter()
                .filter(|switch| switch.can_lead)
                .map(|switch| switch.name.clone())
                .collect();
            crate::leader::set_leader_account(home, task, devws, &leading, account, on)?;
        }
        _ => crate::accounts::set_group_enabled_among(
            task,
            &shown_accounts(board),
            account,
            group,
            on,
        )?,
    }
    if let Some(switch) = board
        .switches
        .iter_mut()
        .find(|switch| switch.name == account)
    {
        switch.set_group_on(group, on);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A press shows at once, from the board's own accounts: before, the global
    // switch waited for a reload that asks every account for its limits.
    #[test]
    fn a_switch_shows_its_new_state_at_once_without_asking_devws() {
        use crate::board::AccountSwitch;
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, devws, task, _repo) = crate::board::tests::started(&dir);
        let switch = |name: &str| AccountSwitch {
            name: name.into(),
            enabled: true,
            five_hour: None,
            week: None,
            leader_on: false,
            leader: true,
            senior: true,
            floor: true,
            can_lead: true,
            kind: crate::providers::Kind::Claude,
        };
        let mut board = crate::board_view::tests::board();
        board.switches = vec![switch("claude"), switch("codex")];
        let asked = devws.calls().len();

        toggle_group_account(&_home, &task, &devws, &mut board, Group::Floor, "claude").unwrap();
        assert!(!board.switches[0].floor);
        assert!(!crate::accounts::is_group_switch_on(
            &task.state().unwrap(),
            "claude",
            Group::Floor
        ));
        toggle_account(&task, &mut board, "codex").unwrap();
        assert!(!board.switches[1].enabled, "the board shows it off at once");
        assert!(!crate::accounts::is_enabled(
            &task.state().unwrap(),
            "codex"
        ));
        assert_eq!(devws.calls().len(), asked, "a press asks devws nothing");

        let error = toggle_account(&task, &mut board, "claude")
            .unwrap_err()
            .to_string();
        assert!(error.contains("last account that is on"), "{error}");
        assert!(
            board.switches[0].enabled,
            "a refused press leaves the board as it was"
        );
    }

    #[test]
    fn help_is_a_dialog_over_the_real_board_and_closes_without_moving_selection() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let entries = board_view::entries(&board, View::Overview, false, &[], None);
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        let selected = ui.nav.selected.clone();
        let (background, _) = ui.screen(&board, &entries, 84, 30, None);
        ui.key(KeyCode::Char('?'), &board, &entries, 10);
        let (rendered, _) = ui.screen(&board, &entries, 84, 30, None);
        let text = board_view::plain(&rendered);
        assert!(text.contains("Help"), "{text}");
        assert!(text.contains("Board navigation"), "{text}");
        assert!(text.contains("Esc/q close"), "{text}");
        for row in 0..30 {
            assert_eq!(rendered[(0, row)], background[(0, row)]);
        }
        assert_eq!(
            ui.key(KeyCode::Char('q'), &board, &entries, 10),
            Action::None
        );
        assert!(!ui.help);
        assert_eq!(ui.nav.selected, selected);
        assert_eq!(ui.nav.view, View::Overview);
    }

    #[test]
    fn dialog_input_uses_its_body_and_clamps_each_event_including_the_wheel() {
        use ratatui::crossterm::event::{KeyEvent, KeyModifiers, MouseEvent};
        use ratatui::layout::Rect;
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let entries = vec![board_view::Entry::text(
            "long".into(),
            "Long details".into(),
            "content wraps ".repeat(300),
            false,
        )];
        let mut ui = Interaction::new(View::Activity);
        ui.nav.reconcile(&entries);
        let size = Rect::new(0, 0, 30, 16);
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        ui.apply(key(KeyCode::Enter), &board, &entries, size);
        ui.apply(key(KeyCode::PageDown), &board, &entries, size);
        assert_eq!(
            ui.nav.detail_scroll,
            board_view::dialog_body(&entries[0].detail, 30, 16, false).height
        );
        ui.apply(key(KeyCode::End), &board, &entries, size);
        let maximum = board_view::dialog_scroll_max(&entries[0].detail, 30, 16, false);
        assert_eq!(ui.nav.detail_scroll, maximum);
        ui.apply(key(KeyCode::Up), &board, &entries, size);
        assert_eq!(
            ui.nav.detail_scroll,
            maximum - 1,
            "Up works immediately after End"
        );
        ui.apply(
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            }),
            &board,
            &entries,
            size,
        );
        assert_eq!(ui.nav.detail_scroll, maximum - 1 - WHEEL);
        ui.apply(key(KeyCode::Home), &board, &entries, size);
        assert_eq!(ui.nav.detail_scroll, 0);
        ui.apply(key(KeyCode::Char('j')), &board, &entries, size);
        assert_eq!(ui.nav.detail_scroll, 1);
        ui.apply(key(KeyCode::Char('k')), &board, &entries, size);
        assert_eq!(ui.nav.detail_scroll, 0);
        ui.apply(key(KeyCode::Esc), &board, &entries, size);
        assert!(!ui.nav.detail);
        assert_eq!(ui.nav.selected.as_deref(), Some("long"));
    }

    #[test]
    fn m_in_a_steps_details_writes_to_the_leader_and_keys_become_text() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let row = |id: &str| board_view::Entry::text(id.into(), id.into(), "details".into(), true);
        let entries = vec![row("step:B1-1-AT"), row("agent:hammer-3")];
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        let press = |ui: &mut Interaction, code| ui.key(code, &board, &entries, 10);

        press(&mut ui, KeyCode::Enter);
        press(&mut ui, KeyCode::Char('m'));
        assert_eq!(ui.nav.compose.as_deref(), Some(""));
        for character in "ok qj".chars() {
            press(&mut ui, KeyCode::Char(character));
        }
        press(&mut ui, KeyCode::Backspace);
        assert_eq!(
            ui.nav.compose.as_deref(),
            Some("ok q"),
            "q and j are text while writing"
        );
        assert!(ui.nav.detail, "q did not close the details");
        assert_eq!(
            press(&mut ui, KeyCode::Enter),
            Action::TellLeader {
                about: crate::leader::About::Step("B1-1-AT".into()),
                text: "ok q".into()
            }
        );
        assert_eq!(ui.nav.compose, None);
        assert!(ui.nav.detail, "the details stay open");

        press(&mut ui, KeyCode::Char('m'));
        press(&mut ui, KeyCode::Char('x'));
        press(&mut ui, KeyCode::Esc);
        assert_eq!(ui.nav.compose, None);
        assert!(ui.nav.detail, "Esc cancels the message only");

        press(&mut ui, KeyCode::Esc);
        press(&mut ui, KeyCode::Char('j'));
        press(&mut ui, KeyCode::Enter);
        press(&mut ui, KeyCode::Char('m'));
        assert_eq!(ui.nav.compose, None, "an agent's details take no message");
    }

    #[test]
    fn enter_a_click_or_a_paste_starts_the_message_and_an_empty_one_is_never_sent() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let row = |id: &str| board_view::Entry::text(id.into(), id.into(), "details".into(), true);
        let entries = vec![row("leader-asks")];
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        let press = |ui: &mut Interaction, code| ui.key(code, &board, &entries, 10);

        press(&mut ui, KeyCode::Enter);
        assert!(ui.nav.detail);
        press(&mut ui, KeyCode::Enter);
        assert_eq!(ui.nav.compose.as_deref(), Some(""), "Enter starts writing");
        assert_eq!(
            press(&mut ui, KeyCode::Enter),
            Action::None,
            "nothing written, nothing sent"
        );
        press(&mut ui, KeyCode::Char(' '));
        assert_eq!(
            press(&mut ui, KeyCode::Enter),
            Action::None,
            "blanks are nothing"
        );
        assert_eq!(ui.nav.compose.as_deref(), Some(" "), "still writing");
        press(&mut ui, KeyCode::Esc);

        ui.paste("yes to 1\nand 3", &entries);
        assert_eq!(
            ui.nav.compose.as_deref(),
            Some("yes to 1 and 3"),
            "a paste starts it"
        );
        assert_eq!(
            press(&mut ui, KeyCode::Enter),
            Action::TellLeader {
                about: crate::leader::About::General,
                text: "yes to 1 and 3".into()
            }
        );

        let size = Rect::new(0, 0, 80, 30);
        let drawn = ui.screen(&board, &entries, size.width, size.height, None).1;
        ui.map = drawn;
        let input = ui.map.input.expect("the dialog has its message box");
        ui.click(input.x + 1, input.y + 1, &entries);
        assert_eq!(
            ui.nav.compose.as_deref(),
            Some(""),
            "a click in the box starts writing"
        );
        assert!(ui.nav.detail, "and keeps the details open");
    }

    #[test]
    fn an_antigravity_account_s_picker_offers_gemini_models_when_agy_lists_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let picker = model_picker(
            &board,
            &devws,
            Group::Floor,
            "gemini",
            crate::providers::Kind::Antigravity,
        );
        let content = picker.content();
        assert!(
            content.contains("Default: each role's own model"),
            "{content}"
        );
        assert!(content.contains("  gemini-3.8-flash-medium"), "{content}");
        assert!(
            devws.calls().contains(&"models gemini".to_string()),
            "agy's own list is asked first"
        );
    }

    #[test]
    fn enter_on_a_group_account_row_picks_its_model_from_the_known_or_a_typed_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let row = |id: &str| board_view::Entry::text(id.into(), id.into(), String::new(), false);
        let entries = vec![row("group:floor:codex"), row("opencode:floor")];
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        assert_eq!(
            ui.key(KeyCode::Enter, &board, &entries, 10),
            Action::ChooseModel(Group::Floor, "codex".into())
        );
        ui.key(KeyCode::Char('j'), &board, &entries, 10);
        assert_eq!(
            ui.key(KeyCode::Enter, &board, &entries, 10),
            Action::ChooseModel(Group::Floor, board.opencode.account.clone())
        );

        let picker = model_picker(
            &board,
            &devws,
            Group::Floor,
            "codex",
            crate::providers::Kind::Codex,
        );
        let content = picker.content();
        assert!(
            content.contains("▸ Default: each role's own model  ✓ now"),
            "{content}"
        );
        assert!(
            content.contains("  gpt-6-luna"),
            "the known ones: {content}"
        );
        assert!(content.contains("  Type a model…"), "{content}");
        ui.picker = Some(picker);
        ui.key(KeyCode::Char('j'), &board, &entries, 10);
        assert_eq!(
            ui.key(KeyCode::Enter, &board, &entries, 10),
            Action::SetModel(Group::Floor, "codex".into(), Some("gpt-6.1-sol".into()))
        );
        assert_eq!(ui.picker, None, "choosing closes it");

        let mut picker = ModelPicker::new(
            Group::Senior,
            "claude",
            "each role's own model".into(),
            vec!["claude-opus-5-5".into()],
            Some("claude-custom".into()),
        );
        assert!(
            picker
                .content()
                .contains("▸ Type a model… (claude-custom)  ✓ now"),
            "a typed one is the current: {}",
            picker.content()
        );
        assert_eq!(picker.key(KeyCode::Enter), None, "it starts typing");
        assert_eq!(picker.typing.as_deref(), Some("claude-custom"));
        for _ in 0.."custom".len() {
            picker.key(KeyCode::Backspace);
        }
        for character in "next".chars() {
            picker.key(KeyCode::Char(character));
        }
        assert_eq!(
            picker.key(KeyCode::Enter),
            Some(Action::SetModel(
                Group::Senior,
                "claude".into(),
                Some("claude-next".into())
            ))
        );
        picker.typing = None;
        picker.index = 0;
        assert_eq!(
            picker.key(KeyCode::Enter),
            Some(Action::SetModel(Group::Senior, "claude".into(), None)),
            "the default clears it"
        );
        assert_eq!(picker.key(KeyCode::Esc), Some(Action::None), "Esc cancels");

        let mut board = board;
        let opencode_default = |board: &Board| {
            model_picker(
                board,
                &devws,
                Group::Floor,
                "opencode",
                crate::providers::Kind::Opencode,
            )
            .content()
        };
        board.opencode.floor.model = None;
        assert!(opencode_default(&board).contains("Default: none set in the devws menu"));
        board.opencode.floor.model = Some("deepseek/deepseek-flash".into());
        assert!(
            opencode_default(&board)
                .contains("Default: deepseek/deepseek-flash (devws menu, every task)")
        );
    }

    #[test]
    fn space_switches_an_account_for_the_selected_group() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let row = |id: &str| board_view::Entry::text(id.into(), id.into(), String::new(), false);
        let entries = vec![
            row("group:senior:codex"),
            row("group:floor:claude"),
            row("group:cto:claude"),
        ];
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        assert_eq!(
            ui.key(KeyCode::Char(' '), &board, &entries, 10),
            Action::ToggleGroupAccount(Group::Senior, "codex".into())
        );
        ui.key(KeyCode::Char('j'), &board, &entries, 10);
        assert_eq!(
            ui.key(KeyCode::Char(' '), &board, &entries, 10),
            Action::ToggleGroupAccount(Group::Floor, "claude".into())
        );
        ui.key(KeyCode::Char('j'), &board, &entries, 10);
        assert_eq!(
            ui.key(KeyCode::Char(' '), &board, &entries, 1),
            Action::ToggleGroupAccount(Group::Cto, "claude".into()),
            "the leader has its own switch"
        );
    }

    #[test]
    fn space_switches_the_selected_account_and_pages_anywhere_else() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let board = load(&task, &devws, &home, &mut UsageReader::new(Duration::ZERO)).unwrap();
        let row = |id: &str| board_view::Entry::text(id.into(), id.into(), String::new(), false);
        let entries = vec![row("leader"), row("account:claude"), row("agent:1")];
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        ui.key(KeyCode::Char('j'), &board, &entries, 10);
        assert_eq!(ui.nav.selected.as_deref(), Some("account:claude"));
        assert_eq!(
            ui.key(KeyCode::Char(' '), &board, &entries, 10),
            Action::ToggleAccount("claude".into())
        );
        assert_eq!(
            ui.nav.selected.as_deref(),
            Some("account:claude"),
            "it stays put"
        );
        ui.key(KeyCode::Char('k'), &board, &entries, 10);
        assert_eq!(
            ui.key(KeyCode::Char(' '), &board, &entries, 1),
            Action::None
        );
        assert_ne!(
            ui.nav.selected.as_deref(),
            Some("leader"),
            "Space still pages"
        );
    }

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
                parts: Vec::new(),
                tail: String::new(),
                seen: board_view::Seen::New,
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
        assert_eq!(ui.nav.view, View::Plan);
        ui.key(KeyCode::Tab, &board, &entries, 5);
        assert_eq!(ui.nav.view, View::Steps);
        ui.key(KeyCode::BackTab, &board, &entries, 5);
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
                parts: Vec::new(),
                tail: String::new(),
                seen: board_view::Seen::New,
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
            ui.apply(
                notch(MouseEventKind::ScrollDown),
                &board,
                &entries,
                Rect::new(0, 0, 84, 16),
            );
        }
        assert_eq!(ui.nav.index, 6 * WHEEL as usize);
        ui.apply(
            notch(MouseEventKind::ScrollUp),
            &board,
            &entries,
            Rect::new(0, 0, 84, 16),
        );
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

    #[test]
    fn a_click_selects_a_row_opens_it_shows_a_view_and_closes_the_dialog() {
        use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
        let board = board_view::tests::board();
        let entries = board_view::entries(&board, View::Overview, false, &[], None);
        let mut ui = Interaction::new(View::Overview);
        ui.nav.reconcile(&entries);
        let size = Rect::new(0, 0, 84, 60);
        let draw = |ui: &mut Interaction| {
            ui.map = ui.screen(&board, &entries, 84, 60, None).1;
        };
        let click = |ui: &mut Interaction, column: u16, row: u16| {
            let event = Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            });
            ui.apply(event, &board, &entries, size)
        };
        let row_of = |ui: &Interaction, id: &str| {
            ui.map
                .rows
                .iter()
                .find(|(_, index)| entries[*index].id == id)
                .map(|(y, _)| *y)
                .unwrap_or_else(|| panic!("{id} is not drawn"))
        };
        draw(&mut ui);
        assert_ne!(ui.nav.selected.as_deref(), Some("leader"));

        let leader = row_of(&ui, "leader");
        assert_eq!(click(&mut ui, 10, leader), Action::None);
        assert_eq!(
            ui.nav.selected.as_deref(),
            Some("leader"),
            "a click selects"
        );
        assert!(!ui.nav.detail);
        draw(&mut ui);
        click(&mut ui, 10, leader);
        assert!(ui.nav.detail, "a second click opens its details");

        draw(&mut ui);
        let dialog = ui.map.dialog.expect("the dialog is drawn");
        click(&mut ui, dialog.x + 3, dialog.y + 1);
        assert!(ui.nav.detail, "a click inside keeps it open");
        click(&mut ui, 0, 0);
        assert!(!ui.nav.detail, "a click outside closes it");
        assert_eq!(
            ui.nav.selected.as_deref(),
            Some("leader"),
            "and keeps the row"
        );

        // The second line of an agent and the headings are not rows.
        draw(&mut ui);
        let note = row_of(&ui, "agent:executor-1") + 1;
        click(&mut ui, 10, note);
        assert_eq!(ui.nav.selected.as_deref(), Some("leader"));

        let (row, columns, _) = ui
            .map
            .tabs
            .iter()
            .find(|(_, _, view)| *view == View::Agents)
            .cloned()
            .expect("the agents tab is drawn");
        click(&mut ui, columns.start + 1, row);
        assert_eq!(
            ui.nav.view,
            View::Agents,
            "a click on a view's name shows it"
        );
    }
}
