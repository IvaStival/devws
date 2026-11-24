//! The plans: the overall one the user approves, and the seniors' execution
//! plans the CTO approves by accepting their step.

use crate::log::Kind;
use crate::roles::Group;
use crate::state::ExecutionPlan;
use crate::task::Task;
use crate::team::Team;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum Stored {
    /// The task's plan, for the user to approve.
    Overall,
    /// A senior's plan for the step of its order.
    Execution { step: String },
}

fn path(task: &Task, step: &str) -> PathBuf {
    task.dir().join("plans").join(format!("{step}.md"))
}

/// Stores a plan written from an agent's pane: the overall plan until the user
/// has approved one, then the execution plan of the senior's open order.
pub fn store(team: &Team, pane: &str, text: &str) -> Result<Stored> {
    let state = team.task.state()?;
    let agent = state
        .agent_in_pane(pane)
        .with_context(|| format!("{pane} is not a team agent's pane"))?;
    if !state.approved {
        team.write_plan(text)?;
        return Ok(Stored::Overall);
    }
    if agent.group() != Group::Senior {
        bail!(
            "only a senior writes an execution plan; {} is on the {}",
            agent.id,
            agent.group().label().to_lowercase()
        );
    }
    let step = store_execution(team.task, &agent.id, text)?;
    Ok(Stored::Execution { step })
}

/// Stores a senior's execution plan for the step of its open order; says
/// which step. A reviewed planner reaches it through its worker tools.
pub fn store_execution(task: &Task, agent: &str, text: &str) -> Result<String> {
    let state = task.state()?;
    let order = state.open_order_of(agent).with_context(|| {
        format!("{agent} has no open order: an execution plan is for the step it was ordered")
    })?;
    if text.trim().is_empty() {
        bail!("the plan is empty");
    }
    let (step, author) = (order.step.clone(), agent.to_string());
    let file = path(task, &step);
    std::fs::create_dir_all(file.parent().unwrap())?;
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &file)?;
    task.transaction(|tx| {
        tx.state.plans.retain(|plan| plan.step != step);
        tx.state.plans.push(ExecutionPlan {
            step: step.clone(),
            author: author.clone(),
        });
        tx.log(
            Kind::Plan,
            Some(&author),
            Some(&step),
            &format!("execution plan written ({} lines)", text.lines().count()),
        );
        Ok(())
    })?;
    Ok(step)
}

/// The execution plan of a step.
pub fn read(task: &Task, step: &str) -> Result<String> {
    if task.state()?.plan(step).is_none() {
        bail!("step {step} has no execution plan");
    }
    std::fs::read_to_string(path(task, step))
        .with_context(|| format!("reading the plan of step {step}"))
}

const PLAN_PAGE_BYTES: usize = 8_192;
const PLAN_PAGE_LINES: usize = 120;
const OUTLINE_HEADINGS: usize = 40;

pub fn worker_view(step: &str, plan: &str, input: &Value) -> Result<Value> {
    let view = input.get("view").map(Value::as_str);
    let has_range = input.get("start_line").is_some() || input.get("line_count").is_some();
    let has_heading = input.get("heading").is_some();
    match view {
        None | Some(Some("full")) if !has_range && !has_heading => {
            Ok(json!({"step":step,"plan":plan}))
        }
        Some(Some("outline")) if !has_range && !has_heading => Ok(outline(step, plan)),
        Some(Some("excerpt")) if !has_heading => {
            let start = positive_number(input, "start_line", 1)?;
            let count = positive_number(input, "line_count", 40)?;
            anyhow::ensure!(
                count <= PLAN_PAGE_LINES,
                "line_count must be at most {PLAN_PAGE_LINES}"
            );
            excerpt(step, plan, start, count)
        }
        Some(Some("section")) => section(step, plan, input),
        _ => bail!(
            "plan_read view must be full, outline, excerpt or section; heading requires section and ranges require excerpt or section"
        ),
    }
}

fn positive_number(input: &Value, field: &str, default: usize) -> Result<usize> {
    match input.get(field) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .with_context(|| format!("{field} must be a positive integer")),
    }
}

fn headings(plan: &str) -> Vec<(usize, usize, &str)> {
    let mut headings = Vec::new();
    let mut fence = None;
    for (index, line) in plan.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(marker @ ('`' | '~')) = trimmed.chars().next() {
            let width = trimmed
                .bytes()
                .take_while(|byte| *byte == marker as u8)
                .count();
            match fence {
                Some((open, minimum))
                    if marker == open && width >= minimum && trimmed[width..].trim().is_empty() =>
                {
                    fence = None
                }
                None if width >= 3 => fence = Some((marker, width)),
                _ => {}
            }
        }
        if fence.is_some() || line.len() - trimmed.len() > 3 {
            continue;
        }
        let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&level)
            && let Some(title) = trimmed[level..].strip_prefix(' ')
        {
            headings.push((index + 1, level, title.trim()));
        }
    }
    headings
}

fn outline(step: &str, plan: &str) -> Value {
    let headings = headings(plan);
    let visible: Vec<_> = headings.iter().take(OUTLINE_HEADINGS).map(|(line, level, title)| {
        json!({"line":line,"level":level,"title":title.chars().take(160).collect::<String>()})
    }).collect();
    json!({"step":step,"total_lines":plan.lines().count(),"heading_count":headings.len(),"headings":visible})
}

fn section(step: &str, plan: &str, input: &Value) -> Result<Value> {
    let title = input["heading"]
        .as_str()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .context("section requires a non-empty heading")?;
    let headings = headings(plan);
    let mut matches = headings
        .iter()
        .enumerate()
        .filter(|(_, (_, _, candidate))| *candidate == title);
    let (index, &(first, level, _)) = matches
        .next()
        .with_context(|| format!("heading {title:?} was not found; use outline to locate it"))?;
    anyhow::ensure!(
        matches.next().is_none(),
        "heading {title:?} is ambiguous; use outline and excerpt"
    );
    let limit = headings[index + 1..]
        .iter()
        .find(|(_, depth, _)| *depth <= level)
        .map_or_else(|| plan.lines().count() + 1, |(line, _, _)| *line);
    let start = positive_number(input, "start_line", first)?;
    let count = positive_number(input, "line_count", PLAN_PAGE_LINES)?;
    anyhow::ensure!(
        (first..limit).contains(&start),
        "start_line must be within the section"
    );
    anyhow::ensure!(
        count <= PLAN_PAGE_LINES,
        "line_count must be at most {PLAN_PAGE_LINES}"
    );
    let mut page = excerpt(step, plan, start, count.min(limit - start))?;
    page["section_end_line"] = json!(limit - 1);
    if page["next_line"].as_u64() == Some(limit as u64) {
        page["next_line"] = Value::Null;
    }
    Ok(page)
}

fn excerpt(step: &str, plan: &str, start: usize, count: usize) -> Result<Value> {
    let total = plan.lines().count();
    anyhow::ensure!(
        start <= total,
        "start_line {start} is past the plan's {total} lines"
    );
    let mut lines = Vec::new();
    let mut size = 0;
    let mut oversized_line = false;
    for line in plan.lines().skip(start - 1).take(count) {
        if size + line.len() + 1 > PLAN_PAGE_BYTES {
            if lines.is_empty() {
                let mut end = PLAN_PAGE_BYTES.min(line.len());
                while !line.is_char_boundary(end) {
                    end -= 1;
                }
                lines.push(&line[..end]);
                oversized_line = true;
            }
            break;
        }
        lines.push(line);
        size += line.len() + 1;
    }
    let end = start + lines.len() - 1;
    Ok(json!({
        "step": step,
        "plan": lines.join("\n"),
        "start_line": start,
        "end_line": end,
        "total_lines": total,
        "next_line": (end < total).then_some(end + 1),
        "oversized_line": oversized_line,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::started;
    use crate::orders::{OrderRequest, send_order};
    use tempfile::TempDir;

    #[test]
    fn named_plan_section_ignores_fences_and_stops_before_the_next_peer() {
        let plan = "# Plan\n```md\n## Validation\n```\n## Validation\nRun checks.\n### Gate\nReview first.\n## Publish\n";
        let read = |input: Value| worker_view("s1", plan, &input).unwrap();
        let first = read(json!({"view":"section","heading":"Validation","line_count":2}));
        assert_eq!(first["plan"], "## Validation\nRun checks.");
        assert_eq!(first["next_line"], 7);
        let last = read(json!({"view":"section","heading":"Validation","start_line":7}));
        assert_eq!(last["plan"], "### Gate\nReview first.");
        assert_eq!(last["section_end_line"], 8);
        assert!(last["next_line"].is_null());
        assert!(
            worker_view(
                "s1",
                plan,
                &json!({"view":"section","heading":"Validation","start_line":9})
            )
            .is_err()
        );
        assert!(worker_view("s1", plan, &json!({"view":"section","heading":"Missing"})).is_err());
        let duplicate = format!("{plan}## Validation\n");
        assert!(
            worker_view(
                "s1",
                &duplicate,
                &json!({"view":"section","heading":"Validation"})
            )
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
        );
    }

    #[test]
    fn worker_plan_outline_ignores_fenced_headings_and_excerpts_use_its_line_numbers() {
        let plan = "# Plan\n~~~json\n# Literal data\n~~~\n## Validation\nRun the check.\n";
        let outline = worker_view("s1", plan, &json!({"view":"outline"})).unwrap();
        assert_eq!(outline["headings"].as_array().unwrap().len(), 2);
        assert_eq!(outline["headings"][1]["line"], 5);
        let page = worker_view(
            "s1",
            plan,
            &json!({"view":"excerpt","start_line":5,"line_count":1}),
        )
        .unwrap();
        assert_eq!(page["plan"], "## Validation");
        assert_eq!(page["next_line"], 6);
        assert_eq!(worker_view("s1", plan, &json!({})).unwrap()["plan"], plan);
        assert!(worker_view("s1", plan, &json!({"view":"excerpt","start_line":7})).is_err());
        assert!(worker_view("s1", plan, &json!({"view":"excerpt","line_count":121})).is_err());
    }

    #[test]
    fn excerpt_bounds_an_oversized_utf8_line_and_reports_truncation() {
        let plan = format!("{}\nnext", "€".repeat(4_000));
        let page = worker_view("s1", &plan, &json!({"view":"excerpt"})).unwrap();
        assert!(page["plan"].as_str().unwrap().len() <= PLAN_PAGE_BYTES);
        assert_eq!(page["oversized_line"], true);
        assert_eq!(page["next_line"], 2);
    }

    fn order(task: &Task, agent: &str, step: &str) {
        send_order(
            task,
            OrderRequest {
                diagnosis: None,
                read: Vec::new(),
                targets: Vec::new(),
                profiles: Vec::new(),
                agent: agent.into(),
                step: step.into(),
                title: "plan the screen".into(),
                text: "plan it".into(),
                allowed: vec![],
                forbidden: vec![],
                check: Some("true".into()),
                plan: None,
                spec: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_plan_is_the_overall_one_until_approved_then_a_seniors_execution_plan() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        assert_eq!(
            store(&team, &planner.pane, "1. s1: the screen\n").unwrap(),
            Stored::Overall
        );
        assert!(read(&task, "s1").is_err());
        team.approve().unwrap();

        let error = store(&team, &planner.pane, "x").unwrap_err().to_string();
        assert!(error.contains("no open order"), "{error}");
        order(&task, &planner.id, "s1");
        assert_eq!(
            task.state()
                .unwrap()
                .open_order_of("planner-1")
                .unwrap()
                .forbidden,
            ["**"],
            "a senior's order forbids every file"
        );
        assert_eq!(
            store(&team, &planner.pane, "1. build the form\n").unwrap(),
            Stored::Execution { step: "s1".into() }
        );
        assert_eq!(read(&task, "s1").unwrap(), "1. build the form\n");
        let state = task.state().unwrap();
        assert!(
            state.approved,
            "an execution plan leaves the overall plan approved"
        );
        assert_eq!(state.plan("s1").unwrap().author, "planner-1");

        let executor = team.spawn_agent("executor", None).unwrap();
        let error = store(&team, &executor.pane, "x").unwrap_err().to_string();
        assert!(error.contains("only a senior"), "{error}");
    }

    #[test]
    fn a_reviewed_planner_stores_the_execution_plan_of_its_order() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        team.write_plan("1. s1\n").unwrap();
        team.approve().unwrap();
        let error = store_execution(&task, &planner.id, "x")
            .unwrap_err()
            .to_string();
        assert!(error.contains("no open order"), "{error}");
        order(&task, &planner.id, "s1");
        assert_eq!(
            store_execution(&task, &planner.id, "1. manifest\n").unwrap(),
            "s1"
        );
        assert_eq!(read(&task, "s1").unwrap(), "1. manifest\n");
        assert!(
            store_execution(&task, &planner.id, "  ").is_err(),
            "an empty plan"
        );
    }

    #[test]
    fn a_worker_reads_the_plan_its_order_follows() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        team.write_plan("1. s1\n").unwrap();
        team.approve().unwrap();
        order(&task, &planner.id, "s1");
        store_execution(&task, &planner.id, "1. write the manifest\n").unwrap();
        let reader = planner.id.clone();
        let read = crate::worker_tools::call(
            &task,
            &reader,
            "plan_read",
            &serde_json::json!({"step": "s1"}),
        )
        .unwrap();
        assert_eq!(read["plan"], "1. write the manifest\n");
        let missing = crate::worker_tools::call(
            &task,
            &reader,
            "plan_read",
            &serde_json::json!({"step": "nope"}),
        );
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .contains("no execution plan")
        );
    }

    #[test]
    fn a_planner_reads_the_baseline_in_pieces_and_amends_it() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        crate::execution::store(&task, crate::execution::tests::baseline(), "first").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        let planner = team.spawn_agent("planner", None).unwrap();
        let call = |action: &str, input: serde_json::Value| {
            crate::worker_tools::call(&task, &planner.id, action, &input)
        };
        let outline = call("application_read", serde_json::json!({})).unwrap();
        assert_eq!(outline["items"]["S01-B01"]["title"], "Token service");
        assert!(outline["items"]["S01-B01"].get("acceptance").is_none());
        let full = call(
            "application_read",
            serde_json::json!({"items": ["S01-B01"]}),
        )
        .unwrap();
        assert_eq!(
            full["items"]["S01-B01"]["acceptance"][0],
            "Service contract passes"
        );

        let amended = call(
            "plan_amend",
            serde_json::json!({"reason": "narrow goal", "changes": {"goal": "Accounts only"}}),
        )
        .unwrap();
        assert_eq!(amended["proposed"], true);
        let proposal = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(
            (proposal.revision, proposal.goal.as_str()),
            (2, "Accounts only")
        );
        assert_eq!(proposal.items, crate::execution::tests::baseline().items);
        let pending = call("application_read", serde_json::json!({"proposal": true})).unwrap();
        assert_eq!(pending["goal"], "Accounts only");

        let executor = team.spawn_agent("executor", None).unwrap();
        for action in ["application_read", "plan_amend"] {
            let refused = crate::worker_tools::call(
                &task,
                &executor.id,
                action,
                &serde_json::json!({"reason": "x", "changes": {}}),
            );
            assert!(refused.is_err(), "{action} is not a writer's");
        }
    }
}
