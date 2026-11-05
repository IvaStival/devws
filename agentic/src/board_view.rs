//! Draws a `Board` into a buffer: boxes of lines when there is room, a few
//! lines when the pane is small. Pure: the terminal loop only copies the
//! buffer to the screen.

use crate::board::{AgentRow, Board, RoleRow, StepRow};
use crate::devws::{PaneState, Usage};
use crate::roles::Group;
use crate::state::StepStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Widget};

/// Below this many rows the board is the short view.
pub const COMPACT_BELOW: u16 = 18;

const KEYS: [(&str, &str); 7] = [
    ("Enter", "Leader"),
    ("1-9", "Agent"),
    ("p1-9", "Peek"),
    ("l", "Log"),
    ("z", "Zoom"),
    ("j/k", "Scroll"),
    ("q", "Quit"),
];

const TITLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const BORDER: Style = Style::new().fg(Color::Blue);
const DIM: Style = Style::new().fg(Color::DarkGray);
const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
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

fn dot(state: PaneState) -> Span<'static> {
    let color = match state {
        PaneState::Working => Color::Green,
        PaneState::Waiting => Color::Yellow,
        PaneState::Done => Color::Cyan,
        PaneState::Idle => Color::DarkGray,
        PaneState::Gone => Color::Red,
    };
    Span::styled("● ", Style::new().fg(color))
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

/// An agent's id, and its lane when it has one: `hammer-codex-1 [back]`.
fn agent_label(agent: &AgentRow) -> String {
    match &agent.lane {
        Some(lane) => format!("{} [{lane}]", agent.id),
        None => agent.id.clone(),
    }
}

fn agent_lines(
    row: &RoleRow,
    agent: &AgentRow,
    first: bool,
    name_width: usize,
    room: usize,
) -> Vec<Line<'static>> {
    let name = if first { row.name.as_str() } else { "" };
    let what = agent
        .now
        .clone()
        .unwrap_or_else(|| state_word(agent.state).to_string());
    let number = agent
        .number
        .map_or_else(|| "  ".to_string(), |number| format!("{number} "));
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{name:<name_width$}"), BOLD),
        Span::styled(number, TITLE),
        dot(agent.state),
        Span::raw(truncate(
            &format!("{}  {what}", agent_label(agent)),
            room.saturating_sub(name_width + 4),
        )),
    ])];
    if !agent.next.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(name_width + 4)),
            Span::styled(
                truncate(
                    &format!("next  {}", agent.next.join("; ")),
                    room.saturating_sub(name_width + 4),
                ),
                DIM,
            ),
        ]));
    }
    lines
}

fn role_lines(roles: &[RoleRow], room: usize) -> Vec<Line<'static>> {
    let name_width = roles.iter().map(|row| row.name.len()).max().unwrap_or(0) + 2;
    let mut lines = Vec::new();
    let mut group = None;
    for row in roles {
        if group != Some(row.group) {
            group = Some(row.group);
            lines.push(Line::styled(row.group.label().to_string(), TITLE));
        }
        if row.agents.is_empty() && row.group == Group::Cto {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<name_width$}", row.name), BOLD),
                Span::styled(
                    truncate(
                        &format!("{} · you talk to it (Enter)", row.program),
                        room.saturating_sub(name_width),
                    ),
                    DIM,
                ),
            ]));
            continue;
        }
        if row.agents.is_empty() {
            let detail = match &row.description {
                Some(description) => format!("{} · {description}", row.program),
                None => row.program.clone(),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<name_width$}", row.name), BOLD),
                Span::styled(
                    truncate(
                        &format!("not running  {detail}"),
                        room.saturating_sub(name_width),
                    ),
                    DIM,
                ),
            ]));
        }
        for (index, agent) in row.agents.iter().enumerate() {
            lines.extend(agent_lines(row, agent, index == 0, name_width, room));
        }
    }
    lines
}

fn step_lines(steps: &[StepRow], room: usize) -> Vec<Line<'static>> {
    steps
        .iter()
        .map(|step| {
            let attempts = if step.attempts > 1 {
                format!(" ×{}", step.attempts)
            } else {
                String::new()
            };
            let tail = format!("  {}{attempts}", step_word(step.status));
            Line::from(vec![
                Span::raw(truncate(
                    &format!("{} {}", step.id, step.title),
                    room.saturating_sub(tail.chars().count()),
                )),
                Span::styled(tail, DIM),
            ])
        })
        .collect()
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

fn header(board: &Board, now: &str, width: u16) -> Line<'static> {
    let left = format!("{}  {}", board.task, board.branch);
    let right = format!("now {now}");
    let gap = (width as usize).saturating_sub(left.chars().count() + right.chars().count());
    Line::from(vec![
        Span::styled(left, BOLD),
        Span::raw(" ".repeat(gap.max(1))),
        Span::styled(right, DIM),
    ])
}

/// The whole board, as tall as it needs to be.
fn full(board: &Board, width: u16, now: &str) -> Buffer {
    let room = (width as usize).saturating_sub(4);
    let mut sections: Vec<(String, Vec<Line<'static>>)> = vec![
        ("Accounts".into(), account_lines(board)),
        ("Usage".into(), usage_lines(board)),
        (
            "State".into(),
            vec![Line::raw(truncate(&board.summary, room))],
        ),
    ];
    if !board.questions.is_empty() {
        sections.push((
            format!("Questions ({})", board.questions.len()),
            board
                .questions
                .iter()
                .map(|question| Line::raw(truncate(question, room)))
                .collect(),
        ));
    }
    sections.push(("Who does what".into(), role_lines(&board.roles, room)));
    if !board.review.is_empty() {
        sections.push((
            format!("Review ({})", board.review.len()),
            step_lines(&board.review, room),
        ));
    }
    if !board.queue.is_empty() {
        sections.push((
            format!("Queue ({})", board.queue.len()),
            board
                .queue
                .iter()
                .map(|order| Line::raw(truncate(order, room)))
                .collect(),
        ));
    }
    if !board.done.is_empty() {
        sections.push((
            format!("Done ({})", board.done.len()),
            step_lines(&board.done, room),
        ));
    }

    let height: usize = 1 + sections
        .iter()
        .map(|(_, lines)| lines.len() + 3)
        .sum::<usize>();
    let area = Rect::new(0, 0, width, height as u16);
    let mut buffer = Buffer::empty(area);
    header(board, now, width).render(Rect::new(0, 0, width, 1), &mut buffer);
    let mut top = 1;
    for (title, lines) in sections {
        let rows = lines.len() as u16 + 2;
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(BORDER)
                    .title(Span::styled(format!(" {title} "), TITLE)),
            )
            .render(Rect::new(0, top, width, rows), &mut buffer);
        top += rows + 1;
    }
    buffer
}

/// A few lines for a small pane: the state, and each running agent.
fn compact(board: &Board, width: u16, now: &str) -> Vec<Line<'static>> {
    let room = width as usize;
    let mut lines = vec![
        header(board, now, width),
        Line::raw(truncate(&board.summary, room)),
    ];
    let mut idle = 0;
    for row in &board.roles {
        if row.agents.is_empty() {
            idle += 1;
        }
        for agent in &row.agents {
            let what = agent
                .now
                .clone()
                .unwrap_or_else(|| state_word(agent.state).to_string());
            lines.push(Line::from(vec![
                Span::styled(
                    agent
                        .number
                        .map_or_else(|| "  ".to_string(), |number| format!("{number} ")),
                    TITLE,
                ),
                dot(agent.state),
                Span::raw(truncate(
                    &format!("{}  {what}", agent_label(agent)),
                    room.saturating_sub(4),
                )),
            ]));
        }
    }
    if idle > 0 {
        lines.push(Line::styled(format!("{idle} roles not running"), DIM));
    }
    lines
}

fn key_line() -> Line<'static> {
    let mut spans = Vec::new();
    for (key, what) in KEYS {
        spans.push(Span::styled(key, TITLE));
        spans.push(Span::styled(format!(" {what}  "), DIM));
    }
    Line::from(spans)
}

/// How far the board scrolls in a pane of this size: past it, the end of the
/// board would leave the screen.
pub fn board_scroll_max(board: &Board, width: u16, height: u16) -> u16 {
    if height < COMPACT_BELOW {
        return 0;
    }
    full_height(board, width).saturating_sub(height.saturating_sub(1))
}

/// How far the log scrolls back from its newest line.
pub fn log_scroll_max(lines: usize, height: u16) -> u16 {
    let body = log_body(height) as usize;
    lines.saturating_sub(body).min(u16::MAX as usize) as u16
}

/// The log's rows: all but the key line and the position line.
fn log_body(height: u16) -> u16 {
    height.saturating_sub(2)
}

/// What the terminal shows: `scroll` rows into the board, the keys on the
/// last row. A pane under `COMPACT_BELOW` rows gets the short view.
pub fn screen(board: &Board, width: u16, height: u16, scroll: u16, now: &str) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut out = Buffer::empty(area);
    let body = height.saturating_sub(1);
    if height < COMPACT_BELOW {
        Paragraph::new(compact(board, width, now)).render(Rect::new(0, 0, width, body), &mut out);
    } else {
        let content = full(board, width, now);
        let rows = content.area.height;
        let scroll = scroll.min(rows.saturating_sub(body));
        for y in 0..body.min(rows) {
            for x in 0..width {
                out[(x, y)] = content[(x, y + scroll)].clone();
            }
        }
    }
    key_line().render(Rect::new(0, body, width, 1), &mut out);
    out
}

/// The log, newest at the bottom; `scroll` rows up from there. A dispatcher
/// problem, if there is one, shows on the row above the keys.
pub fn log_screen(
    log: &[String],
    width: u16,
    height: u16,
    scroll: u16,
    problem: Option<&str>,
) -> Buffer {
    let mut out = Buffer::empty(Rect::new(0, 0, width, height));
    let body = log_body(height);
    let scroll = scroll.min(log_scroll_max(log.len(), height));
    let end = log.len().saturating_sub(scroll as usize);
    let start = end.saturating_sub(body as usize);
    let lines: Vec<Line> = log[start..end]
        .iter()
        .map(|line| Line::raw(truncate(&crate::watch::format_line(line), width as usize)))
        .collect();
    Paragraph::new(lines).render(Rect::new(0, 0, width, body), &mut out);
    let position = match problem {
        Some(problem) => Line::styled(
            truncate(&format!("dispatcher: {problem}"), width as usize),
            Style::new().fg(Color::Red),
        ),
        None if log.is_empty() => Line::styled("Log: nothing yet", DIM),
        None => Line::styled(
            truncate(
                &format!(
                    "Log: lines {}–{end} of {}{}",
                    start + 1,
                    log.len(),
                    if scroll == 0 {
                        "  (newest; new lines appear here)".to_string()
                    } else {
                        format!("  ({scroll} newer below: j, G)")
                    }
                ),
                width as usize,
            ),
            DIM,
        ),
    };
    position.render(Rect::new(0, body, width, 1), &mut out);
    key_line().render(Rect::new(0, body + 1, width, 1), &mut out);
    out
}

/// How many rows the full board needs at this width.
pub fn full_height(board: &Board, width: u16) -> u16 {
    full(board, width, "").area.height
}

/// The buffer as text, one line per row, without the trailing blanks.
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
mod tests {
    use super::*;
    use crate::board::{Account, AgentRow};

    fn board() -> Board {
        Board {
            task: "feat-x".into(),
            branch: "feat/x".into(),
            summary: "Plan approved. 1 working.".into(),
            accounts: vec![Account {
                name: "claude".into(),
                five_hour: Some(40),
                week: None,
            }],
            roles: vec![
                RoleRow {
                    name: "executor".into(),
                    group: Group::Floor,
                    program: "claude".into(),
                    description: Some("Does one order".into()),
                    agents: vec![AgentRow {
                        number: Some(1),
                        id: "executor-1".into(),
                        pane: "%8".into(),
                        lane: None,
                        state: PaneState::Working,
                        now: Some("s1 add a file".into()),
                        next: vec!["s2 add a test".into()],
                    }],
                },
                RoleRow {
                    name: "tester".into(),
                    group: Group::Senior,
                    program: "codex · high".into(),
                    description: Some("Runs the tests".into()),
                    agents: vec![],
                },
            ],
            review: vec![StepRow {
                id: "s0".into(),
                title: "set up".into(),
                status: StepStatus::Failed,
                attempts: 2,
            }],
            queue: vec!["s2 add a test  for executor-1".into()],
            questions: vec!["q9 executor-1 → tester: which file?".into()],
            done: vec![],
            usage: vec![
                (
                    "leader".into(),
                    Usage {
                        input: 1200,
                        output: 3400,
                        cache: 2_500_000,
                        cost: 1.234,
                        unpriced: false,
                    },
                ),
                (
                    "executor-1".into(),
                    Usage {
                        input: 800,
                        output: 900,
                        cache: 90_000,
                        cost: 12.0,
                        unpriced: true,
                    },
                ),
            ],
        }
    }

    #[test]
    fn the_full_board_has_its_boxes_every_role_and_the_keys() {
        let text = plain(&screen(&board(), 80, 40, 0, "08:28"));
        for expected in [
            "feat-x  feat/x",
            "now 08:28",
            "Accounts",
            "claude        5 h 40%   week —",
            "Usage",
            "leader      in 1k     out 3k     cache 2.5M   $1.23",
            "executor-1  in 800    out 900    cache 90k    $12+",
            "team        in 2k     out 4k     cache 2.6M   $13+",
            "State",
            "Plan approved. 1 working.",
            "Who does what",
            "executor  1 ● executor-1  s1 add a file",
            "next  s2 add a test",
            "1-9 Agent",
            "p1-9 Peek",
            "tester    not running  codex · high · Runs the tests",
            "Review (1)",
            "s0 set up  failed ×2",
            "Queue (1)",
            "Enter Leader",
            "q Quit",
        ] {
            assert!(text.contains(expected), "missing '{expected}' in:\n{text}");
        }
        assert!(!text.contains("Done ("), "an empty box is left out");
    }

    #[test]
    fn roles_show_under_their_group_and_open_questions_get_a_box() {
        for width in [73, 120] {
            let text = plain(&screen(&board(), width, 60, 0, "08:28"));
            for expected in [
                "Questions (1)",
                "q9 executor-1 → tester: which file?",
                "Factory floor",
                "Seniors",
            ] {
                assert!(
                    text.contains(expected),
                    "missing '{expected}' at {width}:\n{text}"
                );
            }
            let floor = text.find("Factory floor").unwrap();
            assert!(
                floor < text.find("executor-1  s1").unwrap(),
                "the header comes first"
            );
        }
    }

    #[test]
    fn a_small_pane_gets_the_state_and_the_running_agents() {
        let text = plain(&screen(&board(), 60, 8, 0, "08:28"));
        assert!(text.contains("Plan approved. 1 working."), "{text}");
        assert!(text.contains("1 ● executor-1  s1 add a file"), "{text}");
        assert!(text.contains("1 roles not running"), "{text}");
        assert!(!text.contains("Who does what"), "{text}");
        assert!(text.lines().last().unwrap().contains("Enter Leader"));
    }

    #[test]
    fn scrolling_moves_the_boxes_up_and_stops_at_the_end() {
        let board = board();
        let height = 20;
        let rows = full_height(&board, 80);
        assert!(
            rows > height,
            "the test board must overflow {height}: {rows}"
        );
        let top = plain(&screen(&board, 80, height, 0, "x"));
        assert!(top.contains("feat-x"));
        let end = plain(&screen(&board, 80, height, 999, "x"));
        assert!(!end.contains("feat-x"));
        assert!(end.contains("Queue (1)"), "{end}");
    }

    #[test]
    fn the_log_scrolls_back_from_its_newest_line_and_stops_at_its_first() {
        let log: Vec<String> = (1..=30).map(|number| format!("line {number}")).collect();
        let newest = plain(&log_screen(&log, 60, 12, 0, None));
        assert!(
            newest.contains("line 30") && !newest.contains("line 20\n"),
            "{newest}"
        );
        assert!(newest.contains("lines 21–30 of 30  (newest"), "{newest}");

        let back = plain(&log_screen(&log, 60, 12, 5, None));
        assert!(
            back.contains("line 25") && !back.contains("line 26"),
            "{back}"
        );
        assert!(back.contains("(5 newer below"), "{back}");

        assert_eq!(log_scroll_max(log.len(), 12), 20);
        let first = plain(&log_screen(&log, 60, 12, 999, None));
        assert!(
            first.contains("line 1\n") && first.contains("lines 1–10 of 30"),
            "{first}"
        );
        assert!(plain(&log_screen(&[], 60, 12, 0, None)).contains("nothing yet"));
        assert_eq!(log_scroll_max(3, 12), 0, "a short log does not scroll");
    }

    #[test]
    fn the_board_scrolls_only_as_far_as_its_end() {
        let board = board();
        let max = board_scroll_max(&board, 80, 20);
        assert_eq!(max, full_height(&board, 80) - 19);
        assert_eq!(board_scroll_max(&board, 80, COMPACT_BELOW - 1), 0);
    }

    #[test]
    fn a_long_line_is_cut_with_an_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }
}
