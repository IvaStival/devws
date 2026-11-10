//! Pure board navigation and rendering. Stable row identities keep a refresh
//! from moving the user's focus to a different agent or task.

use crate::board::{AgentRow, Board, RoleRow, StepRow};
use crate::devws::{PaneState, Usage};
use crate::roles::Group;
use crate::state::StepStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

const TITLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().fg(Color::DarkGray);
const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);
const HEADING: Style = Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum View {
    #[default]
    Overview,
    Agents,
    Steps,
    Activity,
    Usage,
}

impl View {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Agents,
        Self::Steps,
        Self::Activity,
        Self::Usage,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Agents => "agents",
            Self::Steps => "steps",
            Self::Activity => "activity",
            Self::Usage => "usage",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|view| view.name() == text.trim())
    }

    pub fn next(self, backwards: bool) -> Self {
        let index = Self::ALL.iter().position(|view| *view == self).unwrap_or(0);
        Self::ALL[(index + if backwards { 4 } else { 1 }) % 5]
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub pane: Option<String>,
    pub attention: bool,
    pub kind: Kind,
    /// What the row ends with, at the right edge: its state (`✓ done`).
    pub tail: String,
    /// Columns after the tail in a wide pane: an agent's cost and tokens.
    pub figures: String,
}

impl Entry {
    pub fn text(id: String, label: String, detail: String, attention: bool) -> Self {
        Self {
            id,
            label,
            detail,
            pane: None,
            attention,
            kind: Kind::Row,
            tail: String::new(),
            figures: String::new(),
        }
    }

    fn shown(kind: Kind, id: String, label: String) -> Self {
        Self {
            id,
            label,
            detail: String::new(),
            pane: None,
            attention: false,
            kind,
            tail: String::new(),
            figures: String::new(),
        }
    }

    /// A section's title, `NAME · count`; its id keeps the name alone, so it
    /// stays the same when the count changes.
    pub fn section(name: &str, count: Option<usize>) -> Self {
        let label = count.map_or_else(|| name.to_string(), |count| format!("{name} · {count}"));
        Self::shown(Kind::Heading, format!("heading:{name}"), label)
    }

    /// The dim line under the row `of`.
    pub fn note(of: &str, text: &str) -> Self {
        Self::shown(Kind::Note, format!("note:{of}"), text.to_string())
    }

    /// An empty line; `name` keeps its id apart from the others.
    pub fn gap(name: &str) -> Self {
        Self::shown(Kind::Gap, format!("gap:{name}"), String::new())
    }

    fn with_tail(mut self, tail: impl Into<String>) -> Self {
        self.tail = tail.into();
        self
    }
}

/// What a line of the list is: only a `Row` can be selected; the others are
/// shown around the rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Row,
    /// A section's title.
    Heading,
    /// A dim line under a row: an agent's current step and counts.
    Note,
    /// An empty line between sections.
    Gap,
}

/// The rows the selection can be on: every one but the headings, notes and
/// gaps.
fn selectable(entries: &[Entry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == Kind::Row)
        .map(|(index, _)| index)
        .collect()
}

#[derive(Debug)]
pub struct Navigation {
    pub view: View,
    pub selected: Option<String>,
    pub index: usize,
    pub detail: bool,
    pub detail_scroll: u16,
    pub expanded: bool,
}

impl Navigation {
    pub fn new(view: View) -> Self {
        Self {
            view,
            selected: None,
            index: 0,
            detail: false,
            detail_scroll: 0,
            expanded: false,
        }
    }

    pub fn change_view(&mut self, view: View) {
        if self.view != view {
            *self = Self::new(view);
        }
    }

    pub fn reconcile(&mut self, entries: &[Entry]) {
        let wanted = self
            .selected
            .as_ref()
            .and_then(|id| entries.iter().position(|entry| &entry.id == id))
            .unwrap_or(self.index);
        // On a heading, or past the end: the next row below, else above.
        let rows = selectable(entries);
        let index = rows
            .iter()
            .find(|&&row| row >= wanted)
            .or_else(|| rows.last())
            .copied();
        self.select(entries, index);
    }

    /// Moves by `amount` rows, headings not counted.
    pub fn move_by(&mut self, entries: &[Entry], amount: isize) {
        let rows = selectable(entries);
        let place = rows
            .iter()
            .position(|&row| row >= self.index)
            .unwrap_or(rows.len().saturating_sub(1));
        let target = place
            .saturating_add_signed(amount)
            .min(rows.len().saturating_sub(1));
        self.select(entries, rows.get(target).copied());
        self.detail_scroll = 0;
    }

    fn select(&mut self, entries: &[Entry], index: Option<usize>) {
        self.index = index.unwrap_or(0);
        self.selected = index.map(|index| entries[index].id.clone());
        if self.selected.is_none() {
            self.detail = false;
        }
    }
}

fn state_word(state: PaneState) -> &'static str {
    match state {
        PaneState::Working => "working",
        PaneState::Waiting => "needs you",
        PaneState::Done => "done",
        PaneState::Idle => "idle",
        PaneState::Gone => "gone",
    }
}

fn step_word(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Ordered => "ordered",
        StepStatus::Reported => "to review",
        StepStatus::Failed => "failed",
        StepStatus::Blocked => "blocked",
        StepStatus::Rejected => "sent back",
        StepStatus::Accepted => "accepted",
    }
}

/// A step's mark: done, at work, waiting for the leader's review, or stuck.
fn step_mark(status: StepStatus) -> char {
    match status {
        StepStatus::Accepted => '✓',
        StepStatus::Ordered => '⠋',
        StepStatus::Reported => '●',
        StepStatus::Failed | StepStatus::Blocked | StepStatus::Rejected => '⚠',
    }
}

fn step_entry(board: &Board, step: &StepRow) -> Entry {
    let id = format!("step:{}", step.id);
    let label = format!("{} {}  {}", step_mark(step.status), step.id, step.title);
    let detail = board.details.get(&id).cloned().unwrap_or_else(|| {
        format!(
            "{} {}: {}\nAttempts: {}",
            step.id,
            step.title,
            step_word(step.status),
            step.attempts
        )
    });
    Entry::text(
        id,
        label,
        detail,
        step.status != StepStatus::Ordered && step.status != StepStatus::Accepted,
    )
    .with_tail(step_word(step.status))
}

/// An agent's row, with its full details for Enter and its pane for `o`/`v`.
fn agent_entry(board: &Board, role: &RoleRow, agent: &AgentRow, label: String) -> Entry {
    let id = format!("agent:{}", agent.id);
    let detail = format!(
        "{}\nGroup: {} · {}\nPane activity: {}\nCurrent: {}\nNext: {}",
        board
            .details
            .get(&id)
            .cloned()
            .unwrap_or_else(|| agent.id.clone()),
        role.group.label(),
        role.program,
        state_word(agent.state),
        agent.now.as_deref().unwrap_or("No delivered order"),
        agent.next.join("\n")
    );
    Entry {
        id,
        label,
        detail,
        pane: (agent.state != PaneState::Gone).then(|| agent.pane.clone()),
        attention: matches!(agent.state, PaneState::Waiting | PaneState::Gone),
        kind: Kind::Row,
        tail: String::new(),
        figures: String::new(),
    }
}

/// A pane's state as the row's right edge shows it.
fn state_mark(state: PaneState) -> &'static str {
    match state {
        PaneState::Working => "⠋ working",
        PaneState::Waiting => "● needs you",
        PaneState::Done => "✓ done",
        PaneState::Idle => "· idle",
        PaneState::Gone => "✕ gone",
    }
}

/// `×3` for a step that took three attempts; nothing for one.
fn attempts(step: &StepRow) -> String {
    if step.attempts > 1 {
        format!("  ×{}", step.attempts)
    } else {
        String::new()
    }
}

/// `$11 · 1.4M tok`: what some agents used in all (input and output tokens;
/// the cache, far larger, is in the header). Nothing when none is known.
fn total_used(used: &[Usage]) -> String {
    if used.is_empty() {
        return String::new();
    }
    let sum = used
        .iter()
        .fold(Usage::default(), |sum, usage| sum.plus(*usage));
    format!("{} · {} tok", dollars(&sum), human(sum.input + sum.output))
}

/// An agent's cost and tokens, right-aligned for the row's columns.
fn figures(usage: Option<Usage>) -> String {
    usage.map_or_else(String::new, |usage| {
        format!(
            "{:>7} {:>6}",
            dollars(&usage),
            human(usage.input + usage.output)
        )
    })
}

/// An agent's second line: its current step, the steps it got accepted and
/// its retries; none when there is nothing to say.
fn agent_note(agent: &AgentRow) -> Option<Entry> {
    let mut counts = Vec::new();
    if agent.accepted > 0 {
        counts.push(format!("{} accepted", agent.accepted));
    }
    if agent.retries > 0 {
        counts.push(format!(
            "{} retr{}",
            agent.retries,
            if agent.retries == 1 { "y" } else { "ies" }
        ));
    }
    let now = agent.now.as_ref().map(|now| format!("└ now {now}"));
    if now.is_none() && counts.is_empty() {
        return None;
    }
    let of = format!("agent:{}", agent.id);
    Some(Entry::note(&of, now.as_deref().unwrap_or("└")).with_tail(counts.join(" · ")))
}

/// The Overview: only what needs looking at, as sections the selection moves
/// through: what needs you, the team (the leader first), the queue's size and
/// the steps done, newest first. A section with nothing in it is left out.
fn overview(board: &Board, problem: Option<&str>) -> Vec<Entry> {
    let mut needs_you = Vec::new();
    if let Some(problem) = problem {
        needs_you.push(Entry::text(
            "dispatcher".into(),
            format!("✕ Dispatcher: {problem}"),
            problem.into(),
            true,
        ));
    }
    for question in &board.questions {
        let id = question.split_whitespace().next().unwrap_or(question);
        needs_you.push(Entry::text(
            format!("question:{id}"),
            format!("? {question}"),
            question.clone(),
            true,
        ));
    }
    for step in &board.review {
        needs_you.push(step_entry(board, step));
    }
    let agents = board
        .roles
        .iter()
        .flat_map(|role| role.agents.iter().map(move |agent| (role, agent)));
    for (role, agent) in agents.clone() {
        if matches!(agent.state, PaneState::Waiting | PaneState::Gone) {
            let mut entry = agent_entry(board, role, agent, format!("● {}", agent.id));
            entry.id = format!("attention:{}", agent.id);
            needs_you.push(entry.with_tail(state_mark(agent.state)));
        }
    }

    let mut entries = Vec::new();
    if !needs_you.is_empty() {
        entries.push(Entry::section("NEEDS YOU", Some(needs_you.len())));
        entries.append(&mut needs_you);
    }

    // The CTO (the leader, and any agent of its group), then the seniors and
    // the floor: each a section with what its agents used in all.
    for group in [Group::Cto, Group::Senior, Group::Floor] {
        let members: Vec<_> = agents
            .clone()
            .filter(|(role, _)| role.group == group)
            .collect();
        if group != Group::Cto && members.is_empty() {
            continue;
        }
        let mut used: Vec<Usage> = members
            .iter()
            .filter_map(|(_, agent)| agent.usage)
            .collect();
        if group == Group::Cto {
            used.extend(board.leader_usage);
        }
        entries.push(Entry::gap(group.name()));
        let name = group.workspace().to_uppercase();
        let count = (group != Group::Cto).then_some(members.len());
        entries.push(Entry::section(&name, count).with_tail(total_used(&used)));
        if group == Group::Cto {
            let mut leader = Entry::text(
                "leader".into(),
                format!("L  {:<16} {}", "leader", board.leader),
                format!("The leader: {}\nYou talk to it in its pane.", board.leader),
                false,
            );
            leader.pane = board.leader_pane.clone();
            leader.figures = figures(board.leader_usage);
            entries.push(leader.with_tail(board.leader_state.map_or("", state_mark)));
        }
        for (role, agent) in members {
            let number = agent
                .number
                .map_or_else(|| " ".to_string(), |number| number.to_string());
            let program = match &agent.lane {
                Some(lane) => format!("{} · {lane}", role.program),
                None => role.program.clone(),
            };
            let mut row = agent_entry(
                board,
                role,
                agent,
                format!("{number}  {:<16} {program}", agent.id),
            );
            row.figures = figures(agent.usage);
            entries.push(row.with_tail(state_mark(agent.state)));
            if let Some(note) = agent_note(agent) {
                entries.push(note);
            }
        }
    }
    if !board.queue.is_empty() {
        let count = board.queue.len();
        entries.push(Entry::text(
            "queue".into(),
            format!(
                "⋯ {count} order{} queued",
                if count == 1 { "" } else { "s" }
            ),
            board
                .queue
                .iter()
                .map(|order| order.label.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            false,
        ));
    }
    if !board.done.is_empty() {
        entries.push(Entry::gap("done"));
        entries.push(
            Entry::section("DONE", Some(board.done.len())).with_tail("newest first · ×n retries"),
        );
        for step in &board.done {
            // Done rows need no "accepted" at the edge: the section says it.
            let mut entry = step_entry(board, step).with_tail("");
            entry.label.push_str(&attempts(step));
            entries.push(entry);
        }
    }
    entries
}

pub fn entries(
    board: &Board,
    view: View,
    expanded: bool,
    log: &[String],
    problem: Option<&str>,
) -> Vec<Entry> {
    if view == View::Overview {
        return overview(board, problem);
    }
    let mut entries = Vec::new();
    if view == View::Agents {
        for role in &board.roles {
            for agent in &role.agents {
                let number = agent
                    .number
                    .map_or_else(String::new, |number| format!("{number} "));
                let label = format!(
                    "{number}● {} · {}{} · [{}]",
                    agent.id,
                    state_word(agent.state),
                    agent
                        .now
                        .as_ref()
                        .map_or_else(String::new, |now| format!(" · {now}")),
                    role.group.workspace()
                );
                entries.push(agent_entry(board, role, agent, label));
            }
            if view == View::Agents && expanded && role.agents.is_empty() {
                let label = format!("{} · not running · {}", role.name, role.program);
                entries.push(Entry::text(
                    format!("role:{}", role.name),
                    label.clone(),
                    format!(
                        "{label}\n{}\n{}",
                        role.group.label(),
                        role.description.as_deref().unwrap_or("")
                    ),
                    false,
                ));
            }
        }
    }
    if view == View::Steps {
        entries.extend(
            board
                .steps
                .iter()
                .filter(|step| expanded || step.status != StepStatus::Accepted)
                .map(|step| step_entry(board, step)),
        );
    }
    if view == View::Activity {
        entries.extend(log.iter().enumerate().rev().map(|(index, line)| {
            let text = crate::watch::format_line(line);
            let detail = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|entry| {
                    entry
                        .get("text")
                        .and_then(|text| text.as_str())
                        .map(str::to_string)
                })
                .map_or_else(|| line.clone(), |full| format!("{text}\n\n{full}"));
            Entry::text(format!("event:{index}"), text, detail, false)
        }));
    }
    if view == View::Usage {
        let mut ids: Vec<String> = if board.accounts.is_empty() {
            vec!["account:none".into()]
        } else {
            board
                .accounts
                .iter()
                .map(|account| format!("account:{}", account.name))
                .collect()
        };
        if board.usage.is_empty() {
            ids.push("usage:none".into());
        } else {
            ids.extend(board.usage.iter().map(|(name, _)| format!("usage:{name}")));
            if board.usage.len() > 1 {
                ids.push("usage:team".into());
            }
            ids.push("usage:pricing-note".into());
        }
        for (id, line) in ids
            .into_iter()
            .zip(account_lines(board).into_iter().chain(usage_lines(board)))
        {
            let text = line.to_string();
            entries.push(Entry::text(id, text.clone(), text, false));
        }
    }
    entries
}

fn percent(value: Option<u32>) -> String {
    value.map_or_else(|| "—".to_string(), |value| format!("{value}%"))
}

fn account_lines(board: &Board) -> Vec<Line<'static>> {
    if board.accounts.is_empty() {
        return vec![Line::styled("No agent is running.", DIM)];
    }
    board
        .accounts
        .iter()
        .map(|account| {
            Line::from(vec![
                Span::styled(format!("{:<14}", account.name), BOLD),
                Span::raw(format!(
                    "5 h {:<5} week {}",
                    percent(account.five_hour),
                    percent(account.week)
                )),
            ])
        })
        .collect()
}

/// 980, 12k, 1.2M.
fn human(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

fn dollars(usage: &Usage) -> String {
    let sign = if usage.unpriced { "+" } else { "" };
    if usage.cost < 10.0 {
        format!("${:.2}{sign}", usage.cost)
    } else {
        format!("${:.0}{sign}", usage.cost)
    }
}

fn usage_line(name: &str, usage: &Usage, name_width: usize, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{name:<name_width$}"), style),
        Span::raw(format!(
            "in {:<6} out {:<6} cache {:<6} {}",
            human(usage.input),
            human(usage.output),
            human(usage.cache),
            dollars(usage)
        )),
    ])
}

fn usage_lines(board: &Board) -> Vec<Line<'static>> {
    if board.usage.is_empty() {
        return vec![Line::styled("Nothing read yet.", DIM)];
    }
    let name_width = board
        .usage
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0)
        .max("team".len())
        + 2;
    let mut lines: Vec<_> = board
        .usage
        .iter()
        .map(|(name, usage)| usage_line(name, usage, name_width, BOLD))
        .collect();
    if board.usage.len() > 1 {
        let total = crate::board_usage::total(&board.usage);
        lines.push(usage_line("team", &total, name_width, TITLE));
    }
    lines.push(Line::styled(
        "tokens read from each agent's log; cost at API prices",
        DIM,
    ));
    lines
}

fn text_line(text: impl Into<String>, style: Style, y: u16, width: u16, buffer: &mut Buffer) {
    Line::styled(text.into(), style).render(Rect::new(0, y, width, 1), buffer);
}

/// A row: its label, cut with `…` where it would reach what is at the right
/// edge: the tail (a state, in its color unless `tail_style` says otherwise)
/// and, when the pane is wide enough, the figures after it in columns.
#[allow(clippy::too_many_arguments)]
fn row_line(
    label: &str,
    tail: &str,
    figures: &str,
    style: Style,
    tail_style: Option<Style>,
    y: u16,
    width: u16,
    buffer: &mut Buffer,
) {
    let width = width as usize;
    let mut right = vec![];
    if !tail.is_empty() {
        let tail_style = tail_style.unwrap_or(match tail.chars().next() {
            Some('⠋') => style.fg(Color::Cyan),
            Some('●') => style.fg(Color::Yellow),
            Some('✓') => style.fg(Color::Green),
            Some('✕') => style.fg(Color::Red),
            _ => style.add_modifier(Modifier::DIM),
        });
        right.push(Span::styled(tail.to_string(), tail_style));
    }
    // Wide panes add the figures, the tail padded so the columns line up.
    if width >= WIDE && !figures.is_empty() {
        let pad = STATE_WIDTH.saturating_sub(Line::raw(tail).width());
        right.push(Span::styled(format!("{}{figures}", " ".repeat(pad)), style));
    }
    let right_width: usize = right.iter().map(Span::width).sum();
    let room = if right_width == 0 {
        width
    } else {
        width.saturating_sub(right_width + 1)
    };
    let mut text: String = label.chars().take(room).collect();
    if text.chars().count() < label.chars().count() && room > 0 {
        text = text.chars().take(room - 1).collect::<String>() + "…";
    }
    let padding = width.saturating_sub(Line::raw(text.as_str()).width() + right_width);
    let mut spans = vec![
        Span::styled(text, style),
        Span::styled(" ".repeat(padding), style),
    ];
    spans.extend(right);
    Line::from(spans).render(Rect::new(0, y, width as u16, 1), buffer);
}

/// Panes this wide show an agent's cost and tokens beside its state.
const WIDE: usize = 60;
/// The widest state (`● needs you`) and a space, so the figures line up.
const STATE_WIDTH: usize = 12;

/// Cells of a limit bar, as in the devws header.
const LIMIT_CELLS: usize = 10;

/// The level color of something `percent` used: green, yellow from 60 %, red
/// from 85 % (the devws header's levels).
fn level(percent: u32) -> Color {
    if percent >= 85 {
        Color::Red
    } else if percent >= 60 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// `cells` cells, `▓` for the part filled and `░` for the rest.
fn bar(filled: u32, total: u32, cells: usize) -> String {
    let full = if total == 0 {
        0
    } else {
        ((filled as usize * cells + total as usize / 2) / total as usize).min(cells)
    };
    "▓".repeat(full) + &"░".repeat(cells - full)
}

/// `5h ▓▓▓▓░░░░░░  42%`, in its level's color; empty and `—` when unknown.
fn limit(label: &str, used: Option<u32>) -> Vec<Span<'static>> {
    let color = used.map_or(Color::DarkGray, level);
    vec![
        Span::styled(format!("{label} "), DIM),
        Span::styled(
            bar(used.unwrap_or(0), 100, LIMIT_CELLS),
            Style::new().fg(color),
        ),
        Span::raw(used.map_or_else(|| "    —".to_string(), |used| format!(" {used:>3}%"))),
    ]
}

/// What needs you: questions, steps to review, agents waiting or gone, and a
/// dispatcher that cannot work.
fn attention(board: &Board, problem: Option<&str>) -> usize {
    board.questions.len()
        + board.review.len()
        + board
            .roles
            .iter()
            .flat_map(|role| &role.agents)
            .filter(|agent| matches!(agent.state, PaneState::Waiting | PaneState::Gone))
            .count()
        + usize::from(problem.is_some())
}

/// The rows above every view, most important first, so a short pane keeps
/// the first ones: the task with what needs you, the views, the progress, a
/// limit row per account, and for views but the Overview what `c` does.
fn header(
    board: &Board,
    nav: &Navigation,
    width: u16,
    problem: Option<&str>,
) -> Vec<Line<'static>> {
    let width = width as usize;
    let mut rows = Vec::new();

    let title = format!(" {} · {}", board.task, board.branch);
    let count = attention(board, problem);
    let mut first = vec![Span::styled(title.clone(), TITLE)];
    if count > 0 {
        let alert = format!("● {count} need you");
        let room = width.saturating_sub(Line::raw(title.as_str()).width() + alert.chars().count());
        first.push(Span::raw(" ".repeat(room.max(1))));
        first.push(Span::styled(
            alert,
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    rows.push(Line::from(first));

    let tabs: Vec<Span> = if width >= 65 {
        View::ALL
            .iter()
            .flat_map(|view| {
                let span = if *view == nav.view {
                    Span::styled(format!("[{}]", view.name()), TITLE)
                } else {
                    Span::styled(view.name().to_string(), DIM)
                };
                [Span::raw(" "), span, Span::raw(" ")]
            })
            .collect()
    } else {
        vec![
            Span::raw(" "),
            Span::styled(format!("[{}]", nav.view.name()), TITLE),
            Span::styled("  Tab views", DIM),
        ]
    };
    rows.push(Line::from(tabs));

    let total = board.steps.len() as u32;
    let done = board.done.len() as u32;
    let percent = (done * 100).checked_div(total).unwrap_or(0);
    let summary = format!(" {done}/{total} · {percent}% · {}", board.phase);
    let cells = width
        .saturating_sub(11 + summary.chars().count())
        .clamp(4, 40);
    rows.push(Line::from(vec![
        Span::styled(" PROGRESS  ", DIM),
        Span::styled(
            dots(done, total, cells),
            Style::new().fg(if total > 0 && done == total {
                Color::Green
            } else {
                Color::Cyan
            }),
        ),
        Span::raw(summary),
    ]));

    if !board.usage.is_empty() {
        let used = crate::board_usage::total(&board.usage);
        rows.push(Line::from(vec![
            Span::styled(" COST  ", DIM),
            Span::styled(dollars(&used), BOLD),
            Span::styled("      TOKENS  ", DIM),
            Span::raw(format!(
                "{} in · {} out · {} cache",
                human(used.input),
                human(used.output),
                human(used.cache)
            )),
        ]));
    }

    let name_width = board
        .accounts
        .iter()
        .map(|account| account.name.chars().count())
        .max()
        .unwrap_or(0);
    for account in &board.accounts {
        let mut spans = vec![Span::raw(format!(" {:<name_width$}  ", account.name))];
        spans.extend(limit("5h", account.five_hour));
        // Narrow panes keep the 5-hour window: it runs out first.
        if width >= 60 {
            spans.push(Span::raw("   "));
            spans.extend(limit("wk", account.week));
        }
        rows.push(Line::from(spans));
    }

    let caption = match nav.view {
        View::Overview => None,
        View::Agents => Some(format!(
            "Agents by group · c {} role catalog",
            if nav.expanded { "hide" } else { "show" }
        )),
        View::Steps => Some(format!(
            "Steps · c {} completed",
            if nav.expanded { "hide" } else { "show" }
        )),
        View::Activity => Some("Activity · newest first".into()),
        View::Usage => Some("Accounts and usage · cost at API prices".into()),
    };
    if let Some(caption) = caption {
        rows.push(Line::styled(format!(" {caption}"), DIM));
    }
    rows
}

/// `cells` braille cells, `⣿` for the part done and `⣀` for the rest: dots
/// every terminal draws, where shade blocks may come out as flat tints.
fn dots(filled: u32, total: u32, cells: usize) -> String {
    bar(filled, total, cells)
        .replace('▓', "⣿")
        .replace('░', "⣀")
}

/// Panes narrower than this have no frame around the header.
const FRAME_WIDTH: u16 = 30;

/// Whether the header is drawn in its frame: wide enough, and tall enough to
/// keep a row of the list and the keys below it.
fn framed(board: &Board, nav: &Navigation, width: u16, height: u16) -> bool {
    // The frame's top edge carries the first row (the task and what needs you).
    let inside = header(board, nav, u16::MAX, None).len() as u16 - 1;
    width >= FRAME_WIDTH && height >= inside + 2 + 2
}

/// How many rows the header takes in a pane of `width` × `height`: the frame
/// and what is in it, else as many plain rows as leave three for the list
/// and the keys, at least the first (the task and what needs you).
pub fn header_rows(board: &Board, nav: &Navigation, width: u16, height: u16) -> u16 {
    let full = header(board, nav, u16::MAX, None).len() as u16;
    if framed(board, nav, width, height) {
        return full - 1 + 2;
    }
    full.min(height.saturating_sub(3).max(1))
}

/// The header in its rounded frame: the task, branch and what needs you in
/// the top edge, the other rows inside.
fn draw_framed(
    board: &Board,
    nav: &Navigation,
    width: u16,
    problem: Option<&str>,
    buffer: &mut Buffer,
) -> u16 {
    let border = DIM;
    let inner = width - 4;
    let rows = header(board, nav, inner, problem);
    let count = attention(board, problem);
    let alert = (count > 0).then(|| format!(" ● {count} need you "));
    let alert_width = alert.as_ref().map_or(0, |alert| alert.chars().count());
    let room = (width as usize).saturating_sub(6 + alert_width);
    let title: String = format!(" {} · {} ", board.task, board.branch)
        .chars()
        .take(room)
        .collect();
    let fill = (width as usize).saturating_sub(4 + title.chars().count() + alert_width);
    let mut top = vec![
        Span::styled("╭─", border),
        Span::styled(title, TITLE),
        Span::styled("─".repeat(fill), border),
    ];
    if let Some(alert) = alert {
        top.push(Span::styled(
            alert,
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    top.push(Span::styled("─╮", border));
    Line::from(top).render(Rect::new(0, 0, width, 1), buffer);
    let mut y = 1;
    for row in rows.into_iter().skip(1) {
        Span::styled("│ ", border).render(Rect::new(0, y, 2, 1), buffer);
        row.render(Rect::new(2, y, inner, 1), buffer);
        Span::styled(" │", border).render(Rect::new(width - 2, y, 2, 1), buffer);
        y += 1;
    }
    Line::styled(format!("╰{}╯", "─".repeat(width as usize - 2)), border)
        .render(Rect::new(0, y, width, 1), buffer);
    y + 1
}

pub fn screen(
    board: &Board,
    nav: &Navigation,
    entries: &[Entry],
    width: u16,
    height: u16,
    problem: Option<&str>,
) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    if width == 0 || height == 0 {
        return buffer;
    }
    let header_rows = header_rows(board, nav, width, height);
    if framed(board, nav, width, height) {
        draw_framed(board, nav, width, problem, &mut buffer);
    } else {
        let rows = header(board, nav, width, problem);
        for (y, row) in rows.into_iter().take(header_rows as usize).enumerate() {
            row.render(Rect::new(0, y as u16, width, 1), &mut buffer);
        }
    }
    if height == 1 {
        return buffer;
    }
    let body = height.saturating_sub(header_rows + 1);
    if nav.detail {
        if let Some(entry) = entries.get(nav.index) {
            let paragraph = Paragraph::new(entry.detail.clone()).wrap(Wrap { trim: false });
            let max = paragraph
                .line_count(width)
                .saturating_sub(body as usize)
                .min(u16::MAX as usize) as u16;
            paragraph
                .scroll((nav.detail_scroll.min(max), 0))
                .render(Rect::new(0, header_rows, width, body), &mut buffer);
        }
    } else if entries.is_empty() {
        if body > 0 {
            text_line("Nothing here yet.", DIM, header_rows, width, &mut buffer);
        }
    } else {
        let top = nav.index.saturating_sub(body.saturating_sub(1) as usize);
        for (offset, entry) in entries.iter().skip(top).take(body as usize).enumerate() {
            let y = header_rows + offset as u16;
            match entry.kind {
                Kind::Heading => {
                    let label = format!(" ▌{}", entry.label);
                    let tail = &entry.tail;
                    row_line(&label, tail, "", HEADING, Some(DIM), y, width, &mut buffer);
                    continue;
                }
                Kind::Note => {
                    let label = format!("     {}", entry.label);
                    let tail = &entry.tail;
                    row_line(&label, tail, "", DIM, Some(DIM), y, width, &mut buffer);
                    continue;
                }
                Kind::Gap => continue,
                Kind::Row => {}
            }
            let selected = top + offset == nav.index && nav.selected.is_some();
            let style = if selected {
                Style::new()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if entry.attention {
                Style::new().fg(Color::Yellow)
            } else {
                Style::default()
            };
            row_line(
                &format!("{} {}", if selected { "▸" } else { " " }, entry.label),
                &entry.tail,
                &entry.figures,
                style,
                None,
                y,
                width,
                &mut buffer,
            );
        }
    }
    let hints: &[&str] = if nav.detail {
        &[
            "Esc back · ↑↓ scroll · o go · v peek · ? help",
            "Esc back · ↑↓ scroll · ? help",
            "Esc back · ? help",
            "Esc",
        ]
    } else {
        &[
            "↑↓ select · Enter details · o go · v peek · Tab views · ? help",
            "↑↓ select · Enter details · Tab views · ? help",
            "Enter details · ? help",
            "? help",
            "?",
        ]
    };
    let footer = hints
        .iter()
        .find(|hint| Line::raw(**hint).width() <= width as usize)
        .copied()
        .unwrap_or("");
    text_line(footer, DIM, height - 1, width, &mut buffer);
    buffer
}

pub fn plain(buffer: &Buffer) -> String {
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "board_view_tests.rs"]
mod tests;
