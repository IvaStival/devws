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
    /// Independent reviews: a light acceptance signed by the leader is not one.
    pub reviews: u64,
    pub reviews_asking_changes: u64,
    /// Light items the leader accepted on its reason, without a reviewer.
    pub light_acceptances: u64,
    /// Items by the rigor they are routed at now.
    pub items_by_rigor: BTreeMap<crate::execution::Rigor, u64>,
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
    /// The application has milestones and every one is ready.
    pub integrated: bool,
    /// Submitted reviews per step that had one: how many rounds review took.
    pub review_rounds_per_reviewed_step: Option<f64>,
    /// Work failures recorded on a step after it was first accepted, by step:
    /// defects acceptance let through. A lower bound: a defect found in
    /// another step's check is not traced back to the step that caused it.
    pub escaped_defects: BTreeMap<String, u64>,
    /// Questions agents asked the code graph.
    pub graph_queries: u64,
    /// None when the team's usage could not be read.
    pub usage: Option<Usage>,
    pub estimated_api_cost: CostCoverage,
    /// The run's cost when it integrated and every agent was priced: one
    /// success, so this is its cost per integrated success. A run that did
    /// not integrate has none; sum its cost into the runs that did.
    pub cost_per_integrated_success: Option<f64>,
    /// Usage summed by role, the leader as `leader`.
    pub usage_by_role: BTreeMap<String, Usage>,
    /// Leader calls (MCP tools and CLI reads) per accepted step, from telemetry: each call
    /// rereads the leader's whole context, so this drives the leader's cost.
    pub leader_calls_per_accepted_step: Option<f64>,
    /// The leader's `leader-read` commands by view, counted in the
    /// transcripts the report was given (`--leader-transcript`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_reads_in_transcripts: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_telemetry: Option<crate::telemetry::Summary>,
}

/// Coverage of the available pane and archived readings. An agent may have
/// additional sessions that those readings do not cover.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CostCoverage {
    pub observed_usd: Option<f64>,
    pub observed_agents: usize,
    pub unpriced_agents: usize,
    pub missing_agents: usize,
}

impl CostCoverage {
    fn from_readings(expected_agents: usize, readings: impl IntoIterator<Item = Usage>) -> Self {
        let mut coverage = Self::default();
        for usage in readings {
            coverage.observed_agents += 1;
            coverage.unpriced_agents += usize::from(usage.unpriced);
            if !usage.unpriced || usage.cost > 0.0 {
                *coverage.observed_usd.get_or_insert(0.0) += usage.cost;
            }
        }
        coverage.missing_agents = expected_agents.saturating_sub(coverage.observed_agents);
        coverage
    }

    fn display(&self) -> String {
        match self.observed_usd {
            Some(cost) if self.unpriced_agents == 0 && self.missing_agents == 0 => {
                format!("${cost:.2} observed")
            }
            Some(cost) => format!("${cost:.2} priced subtotal (incomplete coverage)"),
            None => "unknown".into(),
        }
    }
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

/// A complete price is the only one worth dividing: an unpriced or missing
/// agent would make the cost per success look lower than it was.
fn cost_per_integrated_success(integrated: bool, cost: &CostCoverage) -> Option<f64> {
    let complete = cost.unpriced_agents == 0 && cost.missing_agents == 0;
    cost.observed_usd.filter(|_| integrated && complete)
}

/// Leader tool calls; discovery and handshakes are not moves. CLI reads are
/// the larger of the ledger's count and the transcripts': a read-only
/// sandbox (Codex) can refuse the ledger write while the read itself works.
fn leader_calls(telemetry: &crate::telemetry::Summary, transcript_reads: Option<u64>) -> u64 {
    let mut tools = 0;
    let mut ledger_reads = 0;
    for (operation, payloads) in telemetry
        .mcp
        .get(crate::state::LEADER)
        .into_iter()
        .flatten()
    {
        match operation.as_str() {
            "tools/list" | "initialize" | "ping" | "unknown_method" | "invalid_json" => {}
            cli if cli.starts_with("cli:") => ledger_reads += payloads.calls as u64,
            _ => tools += payloads.calls as u64,
        }
    }
    tools + ledger_reads.max(transcript_reads.unwrap_or(0))
}

/// The `leader-read <task> <view>` commands a leader ran, by view, from its
/// transcripts: Codex's tool calls (`custom_tool_call`, `function_call`) and
/// Claude's Bash `tool_use`. Only the commands are read, never their output
/// or the conversation.
pub fn cli_reads_in_transcript(transcript: &str, task: &str) -> BTreeMap<String, u64> {
    let marker = format!("leader-read {task} ");
    let mut reads = BTreeMap::new();
    for line in transcript.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = &event["payload"];
        let mut commands: Vec<&str> = Vec::new();
        if matches!(
            payload["type"].as_str(),
            Some("custom_tool_call" | "function_call")
        ) {
            commands.extend(payload["input"].as_str());
            commands.extend(payload["arguments"].as_str());
        }
        for block in event["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] == "tool_use" && block["name"] == "Bash" {
                commands.extend(block["input"]["command"].as_str());
            }
        }
        for command in commands {
            for (index, _) in command.match_indices(&marker) {
                let view: String = command[index + marker.len()..]
                    .chars()
                    .take_while(char::is_ascii_alphabetic)
                    .collect();
                if !view.is_empty() {
                    *reads.entry(format!("cli:{view}")).or_default() += 1;
                }
            }
        }
    }
    reads
}

fn escaped_defects(state: &State, log: &[Entry]) -> BTreeMap<String, u64> {
    let mut first_accepted: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in log
        .iter()
        .filter(|entry| entry.kind == Kind::Review && entry.text == "accepted")
    {
        if let Some(step) = &entry.step {
            let at = first_accepted.entry(step).or_insert(entry.at);
            *at = (*at).min(entry.at);
        }
    }
    first_accepted
        .into_iter()
        .filter_map(|(step, accepted_at)| {
            let escaped = state
                .step_failures
                .get(step)
                .into_iter()
                .flatten()
                // Work after a revision reset is new work on a changed
                // contract, not a defect the acceptance let through.
                .take_while(|failure| {
                    failure.at <= accepted_at
                        || failure.kind != crate::failures::FailureKind::RevisionReset
                })
                .filter(|failure| failure.at > accepted_at && crate::failures::counts(failure))
                .count() as u64;
            (escaped > 0).then(|| (step.to_string(), escaped))
        })
        .collect()
}

fn review_rounds_per_reviewed_step(state: &State) -> Option<f64> {
    let mut rounds: BTreeMap<&str, u64> = BTreeMap::new();
    for review in state
        .reviews
        .iter()
        .filter(|review| review.verdict.is_some() && review.reviewer != crate::state::LEADER)
    {
        if let Some(order) = state.orders.iter().find(|order| order.id == review.order) {
            *rounds.entry(&order.step).or_default() += 1;
        }
    }
    (!rounds.is_empty()).then(|| rounds.values().sum::<u64>() as f64 / rounds.len() as f64)
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
            .filter(|review| review.verdict.is_some() && review.reviewer != crate::state::LEADER)
            .count() as u64,
        light_acceptances: state
            .reviews
            .iter()
            .filter(|review| review.reviewer == crate::state::LEADER)
            .count() as u64,
        items_by_rigor: BTreeMap::new(),
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
        integrated: !state.application.milestones.is_empty()
            && state
                .application
                .milestones
                .keys()
                .all(|id| milestones_ready.contains(id)),
        milestones_ready,
        review_rounds_per_reviewed_step: review_rounds_per_reviewed_step(state),
        escaped_defects: escaped_defects(state, log),
        graph_queries: state
            .timing
            .intervals
            .iter()
            .filter(|interval| interval.category == "graph_queries")
            .count() as u64,
        usage,
        estimated_api_cost: CostCoverage::default(),
        cost_per_integrated_success: None,
        usage_by_role: BTreeMap::new(),
        leader_calls_per_accepted_step: None,
        cli_reads_in_transcripts: None,
        payload_telemetry: None,
    }
}

fn count(log: &[Entry], matches: impl Fn(&Entry) -> bool) -> u64 {
    log.iter().filter(|entry| matches(entry)).count() as u64
}

/// The report of a task, with its team's usage when devws can tell it.
/// `transcripts`: the leader's provider transcripts, read for its CLI reads.
pub fn report(
    task: &Task,
    devws: &dyn crate::devws::Devws,
    transcripts: &[std::path::PathBuf],
) -> Result<Report> {
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
    let team_usage = crate::board_usage::UsageReader::new(std::time::Duration::ZERO)
        .of_team(task, devws)
        .ok();
    let usage = team_usage
        .as_ref()
        .filter(|team| !team.by_agent.is_empty())
        .map(|team| crate::board_usage::total(&team.by_role));
    let config = task.config()?;
    let mut report = build(&config.id, &state, &log, milestones_ready, usage);
    for id in state.application.items.keys() {
        let routing = state
            .application
            .routing(id, crate::failures::work_failures(&state, id))?
            .under(config.routing);
        *report.items_by_rigor.entry(routing.rigor).or_default() += 1;
    }
    report.estimated_api_cost = CostCoverage::from_readings(
        state.agents.len() + 1,
        team_usage
            .iter()
            .flat_map(|team| team.by_agent.values().copied()),
    );
    report.cost_per_integrated_success =
        cost_per_integrated_success(report.integrated, &report.estimated_api_cost);
    for (role, usage) in team_usage.iter().flat_map(|team| &team.by_role) {
        let sum = report.usage_by_role.entry(role.clone()).or_default();
        *sum = sum.plus(*usage);
    }
    report.payload_telemetry = crate::telemetry::summary(task)?;
    if !transcripts.is_empty() {
        let mut reads = BTreeMap::new();
        for path in transcripts {
            let text = std::fs::read_to_string(path).map_err(|error| {
                anyhow::anyhow!("reading the transcript {}: {error}", path.display())
            })?;
            for (view, number) in cli_reads_in_transcript(&text, &config.id) {
                *reads.entry(view).or_default() += number;
            }
        }
        report.cli_reads_in_transcripts = Some(reads);
    }
    let transcript_reads = report
        .cli_reads_in_transcripts
        .as_ref()
        .map(|reads| reads.values().sum());
    report.leader_calls_per_accepted_step = report
        .payload_telemetry
        .as_ref()
        .filter(|_| report.accepted > 0)
        .map(|telemetry| leader_calls(telemetry, transcript_reads) as f64 / report.accepted as f64);
    Ok(report)
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
            "light acceptances (no reviewer)".into(),
            report.light_acceptances.to_string(),
        ),
        (
            "items by rigor".into(),
            if report.items_by_rigor.is_empty() {
                "-".into()
            } else {
                report
                    .items_by_rigor
                    .iter()
                    .map(|(rigor, number)| format!("{rigor:?} {number}").to_lowercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
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
    lines.push(("graph queries".into(), report.graph_queries.to_string()));
    lines.push((
        "milestones ready".into(),
        if report.milestones_ready.is_empty() {
            "none".into()
        } else {
            report.milestones_ready.join(", ")
        },
    ));
    lines.push((
        "integrated".into(),
        if report.integrated { "yes" } else { "no" }.into(),
    ));
    lines.push((
        "review rounds per reviewed step".into(),
        report
            .review_rounds_per_reviewed_step
            .map_or("-".into(), |rounds| format!("{rounds:.1}")),
    ));
    lines.push((
        "escaped defects (lower bound)".into(),
        if report.escaped_defects.is_empty() {
            "0".into()
        } else {
            let steps: Vec<String> = report
                .escaped_defects
                .iter()
                .map(|(step, number)| format!("{step} {number}"))
                .collect();
            format!(
                "{} ({})",
                report.escaped_defects.values().sum::<u64>(),
                steps.join(", ")
            )
        },
    ));
    if let Some(reads) = &report.cli_reads_in_transcripts {
        let ledger: u64 = report
            .payload_telemetry
            .iter()
            .flat_map(|telemetry| telemetry.mcp.get(crate::state::LEADER))
            .flatten()
            .filter(|(operation, _)| operation.starts_with("cli:"))
            .map(|(_, payloads)| payloads.calls as u64)
            .sum();
        lines.push((
            "leader CLI reads (ledger / transcripts)".into(),
            format!("{ledger} / {}", reads.values().sum::<u64>()),
        ));
    }
    lines.push((
        "leader calls per accepted step".into(),
        report
            .leader_calls_per_accepted_step
            .map_or("unknown (telemetry off)".into(), |calls| {
                format!("{calls:.1}")
            }),
    ));
    lines.push((
        "tokens in / out / cache".into(),
        report.usage.map_or("unknown".into(), |usage| {
            format!("{} / {} / {}", usage.input, usage.output, usage.cache)
        }),
    ));
    lines.push((
        "cost at API prices".into(),
        report.estimated_api_cost.display(),
    ));
    lines.push((
        "cost per integrated success".into(),
        report.cost_per_integrated_success.map_or_else(
            || {
                if report.integrated {
                    "unknown (incomplete pricing)".into()
                } else {
                    "- (not integrated)".into()
                }
            },
            |cost| format!("${cost:.2}"),
        ),
    ));
    let total: f64 = report.usage_by_role.values().map(|usage| usage.cost).sum();
    for (role, usage) in &report.usage_by_role {
        let share = if total > 0.0 {
            format!(" ({:.0}%)", usage.cost / total * 100.0)
        } else {
            String::new()
        };
        let unpriced = if usage.unpriced {
            ", unpriced usage"
        } else {
            ""
        };
        lines.push((
            format!("  cost: {role}"),
            format!("${:.2}{share}{unpriced}", usage.cost),
        ));
    }
    lines.push((
        "agent price coverage".into(),
        format!(
            "{} observed, {} with unpriced usage, {} missing",
            report.estimated_api_cost.observed_agents,
            report.estimated_api_cost.unpriced_agents,
            report.estimated_api_cost.missing_agents,
        ),
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
            read: Vec::new(),
            targets: Vec::new(),
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
            pre_existing: false,
            caused_by_change: false,
        }
    }

    fn accepted(at: u64, step: &str) -> Entry {
        Entry {
            step: Some(step.into()),
            ..entry(at, Kind::Review, "accepted")
        }
    }

    fn failure(at: u64, kind: crate::failures::FailureKind) -> crate::failures::Failure {
        crate::failures::Failure {
            at,
            order: Some("o9".into()),
            kind,
            summary: "failed".into(),
            diagnosis: None,
            resolved_by: None,
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
        state.timing.record("graph_queries", 340, 341).unwrap();
        state.timing.record("graph_queries", 342, 342).unwrap();
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
        assert_eq!(report.graph_queries, 2);
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
            report.recorded_elapsed_seconds, 181,
            "100–250, 300–330 and a 1 s graph query: overlapping turns once"
        );
        assert!(!report.unknown_historical_timing);
        assert!(render(&report).contains("first pass"));

        state.timing.unknown_historical_timing = true;
        let report = build("bench-1", &state, &[], vec![], None);
        assert!(render(&report).contains("lower bound"));
        assert!(render(&report).contains("unknown"), "no usage known");
    }

    #[test]
    fn observed_cost_distinguishes_unknown_from_zero_and_partial_pricing() {
        let unknown = Usage {
            unpriced: true,
            ..Usage::default()
        };
        let zero = Usage::default();
        let paid = Usage {
            cost: 0.42,
            ..Usage::default()
        };

        let coverage = CostCoverage::from_readings(2, [unknown]);
        assert_eq!(coverage.observed_usd, None);
        assert_eq!(coverage.unpriced_agents, 1);
        assert_eq!(coverage.missing_agents, 1);
        assert_eq!(coverage.display(), "unknown");
        assert!(serde_json::to_value(&coverage).unwrap()["observed_usd"].is_null());

        let coverage = CostCoverage::from_readings(1, [zero]);
        assert_eq!(coverage.display(), "$0.00 observed");

        let coverage = CostCoverage::from_readings(3, [unknown, paid]);
        assert_eq!(
            coverage.display(),
            "$0.42 priced subtotal (incomplete coverage)"
        );
        assert_eq!(coverage.observed_agents, 2);
        assert_eq!(coverage.missing_agents, 1);

        let partially_priced = Usage {
            unpriced: true,
            ..paid
        };
        let coverage = CostCoverage::from_readings(1, [partially_priced]);
        assert_eq!(coverage.observed_usd, Some(0.42));
        assert_eq!(coverage.unpriced_agents, 1);
    }

    #[test]
    fn two_runs_compare_line_by_line() {
        let legacy = build("legacy-1", &State::default(), &[], vec![], None);
        let usage = Usage {
            input: 10,
            output: 5,
            cost: 1.5,
            ..Usage::default()
        };
        let mut reviewed = build("reviewed-1", &run(), &[], vec!["M01".into()], Some(usage));
        reviewed.estimated_api_cost = CostCoverage::from_readings(1, [usage]);
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

    #[test]
    fn a_run_integrates_only_with_every_milestone_ready() {
        let mut state = run();
        assert!(
            !build("bench-1", &state, &[], vec![], None).integrated,
            "no milestones"
        );
        state.application.milestones = BTreeMap::from([
            ("M01".into(), crate::execution::Milestone::default()),
            ("M02".into(), crate::execution::Milestone::default()),
        ]);
        assert!(!build("bench-1", &state, &[], vec!["M01".into()], None).integrated);
        let report = build(
            "bench-1",
            &state,
            &[],
            vec!["M01".into(), "M02".into()],
            None,
        );
        assert!(report.integrated);
        assert!(
            render(&report)
                .lines()
                .any(|line| line.trim_start().starts_with("integrated") && line.ends_with("yes"))
        );
    }

    #[test]
    fn cost_per_success_needs_an_integrated_run_and_complete_pricing() {
        let priced = CostCoverage::from_readings(
            2,
            [
                Usage {
                    cost: 2.76,
                    ..Usage::default()
                },
                Usage {
                    cost: 1.80,
                    ..Usage::default()
                },
            ],
        );
        let success = cost_per_integrated_success(true, &priced).unwrap();
        assert!((success - 4.56).abs() < 1e-9);
        assert_eq!(cost_per_integrated_success(false, &priced), None);
        let missing = CostCoverage::from_readings(3, [Usage::default()]);
        assert_eq!(
            cost_per_integrated_success(true, &missing),
            None,
            "a missing agent would make it look cheaper than it was"
        );
    }

    #[test]
    fn a_work_failure_after_acceptance_is_an_escaped_defect() {
        use crate::failures::FailureKind;
        let mut state = run();
        state.step_failures = BTreeMap::from([(
            "S01-B01".into(),
            vec![
                failure(900, FailureKind::CheckFailed),
                failure(1_500, FailureKind::CheckFailed),
                failure(1_600, FailureKind::RevisionReset),
            ],
        )]);
        state
            .step_failures
            .get_mut("S01-B01")
            .unwrap()
            .push(failure(1_650, FailureKind::CheckFailed));
        let log = [accepted(1_000, "S01-B01"), accepted(1_700, "S01-B01")];
        let report = build("bench-1", &state, &log, vec![], None);
        assert_eq!(
            report.escaped_defects,
            BTreeMap::from([("S01-B01".into(), 1)]),
            "the failure before acceptance was caught; a revision reset and the new work after it are no defect"
        );
        assert!(render(&report).contains("1 (S01-B01 1)"));
    }

    #[test]
    fn a_light_acceptance_is_counted_apart_from_independent_reviews() {
        let mut state = run();
        state.reviews.push(Review {
            id: "r4".into(),
            order: "o1".into(),
            reviewer: crate::state::LEADER.into(),
            verdict: Some(Verdict::Approved),
            ..Review::default()
        });
        let report = build("bench-1", &state, &[], vec![], None);
        assert_eq!((report.reviews, report.light_acceptances), (2, 1));
        assert_eq!(
            report.review_rounds_per_reviewed_step,
            Some(2.0),
            "S01-B01's light acceptance is no review round"
        );
    }

    #[test]
    fn review_rounds_count_submitted_reviews_per_reviewed_step() {
        let report = build("bench-1", &run(), &[], vec![], None);
        assert_eq!(
            report.review_rounds_per_reviewed_step,
            Some(2.0),
            "S01-F01 had two submitted reviews; the open one on S01-B01 is no round"
        );
        let empty = build("bench-1", &State::default(), &[], vec![], None);
        assert_eq!(empty.review_rounds_per_reviewed_step, None);
    }

    #[test]
    fn leader_calls_leave_out_discovery_and_other_agents() {
        let calls = |number| crate::telemetry::Payloads {
            calls: number,
            ..crate::telemetry::Payloads::default()
        };
        let telemetry = crate::telemetry::Summary {
            mcp: BTreeMap::from([
                (
                    crate::state::LEADER.to_string(),
                    BTreeMap::from([
                        ("tools/list".into(), calls(3)),
                        ("initialize".into(), calls(1)),
                        ("task_status.progress".into(), calls(4)),
                        ("send_order".into(), calls(2)),
                        ("cli:progress".into(), calls(5)),
                    ]),
                ),
                (
                    "worker-1".into(),
                    BTreeMap::from([("worker".into(), calls(9))]),
                ),
            ]),
            ..crate::telemetry::Summary::default()
        };
        assert_eq!(
            leader_calls(&telemetry, None),
            11,
            "CLI reads are leader calls too"
        );
        assert_eq!(
            leader_calls(&telemetry, Some(8)),
            14,
            "the sandbox kept 3 reads out of the ledger"
        );
        assert_eq!(leader_calls(&telemetry, Some(2)), 11);
    }

    #[test]
    fn cli_reads_are_counted_from_codex_and_claude_transcripts_only_for_the_task() {
        let codex = serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"exec",
            "input":"text(await tools.exec_command({cmd:\"'/bin/devagentics' leader-read feat-x progress\"}));"}});
        let claude = serde_json::json!({"type":"assistant","message":{"content":[
            {"type":"tool_use","name":"Bash","input":{"command":"/bin/devagentics leader-read feat-x order o1"}}]}});
        let other_task = serde_json::json!({"type":"response_item","payload":{"type":"function_call",
            "arguments":"{\"cmd\":\"devagentics leader-read feat-y inbox\"}"}});
        let mentioned = serde_json::json!({"type":"response_item","payload":{"type":"message",
            "content":[{"text":"use `devagentics leader-read feat-x progress`"}]}});
        let transcript = [codex.clone(), claude, other_task, mentioned, codex]
            .iter()
            .map(|event| event.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            cli_reads_in_transcript(&transcript, "feat-x"),
            BTreeMap::from([("cli:order".into(), 1), ("cli:progress".into(), 2)])
        );
    }

    #[test]
    fn cost_by_role_shows_each_role_and_its_share() {
        let mut report = build("bench-1", &run(), &[], vec![], None);
        for (role, cost) in [("leader", 3.0), ("code-reviewer", 1.0)] {
            report.usage_by_role.insert(
                role.into(),
                Usage {
                    cost,
                    ..Usage::default()
                },
            );
        }
        let text = render(&report);
        assert!(
            text.contains("cost: leader") && text.contains("$3.00 (75%)"),
            "{text}"
        );
        assert!(text.contains("$1.00 (25%)"), "{text}");
        assert!(text.contains("unknown (telemetry off)"));
    }
}
