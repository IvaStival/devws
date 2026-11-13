use super::*;
use crate::board::{Account, AgentRow, PermissionRow, QueueRow, RoleRow, SpecRow};
use crate::roles::Group;
use crate::state::Effect;
pub(crate) fn board() -> Board {
    Board {
        task: "feat-x".into(),
        branch: "feat/x".into(),
        leader: "claude · high".into(),
        leader_pane: Some("%2".into()),
        leader_state: Some(PaneState::Working),
        leader_usage: None,
        leader_permissions: "read-only · provider-managed · default".into(),
        leader_fallback: Some("codex · gpt-5.5 · standby".into()),
        approved: true,
        paused: false,
        permissions: vec![],
        steps: vec![],
        details: Default::default(),
        phase: "Approved".into(),
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
                permissions: "order-scoped write · workspace-write · acceptEdits".into(),
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
                    usage: None,
                    accepted: 0,
                    retries: 0,
                }],
            },
            RoleRow {
                name: "tester".into(),
                group: Group::Senior,
                permissions: "read-only · read-only · on-request".into(),
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
        queue: vec![QueueRow {
            id: "o2".into(),
            label: "s2 add a test  for executor-1".into(),
        }],
        questions: vec!["q9 executor-1 → tester: which file?".into()],
        done: vec![],
        done_specs: vec![],
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
fn overview_prioritizes_attention_and_keeps_it_visible_in_short_panes() {
    let board = board();
    let mut nav = Navigation::new(View::Overview);
    let entries = entries(&board, nav.view, false, &[], Some("cannot deliver"));
    nav.reconcile(&entries);
    let text = plain(&screen(
        &board,
        &nav,
        &entries,
        85,
        12,
        Some("cannot deliver"),
    ));
    assert!(text.contains("● 3 need you"), "{text}");
    assert!(text.contains("cannot deliver"), "{text}");
    assert!(!text.contains("cache 2.5M"), "{text}");
    let taller = plain(&screen(
        &board,
        &nav,
        &entries,
        85,
        20,
        Some("cannot deliver"),
    ));
    assert!(
        taller.contains("L  leader") && taller.contains("claude · high"),
        "the leader is the team's first row: {taller}"
    );
    assert!(
        entries
            .iter()
            .position(|entry| entry.id == "step:s0")
            .unwrap()
            < entries
                .iter()
                .position(|entry| entry.id == "agent:executor-1")
                .unwrap()
    );
}

#[test]
fn selection_survives_insertions_and_falls_back_when_removed() {
    let board = board();
    let mut rows = entries(&board, View::Agents, false, &[], None);
    let mut nav = Navigation::new(View::Agents);
    nav.reconcile(&rows);
    let selected = nav.selected.clone();
    let mut extra = rows[0].clone();
    extra.id = "agent:earlier".into();
    rows.insert(0, extra);
    nav.reconcile(&rows);
    assert_eq!(nav.selected, selected);
    assert_eq!(nav.index, 1);
    rows.remove(1);
    nav.reconcile(&rows);
    assert_eq!(nav.index, 0);
    assert_eq!(nav.selected.as_deref(), Some("agent:earlier"));
}

#[test]
fn every_agent_is_reachable_and_small_panes_scroll_to_selection() {
    let mut board = board();
    for number in 2..=15 {
        let mut agent = board.roles[0].agents[0].clone();
        agent.id = format!("executor-{number}");
        agent.number = None;
        board.roles[0].agents.push(agent);
    }
    let rows = entries(&board, View::Agents, false, &[], None);
    let mut nav = Navigation::new(View::Agents);
    nav.reconcile(&rows);
    nav.move_by(&rows, 14);
    assert_eq!(nav.selected.as_deref(), Some("agent:executor-15"));
    let text = plain(&screen(&board, &nav, &rows, 38, 8, None));
    assert!(text.contains("executor-15"), "{text}");
    assert!(!text.contains("executor-1 ·"), "{text}");
}

/// The test board with a long history: 99 of 101 steps accepted, the 99th
/// after a retry.
fn board_with_history() -> Board {
    let mut board = board();
    board.steps = (1..=101)
        .map(|number| StepRow {
            id: format!("p{number}"),
            title: format!("step number {number}"),
            status: if number <= 99 {
                StepStatus::Accepted
            } else {
                StepStatus::Ordered
            },
            attempts: if number == 99 { 2 } else { 1 },
        })
        .collect();
    board.done = board
        .steps
        .iter()
        .rev()
        .filter(|step| step.status == StepStatus::Accepted)
        .map(|step| StepRow {
            id: step.id.clone(),
            title: step.title.clone(),
            status: step.status,
            attempts: step.attempts,
        })
        .collect();
    board.done_specs = board
        .done
        .iter()
        .map(|step| SpecRow {
            id: step.id.clone(),
            version: format!("1.{}.0", &step.id[1..]),
            title: step.title.clone(),
            steps: vec![StepRow {
                id: step.id.clone(),
                title: step.title.clone(),
                status: step.status,
                attempts: step.attempts,
            }],
        })
        .collect();
    board
}

/// `input`/`output` tokens at `cost`.
fn used(input: u64, output: u64, cost: f64) -> Usage {
    Usage {
        input,
        output,
        cache: 0,
        cost,
        unpriced: false,
    }
}

#[test]
fn the_overview_groups_the_team_with_what_each_agent_costs() {
    let mut board = board_with_history();
    board.leader_usage = Some(used(1_500_000, 600_000, 12.1));
    let first = &mut board.roles[0].agents[0];
    first.usage = Some(used(600_000, 300_000, 6.2));
    first.accepted = 31;
    first.retries = 4;
    let mut second = first.clone();
    second.id = "executor-2".into();
    second.number = Some(2);
    second.state = PaneState::Done;
    second.now = None;
    second.usage = Some(used(400_000, 100_000, 4.8));
    second.accepted = 0;
    second.retries = 0;
    board.roles[0].agents.push(second);
    let rows = entries(&board, View::Overview, false, &[], None);

    let headings: Vec<_> = rows
        .iter()
        .filter(|entry| entry.kind == Kind::Heading)
        .map(|entry| entry.label.as_str())
        .collect();
    assert_eq!(
        headings,
        [
            "NEEDS YOU · 2",
            "CTO",
            "FLOOR · 2",
            "QUEUE · 1",
            "USAGE",
            "DONE · 99"
        ]
    );
    let ids: Vec<_> = rows.iter().map(|entry| entry.id.as_str()).collect();
    let place = |id: &str| {
        ids.iter()
            .position(|row| *row == id)
            .unwrap_or_else(|| panic!("no {id} in {ids:?}"))
    };
    for section in ["CTO", "FLOOR", "QUEUE", "USAGE", "DONE"] {
        assert_eq!(
            rows[place(&format!("heading:{section}")) - 1].kind,
            Kind::Gap,
            "a blank line before {section}"
        );
    }
    assert!(place("question:q9") < place("heading:CTO"));
    assert_eq!(ids[place("heading:CTO") + 1], "leader");
    assert!(place("heading:FLOOR") < place("agent:executor-1"));
    assert!(place("agent:executor-2") < place("queue:o2"));
    assert!(place("queue:o2") < place("heading:USAGE"));
    assert!(place("heading:USAGE") < place("heading:DONE"));
    assert_eq!(ids[place("heading:DONE") + 1], "spec:p99", "newest first");
    assert_eq!(*ids.last().unwrap(), "spec:p1");

    let leader = &rows[place("leader")];
    assert!(leader.label.starts_with("L  leader"), "{}", leader.label);
    assert!(leader.label.contains("claude · high"), "{}", leader.label);
    assert_eq!(leader.tail, "⠋ working");
    assert_eq!(leader.pane.as_deref(), Some("%2"));
    let agent = &rows[place("agent:executor-1")];
    assert!(agent.label.starts_with("1  executor-1"), "{}", agent.label);
    assert!(agent.label.contains("claude"), "{}", agent.label);
    assert_eq!(agent.tail, "⠋ working");
    let note = &rows[place("note:agent:executor-1")];
    assert_eq!(note.label, "└ now s1 add a file");
    assert_eq!(note.tail, "31 accepted · 4 retries");
    assert!(
        !ids.contains(&"note:agent:executor-2"),
        "nothing to say, no second line"
    );
    assert!(rows[place("queue:o2")].label.contains("s2 add a test"));
    assert_eq!(
        rows[place("spec:p99")].tail,
        "1 step · 1 retry",
        "retries end the row"
    );
    assert!(rows[place("spec:p99")].label.starts_with("✓ 1.99.0"));
    assert_eq!(rows[place("spec:p98")].tail, "1 step");

    // The USAGE card: each agent's tokens and cost, then the team's total.
    assert!(place("heading:USAGE") < place("usage:leader"));
    assert_eq!(ids[place("heading:DONE") - 2], "usage:team");
    let team = &rows[place("usage:team")];
    assert!(team.label.starts_with("team"), "{}", team.label);
    assert_eq!(team.tail, "$13+");
    assert!(
        rows[place("usage:leader")].detail.contains("Cache: 2.5M"),
        "the cache is in the details"
    );

    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    let text = plain(&screen(&board, &nav, &rows, 84, 60, None));
    let line = |text: &str, id: &str| {
        text.lines()
            .find(|line| line.contains(id))
            .unwrap_or_else(|| panic!("no {id}: {text}"))
            .to_string()
    };
    // Every section is a card, its count in the top edge.
    for top in [
        "╭─ NEEDS YOU · 2 ─",
        "╭─ CTO ─",
        "╭─ FLOOR · 2 ─",
        "╭─ USAGE ─",
    ] {
        assert!(text.contains(top), "no {top}: {text}");
    }
    let first = line(&text, "1  executor-1");
    let second = line(&text, "2  executor-2");
    assert!(
        first.starts_with("│ ") && first.ends_with(" │"),
        "{first:?}"
    );
    assert!(
        first.trim_end_matches(" │").ends_with("⠋ working"),
        "{first:?}"
    );
    assert_eq!(
        first.find("⠋ working").map(|at| at + "⠋ working".len()),
        second.find("✓ done").map(|at| at + "✓ done".len()),
        "states end at the same edge"
    );
    assert_eq!(first.chars().count(), 84);
    assert!(line(&text, "╭─ DONE · 99 ─").ends_with("newest first ─╮"));

    // Only DONE scrolls: the wheel reaches the oldest step and the cards
    // above it stay where they were.
    nav.move_by(&rows, isize::MAX);
    assert_eq!(nav.selected.as_deref(), Some("spec:p1"));
    let scrolled = plain(&screen(&board, &nav, &rows, 84, 60, None));
    assert!(scrolled.contains("1.1.0  step number 1"), "{scrolled}");
    assert!(!scrolled.contains("1.99.0  "), "{scrolled}");
    // The same lines, but for the selection marker, which moved to DONE.
    let above = |text: &str| {
        text.lines()
            .take_while(|line| !line.contains("DONE"))
            .map(|line| line.replace('▸', " "))
            .collect::<Vec<_>>()
    };
    assert_eq!(above(&text), above(&scrolled), "the fixed cards stay");
    assert!(scrolled.contains(" of 99 ─╯"), "{scrolled}");
    let narrow = plain(&screen(&board, &nav, &rows, 50, 60, None));
    let first = line(&narrow, "1  executor-1");
    assert!(
        first.trim_end_matches(" │").ends_with("⠋ working"),
        "narrow keeps the state: {first:?}"
    );
    assert!(first.chars().count() <= 50);
}

#[test]
fn the_done_card_scrolls_with_a_bar_and_too_short_a_pane_is_a_plain_list() {
    let board = board_with_history();
    let rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    let text = plain(&screen(&board, &nav, &rows, 70, 40, None));
    let done: Vec<_> = text
        .lines()
        .skip_while(|line| !line.contains("DONE"))
        .collect();
    let bottom = done.iter().find(|line| line.starts_with('╰')).unwrap();
    let shown = done.iter().filter(|line| line.contains("✓ 1.")).count();
    assert!(shown >= MIN_SCROLL_ROWS, "{text}");
    assert!(bottom.contains(&format!(" 1–{shown} of 99 ")), "{bottom:?}");
    assert_eq!(
        done.iter().filter(|line| line.ends_with("█ │")).count(),
        1.max(shown * shown / 99),
        "the thumb is as long as the part shown: {text}"
    );
    assert!(
        done[1].ends_with("█ │"),
        "the thumb starts at the top: {text}"
    );

    // Not enough room for the fixed cards and a few done steps: the board is
    // the plain list, scrolled to the selection.
    let short = plain(&screen(&board, &nav, &rows, 70, 16, None));
    assert!(!short.contains("╭─ CTO"), "{short}");
    assert!(short.contains("▌NEEDS YOU · 2"), "{short}");
    assert_eq!(
        list_rows(&rows),
        rows.iter().filter(|row| row.kind != Kind::Gap).count() + 6 - (99 - DONE_ROWS),
        "DONE counts its fixed height"
    );
}

#[test]
fn the_header_has_the_count_the_tabs_the_progress_and_limit_bars() {
    let mut board = board_with_history();
    board.accounts.push(crate::board::Account {
        name: "codex".into(),
        five_hour: Some(9),
        week: Some(87),
    });
    let rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    board.usage = vec![
        ("leader".into(), used(3_500_000, 1_100_000, 20.0)),
        ("executor".into(), used(1_000_000, 0, 11.4)),
    ];
    board.usage[1].1.cache = 474_000_000;
    let buffer = screen(&board, &nav, &rows, 84, 40, None);
    let text = plain(&buffer);
    let lines: Vec<&str> = text.lines().collect();
    // A rounded frame: the task and what needs you in its top edge.
    assert!(
        lines[0].starts_with("╭─ feat-x · feat/x "),
        "{:?}",
        lines[0]
    );
    assert!(lines[0].ends_with("● 2 need you ─╮"), "{:?}", lines[0]);
    let inside = lines
        .iter()
        .skip(1)
        .take_while(|line| line.starts_with('│'))
        .copied()
        .collect::<Vec<_>>();
    assert!(inside.iter().all(|line| line.ends_with('│')), "{inside:?}");
    let bottom = lines[inside.len() + 1];
    assert!(
        bottom.starts_with('╰') && bottom.ends_with('╯'),
        "{bottom:?}"
    );
    let row = |word: &str| {
        inside
            .iter()
            .find(|line| line.contains(word))
            .unwrap_or_else(|| panic!("no {word}: {inside:?}"))
            .to_string()
    };
    assert!(row("[overview]").contains("usage"));
    let progress = row("PROGRESS");
    assert!(progress.contains("99/101 · 98%"), "{progress:?}");
    assert!(
        progress.contains('⣿') && progress.contains('⣀'),
        "dots: {progress:?}"
    );
    // What the team used is the USAGE card's, not the header's.
    assert!(
        !inside.iter().any(|line| line.contains("COST")),
        "{inside:?}"
    );
    let usage = entries(&board, View::Overview, false, &[], None);
    let team = usage.iter().find(|entry| entry.id == "usage:team").unwrap();
    assert_eq!(team.tail, "$31");
    assert!(team.label.contains("4.5M in    1.1M out"), "{}", team.label);
    let claude = row("claude ");
    assert!(claude.contains("5h ▓▓▓▓░░░░░░  40%"), "{claude:?}");
    assert!(claude.contains("wk ░░░░░░░░░░    —"), "{claude:?}");
    let codex_row = lines
        .iter()
        .position(|line| line.contains("codex "))
        .unwrap();
    let codex = lines[codex_row];
    assert!(codex.contains("5h ▓░░░░░░░░░   9%"), "{codex:?}");
    assert!(codex.contains("wk ▓▓▓▓▓▓▓▓▓░  87%"), "{codex:?}");
    // 87 % used is red, 9 % green, as the devws header colors them.
    let column = |at: usize| codex[..at].chars().count() as u16;
    let week_bar = column(codex.find("wk ▓").unwrap() + 3);
    let five_hour_bar = column(codex.find("5h ▓").unwrap() + 3);
    let color = |x: u16| buffer[(x, codex_row as u16)].fg;
    assert_eq!(color(week_bar), Color::Red);
    assert_eq!(color(five_hour_bar), Color::Green);
    assert!(!text.contains("Leader:"), "{text}");
    assert!(!text.contains("Plan approved"), "{text}");

    let narrow = plain(&screen(&board, &nav, &rows, 40, 40, None));
    for line in narrow.lines() {
        assert!(line.chars().count() <= 40, "{line:?}");
    }
    assert!(
        narrow.lines().next().unwrap().contains("2 need you"),
        "{narrow}"
    );
    // Too short for the frame: the task and what needs you still show.
    let short = plain(&screen(&board, &nav, &rows, 84, 4, None));
    assert!(
        short.lines().next().unwrap().contains("2 need you"),
        "{short}"
    );
    assert!(!narrow.contains("wk "), "the week bar goes first: {narrow}");

    board.questions.clear();
    board.review.clear();
    let calm = plain(&screen(&board, &nav, &rows, 84, 40, None));
    assert!(!calm.lines().next().unwrap().contains("need you"), "{calm}");
}

#[test]
fn every_step_row_starts_with_its_mark_and_ends_with_its_state() {
    let mut board = board();
    let step = |id: &str, status| StepRow {
        id: id.into(),
        title: format!("title of {id}"),
        status,
        attempts: 1,
    };
    board.steps = vec![
        step("a", StepStatus::Accepted),
        step("o", StepStatus::Ordered),
        step("r", StepStatus::Reported),
        step("f", StepStatus::Failed),
        step("b", StepStatus::Blocked),
        step("x", StepStatus::Rejected),
    ];
    let rows = entries(&board, View::Steps, true, &[], None);
    let shown: Vec<(&str, &str)> = rows
        .iter()
        .map(|entry| (entry.label.as_str(), entry.tail.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("✓ a  title of a", "accepted"),
            ("⠋ o  title of o", "ordered"),
            ("● r  title of r", "to review"),
            ("⚠ f  title of f", "failed"),
            ("⚠ b  title of b", "blocked"),
            ("⚠ x  title of x", "sent back"),
        ]
    );
    assert_eq!(
        entries(&board, View::Steps, false, &[], None).len(),
        5,
        "c still hides the accepted ones"
    );
}

#[test]
fn quiet_sections_are_left_out() {
    let mut board = board();
    board.questions.clear();
    board.review.clear();
    board.queue.clear();
    let rows = entries(&board, View::Overview, false, &[], None);
    assert!(!rows.iter().any(|entry| entry.id == "heading:NEEDS YOU"));
    assert!(!rows.iter().any(|entry| entry.id == "queue"));
    assert!(!rows.iter().any(|entry| entry.id == "heading:DONE"));
    assert!(
        !rows.iter().any(|entry| entry.id == "heading:SENIORS"),
        "no agents, no section"
    );
    assert!(rows.iter().any(|entry| entry.id == "heading:CTO"));
}

#[test]
fn the_selection_steps_over_section_headings() {
    let row = |id: &str| Entry::text(id.into(), id.into(), String::new(), false);
    let rows = vec![
        Entry::section("NEEDS YOU", None),
        row("a"),
        Entry::section("TEAM", None),
        Entry::section("QUEUED", None),
        row("b"),
        row("c"),
        Entry::section("DONE", None),
    ];
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    assert_eq!(
        nav.selected.as_deref(),
        Some("a"),
        "the first row, not a heading"
    );
    nav.move_by(&rows, 1);
    assert_eq!(
        nav.selected.as_deref(),
        Some("b"),
        "down skips both headings"
    );
    nav.move_by(&rows, -1);
    assert_eq!(nav.selected.as_deref(), Some("a"), "up skips them too");
    nav.move_by(&rows, -1);
    assert_eq!(
        nav.selected.as_deref(),
        Some("a"),
        "nothing above the first row"
    );
    nav.move_by(&rows, isize::MAX);
    assert_eq!(
        nav.selected.as_deref(),
        Some("c"),
        "the end is the last row, not DONE"
    );
    nav.move_by(&rows, isize::MIN);
    assert_eq!(nav.selected.as_deref(), Some("a"));
    nav.move_by(&rows, 2);
    assert_eq!(
        nav.selected.as_deref(),
        Some("c"),
        "a page counts rows, not headings"
    );

    let only_headings = vec![Entry::section("TEAM", None)];
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&only_headings);
    assert_eq!(nav.selected, None);
    nav.move_by(&only_headings, 1);
    assert_eq!(nav.selected, None);
}

#[test]
fn an_agents_second_line_and_the_gaps_are_never_selected() {
    let row = |id: &str| Entry::text(id.into(), id.into(), String::new(), false);
    let rows = vec![
        Entry::gap("top"),
        Entry::section("FLOOR", None),
        row("agent:a"),
        Entry::note("agent:a", "└ now s1"),
        Entry::gap("floor"),
        row("agent:b"),
        Entry::note("agent:b", "└ 3 accepted"),
    ];
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    assert_eq!(nav.selected.as_deref(), Some("agent:a"));
    nav.move_by(&rows, 1);
    assert_eq!(
        nav.selected.as_deref(),
        Some("agent:b"),
        "its note and the gap are skipped"
    );
    nav.move_by(&rows, isize::MAX);
    assert_eq!(
        nav.selected.as_deref(),
        Some("agent:b"),
        "the last note is not the end"
    );
    assert!(
        rows.iter()
            .map(|entry| &entry.id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == rows.len(),
        "ids stay unique"
    );
}

#[test]
fn card_frames_use_the_menu_accent_and_success_marks_stay_green() {
    let board = board_with_history();
    let rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    let rendered = screen(&board, &nav, &rows, 84, 60, None);
    assert!(
        rendered
            .content
            .iter()
            .any(|cell| { cell.symbol() == "╭" && cell.fg == Color::Indexed(110) }),
        "card frames must match the menu accent"
    );
    assert!(
        !rendered.content.iter().any(|cell| {
            matches!(cell.symbol(), "╭" | "╰" | "─" | "│") && cell.fg == Color::Green
        }),
        "green is reserved for status"
    );
    assert!(
        rendered
            .content
            .iter()
            .any(|cell| { cell.symbol() == "✓" && cell.fg == Color::Green }),
        "successful steps keep their status color"
    );
}

#[test]
fn details_open_in_a_titled_dialog_over_the_existing_board() {
    let board = board();
    let mut rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    rows[nav.index].label = "Selected question".into();
    rows[nav.index].detail = "Complete question context".into();
    let background = screen(&board, &nav, &rows, 84, 40, None);
    nav.detail = true;
    let rendered = screen(&board, &nav, &rows, 84, 40, None);
    let text = plain(&rendered);
    assert!(text.contains("Selected question"), "{text}");
    assert!(text.contains("Complete question context"), "{text}");
    assert!(text.contains("Esc/q close"), "{text}");
    for row in 0..40 {
        assert_eq!(
            rendered[(0, row)],
            background[(0, row)],
            "board behind the dialog"
        );
        assert_eq!(
            rendered[(83, row)],
            background[(83, row)],
            "board behind the dialog"
        );
    }
}

#[test]
fn a_refresh_keeps_the_detail_target_or_closes_it_if_removed() {
    let mut rows = vec![
        Entry::text("first".into(), "First".into(), "One".into(), false),
        Entry::text("second".into(), "Second".into(), "Two".into(), false),
    ];
    let mut nav = Navigation::new(View::Activity);
    nav.reconcile(&rows);
    nav.detail = true;
    nav.detail_scroll = 12;
    rows.swap(0, 1);
    nav.reconcile(&rows);
    assert_eq!(nav.selected.as_deref(), Some("first"));
    assert_eq!(nav.index, 1);
    assert!(nav.detail);
    assert_eq!(nav.detail_scroll, 12, "refresh preserves scrolling");
    rows.pop();
    nav.reconcile(&rows);
    assert!(
        !nav.detail,
        "never show a different entry in the open dialog"
    );
    assert_eq!(nav.detail_scroll, 0);
}

/// The text area of the details dialog as `screen` draws it: next to the
/// selected row when it shows.
fn drawn_body(
    board: &Board,
    nav: &mut Navigation,
    rows: &[Entry],
    width: u16,
    height: u16,
) -> Rect {
    let open = std::mem::replace(&mut nav.detail, false);
    let buffer = screen(board, nav, rows, width, height, None);
    nav.detail = open;
    let anchor = (0..height).find(|&y| {
        (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .contains('▸')
    });
    dialog_body_of(dialog_area(&rows[nav.index].detail, width, height, anchor))
}

#[test]
fn dialog_wrapping_and_end_scrolling_reach_the_last_line_after_resize() {
    let board = board();
    let mut rows = entries(&board, View::Agents, false, &[], None);
    let mut nav = Navigation::new(View::Agents);
    nav.reconcile(&rows);
    nav.detail = true;
    rows[nav.index].detail = format!("{}\nFINAL LINE", "wide context ".repeat(80));
    for (width, height) in [(84, 30), (24, 12), (12, 8), (5, 5), (84, 30)] {
        nav.detail_scroll = dialog_scroll_max(&rows[nav.index].detail, width, height);
        let rendered = screen(&board, &nav, &rows, width, height, None);
        let body = drawn_body(&board, &mut nav, &rows, width, height);
        let text = (body.y..body.bottom())
            .map(|row| {
                (body.x..body.right())
                    .map(|column| rendered[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("");
        // The narrowest panes split words over multiple lines.
        assert!(
            text.ends_with("FINAL LINE") || text.trim_end().ends_with("LINE"),
            "{width}x{height}: {text}"
        );
    }
}

#[test]
fn details_wrap_without_losing_text_and_tiny_sizes_do_not_panic() {
    let mut board = board();
    board
        .details
        .insert("agent:executor-1".into(), "full context ".repeat(80));
    let rows = entries(&board, View::Agents, false, &[], None);
    let mut nav = Navigation::new(View::Agents);
    nav.reconcile(&rows);
    nav.detail = true;
    for width in [0, 1, 12, 38, 85] {
        for height in [0, 1, 3, 8, 59] {
            let rendered = screen(&board, &nav, &rows, width, height, None);
            assert_eq!(rendered.area.width, width);
            assert_eq!(rendered.area.height, height);
        }
    }
    assert!(rows[0].detail.contains(&"full context ".repeat(80)));
}

#[test]
fn steps_filter_history_and_role_catalog_is_secondary() {
    let mut board = board();
    board.steps = vec![StepRow {
        id: "done".into(),
        title: "finished".into(),
        status: StepStatus::Accepted,
        attempts: 1,
    }];
    assert!(entries(&board, View::Steps, false, &[], None).is_empty());
    assert_eq!(entries(&board, View::Steps, true, &[], None).len(), 1);
    let active = entries(&board, View::Agents, false, &[], None);
    assert!(!active.iter().any(|entry| entry.id == "role:tester"));
    assert!(
        entries(&board, View::Agents, true, &[], None)
            .iter()
            .any(|entry| entry.id == "role:tester")
    );
}

#[test]
fn the_shortest_layout_prioritizes_attention_counts() {
    let board = board();
    let nav = Navigation::new(View::Overview);
    let entries = entries(&board, nav.view, false, &[], None);
    for height in [1, 2, 3, 4, 7] {
        let text = plain(&screen(&board, &nav, &entries, 38, height, None));
        assert!(text.contains("2 need you"), "height {height}: {text}");
    }
}

#[test]
fn activity_details_preserve_multiline_event_text() {
    let full = format!("{}\nsecond line of the report", "x".repeat(300));
    let log = vec![serde_json::json!({"kind":"report", "at":1, "text":full}).to_string()];
    let entries = entries(&board(), View::Activity, false, &log, None);
    assert!(entries[0].detail.contains(&full));
    assert!(!entries[0].label.contains("second line"));
}

#[test]
fn shortcut_hints_fit_the_available_width() {
    let board = board();
    let nav = Navigation::new(View::Agents);
    let entries = entries(&board, nav.view, false, &[], None);
    for width in [6, 12, 38, 65, 85, 100, 130] {
        let text = plain(&screen(&board, &nav, &entries, width, 8, None));
        let footer = text.lines().last().unwrap();
        assert!(footer.contains('?'), "width {width}: {footer}");
        assert!(
            footer.chars().count() <= width as usize,
            "width {width}: {footer}"
        );
    }
}

#[test]
fn the_leaders_details_show_how_it_runs_and_what_waits_for_you() {
    let rows = entries(&board(), View::Overview, false, &[], None);
    let leader = rows.iter().find(|row| row.id == "leader").unwrap();
    for line in [
        "The leader: claude · high",
        "Pane: %2 · working",
        "Permissions: read-only · provider-managed · default",
        "Fallback: codex · gpt-5.5 · standby",
        "Plan approved: yes",
        "Paused: no",
        "Pending permissions: none",
    ] {
        assert!(leader.detail.contains(line), "{line}\n{}", leader.detail);
    }
    assert!(
        !rows.iter().any(|row| row.id.starts_with("permission:")),
        "nothing to approve, no row"
    );
}

#[test]
fn a_pending_leader_call_needs_you_with_its_exact_arguments() {
    let mut board = board();
    board.paused = true;
    board.permissions = vec![PermissionRow {
        id: "p7".into(),
        tool: "stop_agent".into(),
        effect: Effect::Destructive,
        arguments: r#"{"agent":"executor-1"}"#.into(),
    }];
    let quiet = attention(&self::board(), None);
    assert_eq!(attention(&board, None), quiet + 1);

    let rows = entries(&board, View::Overview, false, &[], None);
    let place = |id: &str| rows.iter().position(|row| row.id == id).unwrap();
    assert!(place("heading:NEEDS YOU") < place("permission:p7"));
    assert!(place("permission:p7") < place("heading:CTO"));
    let request = &rows[place("permission:p7")];
    assert!(request.attention);
    assert_eq!(
        request.label,
        r#"⚠ p7 stop_agent · destructive · {"agent":"executor-1"}"#
    );
    assert!(request.detail.contains("Effect: destructive"));
    assert!(
        request
            .detail
            .contains(r#"Arguments: {"agent":"executor-1"}"#)
    );

    let leader = &rows[place("leader")];
    assert!(leader.detail.contains("Paused: yes"), "{}", leader.detail);
    assert!(
        leader.detail.contains(
            "Pending permissions: 1\n  p7 stop_agent · destructive · {\"agent\":\"executor-1\"}"
        ),
        "{}",
        leader.detail
    );
}

#[test]
fn the_done_card_keeps_its_fixed_height_in_a_tall_pane() {
    let board = board_with_history();
    let rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    let text = plain(&screen(&board, &nav, &rows, 84, 200, None));
    let done: Vec<_> = text
        .lines()
        .skip_while(|line| !line.contains("╭─ DONE"))
        .take_while(|line| !line.starts_with('╰'))
        .collect();
    assert_eq!(done.len() - 1, DONE_ROWS, "{text}");
    assert!(text.contains(&format!(" 1–{DONE_ROWS} of 99 ─╯")), "{text}");
}

#[test]
fn a_finished_spec_is_one_row_and_its_details_list_its_steps() {
    let mut board = board();
    let step = |id: &str, attempts| StepRow {
        id: id.into(),
        title: format!("title {id}"),
        status: StepStatus::Accepted,
        attempts,
    };
    board.done_specs = vec![SpecRow {
        id: "H2f".into(),
        version: "1.78.0".into(),
        title: "Plan H2f replay".into(),
        steps: vec![step("H2f-execution", 1), step("H2f.1", 3), step("H2f.2", 1)],
    }];
    let rows = entries(&board, View::Overview, false, &[], None);
    let heading = rows.iter().find(|row| row.id == "heading:DONE").unwrap();
    assert_eq!(heading.label, "DONE · 1");
    assert_eq!(heading.tail, "3 steps · newest first");
    let spec = rows.iter().find(|row| row.id == "spec:H2f").unwrap();
    assert_eq!(spec.label, "✓ 1.78.0  Plan H2f replay");
    assert_eq!(spec.tail, "3 steps · 2 retries");
    for line in [
        "1.78.0 Plan H2f replay (H2f)",
        "✓ 1.78.1  title H2f-execution (H2f-execution)",
        "✓ 1.78.2  title H2f.1 ×3 (H2f.1)",
    ] {
        assert!(spec.detail.contains(line), "{line}\n{}", spec.detail);
    }
    assert!(
        !rows.iter().any(|row| row.id.starts_with("step:H2f")),
        "its steps are not rows of their own"
    );
}

#[test]
fn a_dialog_is_as_tall_as_its_content_and_opens_next_to_its_row() {
    let row_of = |buffer: &Buffer, y: u16| {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let selected_y = |buffer: &Buffer| {
        (0..buffer.area.height)
            .find(|&y| row_of(buffer, y).contains('▸'))
            .unwrap()
    };
    let (width, height) = (84, 60);

    // A row near the top: the details open just below it.
    let board = board();
    let mut rows = entries(&board, View::Agents, false, &[], None);
    let mut nav = Navigation::new(View::Agents);
    nav.reconcile(&rows);
    rows[nav.index].detail = "first line\nsecond line".into();
    let row = selected_y(&screen(&board, &nav, &rows, width, height, None));
    nav.detail = true;
    let rendered = screen(&board, &nav, &rows, width, height, None);
    assert!(
        row_of(&rendered, row).contains('▸'),
        "the row stays in sight"
    );
    assert!(row_of(&rendered, row + 1).trim_start().starts_with('╭'));
    assert!(row_of(&rendered, row + 2).contains("first line"));
    assert!(row_of(&rendered, row + 3).contains("second line"));
    assert!(
        row_of(&rendered, row + 4).contains("Esc/q close"),
        "the hint row"
    );
    assert!(
        row_of(&rendered, row + 5).trim_start().starts_with('╰'),
        "as tall as its content"
    );
    assert_eq!(
        dialog_body(&rows[nav.index].detail, width, height).height,
        2
    );

    // The last done spec, at the bottom of a pane it fills: the details open
    // above it.
    let height = 40;
    let board = board_with_history();
    let rows = entries(&board, View::Overview, false, &[], None);
    let mut nav = Navigation::new(View::Overview);
    nav.reconcile(&rows);
    nav.move_by(&rows, isize::MAX);
    let row = selected_y(&screen(&board, &nav, &rows, width, height, None));
    nav.detail = true;
    let rendered = screen(&board, &nav, &rows, width, height, None);
    assert!(
        row_of(&rendered, row).contains('▸'),
        "the row stays in sight"
    );
    assert!(
        row_of(&rendered, row - 1).chars().nth(2) == Some('╰'),
        "row {row}:\n{}",
        plain(&rendered)
    );

    let long = "line\n".repeat(200);
    assert_eq!(
        dialog_body(&long, width, height).height,
        height - 2 - 2 - 1,
        "long details still take the pane less its margin, borders and hints"
    );
}

#[test]
fn an_idle_agent_shows_its_counts_where_its_step_would_be() {
    let mut board = board();
    let idle = &mut board.roles[0].agents[0];
    idle.now = None;
    idle.accepted = 11;
    idle.retries = 3;
    let rows = entries(&board, View::Overview, false, &[], None);
    let note = rows
        .iter()
        .find(|row| row.id == "note:agent:executor-1")
        .unwrap();
    assert_eq!(note.label, "└ 11 accepted · 3 retries");
    assert_eq!(note.tail, "", "nothing left alone at the far edge");
}
