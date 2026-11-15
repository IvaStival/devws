//! A team run measured after the fact, so two ways of working (legacy and
//! reviewed) can be compared on the same fixed scope: how much was right the
//! first time, why the rest was corrected, how long things waited, what the
//! user had to do and what it cost. One run proves nothing; compare many.
use crate::devws::Usage;
use crate::log::{Entry, Kind};
use crate::reviews::{Category, Verdict};
use crate::state::{State, StepStatus};
use crate::task::Task;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub task: String,
    pub workflow: crate::execution::Workflow,
    /// From the first log entry to the last.
    pub elapsed_seconds: u64,
    pub steps: u64,
    pub accepted: u64,
    /// Accepted with one order and no review asking for changes.
    pub first_pass: u64,
    /// Orders after a step's first.
    pub corrective_orders: u64,
    pub reviews: u64,
    pub reviews_asking_changes: u64,
    /// Blocking review findings by category, plus failed candidate checks as
    /// integration defects.
    pub corrections: BTreeMap<Category, u64>,
    pub candidate_check_failures: u64,
    /// Seconds by timing category, summed over agents (they overlap).
    pub busy_seconds: BTreeMap<String, u64>,
    /// Seconds anything recorded was running: overlapping work counted once.
    pub recorded_elapsed_seconds: u64,
    /// Older tasks lack most intervals: their times are lower bounds.
    pub unknown_historical_timing: bool,
    pub interventions: Interventions,
    pub milestones_ready: Vec<String>,
    /// None when the team's usage could not be read.
    pub usage: Option<Usage>,
}

/// What the run needed from a person.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Interventions {
    pub escalations: u64,
    pub approvals: u64,
    pub permission_requests: u64,
    pub attempt_allowances: u64,
}

impl Report {
    pub fn first_pass_rate(&self) -> Option<f64> {
        (self.accepted > 0).then(|| self.first_pass as f64 / self.accepted as f64)
    }
}

/// The report of a task's state and log; `usage` is the team's total.
pub fn build(
    task: &str,
    state: &State,
    log: &[Entry],
    milestones_ready: Vec<String>,
    usage: Option<Usage>,
) -> Report {
    let accepted: Vec<_> = state
        .steps
        .iter()
        .filter(|step| step.status == StepStatus::Accepted)
        .collect();
    let first_pass = accepted
        .iter()
        .filter(|step| {
            step.attempts == 1 && crate::reviews::changes_requested(state, &step.id) == 0
        })
        .count();
    let mut corrections: BTreeMap<Category, u64> = BTreeMap::new();
    for finding in state
        .reviews
        .iter()
        .filter(|review| review.verdict.is_some())
        .flat_map(|review| &review.findings)
        .filter(|finding| finding.blocking)
    {
        if let Some(category) = finding.category {
            *corrections.entry(category).or_default() += 1;
        }
    }
    let candidate_check_failures = count(log, |entry| {
        entry.kind == Kind::Check && entry.text.starts_with("candidate check failed")
    });
    if candidate_check_failures > 0 {
        *corrections.entry(Category::IntegrationDefect).or_default() += candidate_check_failures;
    }
    let timing = state.timing.summary();
    let first = log.iter().map(|entry| entry.at).min().unwrap_or(0);
    let last = log.iter().map(|entry| entry.at).max().unwrap_or(0);
    Report {
        task: task.into(),
        workflow: state.workflow,
        elapsed_seconds: last - first,
        steps: state.steps.len() as u64,
        accepted: accepted.len() as u64,
        first_pass: first_pass as u64,
        corrective_orders: state
            .steps
            .iter()
            .map(|step| u64::from(step.attempts.saturating_sub(1)))
            .sum(),
        reviews: state
            .reviews
            .iter()
            .filter(|review| review.verdict.is_some())
            .count() as u64,
        reviews_asking_changes: state
            .reviews
            .iter()
            .filter(|review| review.verdict == Some(Verdict::ChangesRequested))
            .count() as u64,
        corrections,
        candidate_check_failures,
        busy_seconds: timing.aggregate_seconds,
        recorded_elapsed_seconds: timing.elapsed_union_seconds,
        unknown_historical_timing: timing.unknown_historical_timing,
        interventions: Interventions {
            escalations: count(log, |entry| entry.kind == Kind::Escalate),
            approvals: count(log, |entry| entry.kind == Kind::Approve),
            permission_requests: state.permissions.len() as u64,
            attempt_allowances: state.attempt_limits.len() as u64,
        },
        milestones_ready,
        usage,
    }
}

fn count(log: &[Entry], matches: impl Fn(&Entry) -> bool) -> u64 {
    log.iter().filter(|entry| matches(entry)).count() as u64
}

/// The report of a task, with its team's usage when devws can tell it.
pub fn report(task: &Task, devws: &dyn crate::devws::Devws) -> Result<Report> {
    let state = task.state()?;
    let log: Vec<Entry> = task
        .log_lines()?
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let mut milestones_ready = Vec::new();
    for id in state.application.milestones.keys() {
        if crate::milestones::ready(task, id)? {
            milestones_ready.push(id.clone());
        }
    }
    let usage = crate::board_usage::UsageReader::new(std::time::Duration::ZERO)
        .of_team(task, devws)
        .ok()
        .map(|team| crate::board_usage::total(&team.by_role));
    Ok(build(
        &task.config()?.id,
        &state,
        &log,
        milestones_ready,
        usage,
    ))
}

/// The report as lines of `name  value`.
fn lines(report: &Report) -> Vec<(String, String)> {
    let rate = report
        .first_pass_rate()
        .map_or("-".to_string(), |rate| format!("{:.0}%", rate * 100.0));
    let mut lines = vec![
        (
            "workflow".into(),
            format!("{:?}", report.workflow).to_lowercase(),
        ),
        ("elapsed".into(), duration(report.elapsed_seconds)),
        (
            "accepted steps".into(),
            format!("{} of {}", report.accepted, report.steps),
        ),
        (
            "first pass".into(),
            format!("{} ({rate})", report.first_pass),
        ),
        (
            "corrective orders".into(),
            report.corrective_orders.to_string(),
        ),
        (
            "reviews (asking changes)".into(),
            format!("{} ({})", report.reviews, report.reviews_asking_changes),
        ),
        (
            "candidate check failures".into(),
            report.candidate_check_failures.to_string(),
        ),
    ];
    for (category, number) in &report.corrections {
        lines.push((format!("  correction: {category:?}"), number.to_string()));
    }
    for (category, seconds) in &report.busy_seconds {
        lines.push((format!("busy: {category}"), duration(*seconds)));
    }
    lines.push((
        "recorded elapsed (overlap once)".into(),
        duration(report.recorded_elapsed_seconds)
            + if report.unknown_historical_timing {
                " (lower bound: older history has no times)"
            } else {
                ""
            },
    ));
    let people = &report.interventions;
    lines.push((
        "user: escalations / approvals".into(),
        format!("{} / {}", people.escalations, people.approvals),
    ));
    lines.push((
        "user: permissions / more attempts".into(),
        format!(
            "{} / {}",
            people.permission_requests, people.attempt_allowances
        ),
    ));
    lines.push((
        "milestones ready".into(),
        if report.milestones_ready.is_empty() {
            "none".into()
        } else {
            report.milestones_ready.join(", ")
        },
    ));
    lines.push((
        "tokens in / out / cache".into(),
        report.usage.map_or("unknown".into(), |usage| {
            format!("{} / {} / {}", usage.input, usage.output, usage.cache)
        }),
    ));
    lines.push((
        "cost at API prices".into(),
        report.usage.map_or("unknown".into(), |usage| {
            format!(
                "${:.2}{}",
                usage.cost,
                if usage.unpriced { " or more" } else { "" }
            )
        }),
    ));
    lines
}

pub fn render(report: &Report) -> String {
    let lines = lines(report);
    let width = lines.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut text = format!("{}\n", report.task);
    for (name, value) in lines {
        text.push_str(&format!("  {name:<width$}  {value}\n"));
    }
    text
}

/// Two runs side by side, a line per measure.
pub fn compare(first: &Report, second: &Report) -> String {
    let (left, right) = (lines(first), lines(second));
    let width = left.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let column = left
        .iter()
        .map(|(_, value)| value.len())
        .chain([first.task.len()])
        .max()
        .unwrap_or(0);
    let mut text = format!(
        "  {:<width$}  {:<column$}  {}\n",
        "", first.task, second.task
    );
    let mut names: Vec<&String> = left.iter().map(|(name, _)| name).collect();
    for (name, _) in &right {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let value = |lines: &[(String, String)], name: &str| {
        lines
            .iter()
            .find(|(found, _)| found == name)
            .map_or("-".to_string(), |(_, value)| value.clone())
    };
    for name in names {
        text.push_str(&format!(
            "  {name:<width$}  {:<column$}  {}\n",
            value(&left, name),
            value(&right, name)
        ));
    }
    text
}

fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reviews::{Finding, Review};
    use crate::state::{Order, OrderStatus, Step};

    fn step(id: &str, status: StepStatus, attempts: u32) -> Step {
        Step {
            id: id.into(),
            title: id.into(),
            status,
            attempts,
            spec: None,
            review_since: None,
        }
    }

    fn order(id: &str, step: &str) -> Order {
        Order {
            id: id.into(),
            step: step.into(),
            agent: "a".into(),
            text: String::new(),
            allowed: vec![],
            forbidden: vec![],
            check: None,
            base: String::new(),
            status: OrderStatus::Closed,
            delivered: true,
            stalled: false,
            plan: None,
        }
    }

    fn finding(category: Option<Category>, blocking: bool) -> Finding {
        Finding {
            file: "a.rs".into(),
            line: 1,
            severity: "high".into(),
            blocking,
            rationale: "why".into(),
            resolution: None,
            category,
        }
    }

    fn entry(at: u64, kind: Kind, text: &str) -> Entry {
        Entry {
            at,
            kind,
            agent: None,
            step: None,
            text: text.into(),
        }
    }

    fn run() -> State {
        let mut state = State {
            steps: vec![
                step("S01-B01", StepStatus::Accepted, 1),
                step("S01-F01", StepStatus::Accepted, 1),
                step("S01-B02", StepStatus::Accepted, 3),
                step("S01-D01", StepStatus::Ordered, 1),
            ],
            orders: vec![order("o1", "S01-B01"), order("o2", "S01-F01")],
            reviews: vec![
                Review {
                    id: "r1".into(),
                    order: "o2".into(),
                    verdict: Some(Verdict::ChangesRequested),
                    findings: vec![
                        finding(Some(Category::TestWeakening), true),
                        finding(Some(Category::ImplementationDefect), false),
                    ],
                    ..Review::default()
                },
                Review {
                    id: "r2".into(),
                    order: "o2".into(),
                    verdict: Some(Verdict::Approved),
                    ..Review::default()
                },
                Review {
                    id: "r3".into(),
                    order: "o1".into(),
                    verdict: None,
                    findings: vec![finding(Some(Category::ScopeOmission), true)],
                    ..Review::default()
                },
            ],
            ..State::default()
        };
        state.timing.record("worker_turn", 100, 200).unwrap();
        state.timing.record("worker_turn", 150, 250).unwrap();
        state.timing.record("review_wait", 300, 330).unwrap();
        state
    }

    #[test]
    fn first_pass_means_one_order_and_no_review_asking_changes() {
        let log = [
            entry(1_000, Kind::Start, "task started"),
            entry(1_100, Kind::Check, "candidate check failed: smoke"),
            entry(1_200, Kind::Escalate, "which tenant?"),
            entry(1_300, Kind::Approve, "plan approved"),
            entry(4_600, Kind::Review, "accepted"),
        ];
        let report = build("bench-1", &run(), &log, vec!["M01".into()], None);
        assert_eq!(report.elapsed_seconds, 3_600);
        assert_eq!((report.steps, report.accepted), (4, 3));
        assert_eq!(
            report.first_pass, 1,
            "S01-F01 needed a correction round, S01-B02 three orders"
        );
        assert_eq!(report.first_pass_rate(), Some(1.0 / 3.0));
        assert_eq!(report.corrective_orders, 2);
        assert_eq!((report.reviews, report.reviews_asking_changes), (2, 1));
        assert_eq!(
            report.corrections,
            BTreeMap::from([
                (Category::IntegrationDefect, 1),
                (Category::TestWeakening, 1),
            ]),
            "blocking findings of submitted reviews, and failed candidate checks"
        );
        assert_eq!(
            report.interventions,
            Interventions {
                escalations: 1,
                approvals: 1,
                ..Interventions::default()
            }
        );
    }

    #[test]
    fn overlapping_work_is_counted_once_and_old_history_stays_a_lower_bound() {
        let mut state = run();
        let report = build("bench-1", &state, &[], vec![], None);
        assert_eq!(report.busy_seconds["worker_turn"], 200);
        assert_eq!(
            report.recorded_elapsed_seconds, 180,
            "100–250 and 300–330: overlapping turns once"
        );
        assert!(!report.unknown_historical_timing);
        assert!(render(&report).contains("first pass"));

        state.timing.unknown_historical_timing = true;
        let report = build("bench-1", &state, &[], vec![], None);
        assert!(render(&report).contains("lower bound"));
        assert!(render(&report).contains("unknown"), "no usage known");
    }

    #[test]
    fn two_runs_compare_line_by_line() {
        let legacy = build("legacy-1", &State::default(), &[], vec![], None);
        let reviewed = build(
            "reviewed-1",
            &run(),
            &[],
            vec!["M01".into()],
            Some(Usage {
                input: 10,
                output: 5,
                cache: 0,
                cost: 1.5,
                unpriced: false,
            }),
        );
        let table = compare(&legacy, &reviewed);
        let first_pass = table
            .lines()
            .find(|line| line.trim_start().starts_with("first pass"))
            .unwrap();
        assert!(
            first_pass.contains("0 (-)") && first_pass.contains("1 (33%)"),
            "{first_pass}"
        );
        assert!(table.lines().next().unwrap().contains("legacy-1"));
        assert!(table.contains("$1.50"));
        assert!(
            table.contains("correction: TestWeakening"),
            "a measure only one run has still shows"
        );
    }
}
