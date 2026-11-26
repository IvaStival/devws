//! What the leader reads of a check run: it has no shell, and reviewers'
//! sandboxes hold only the worktree, so the check logs under the task's
//! folder reach them only through here.

use crate::check_profiles::{Outcome, Phase, log_tail};
use crate::task::Task;
use anyhow::{Context, Result, bail};

/// Most lines of one log the leader gets at once.
const MAX_LINES: usize = 300;

/// The latest check run of `order` (its own checks), or of its lane's merge
/// candidate: each profile's outcome, and the last `lines` lines of the logs
/// of `profile`, or of every profile that did not pass.
pub fn read(
    task: &Task,
    order: &str,
    candidate: bool,
    profile: Option<&str>,
    lines: usize,
) -> Result<String> {
    let state = task.state()?;
    let (title, phases) = if candidate {
        let found = state
            .orders
            .iter()
            .find(|found| found.id == order)
            .with_context(|| format!("there is no order {order}"))?;
        let author = state
            .agent(&found.agent)
            .with_context(|| format!("order {order} has no agent"))?;
        let lane = crate::lanes::of_agent(task, author)?
            .with_context(|| format!("order {order} has no lane, so no merge candidate"))?;
        let merge = crate::integration::candidate(task, &lane)?;
        let evidence = state
            .candidate_evidence
            .get(&merge.tree)
            .with_context(|| {
                format!(
                    "lane {}'s current merge candidate has not been checked yet (review_request runs it)",
                    lane.name
                )
            })?;
        (
            format!(
                "Merge candidate of lane {} ({} onto {}):",
                lane.name,
                short(&evidence.source),
                short(&evidence.target)
            ),
            evidence.phases.clone(),
        )
    } else {
        let evidence = state
            .check_evidence
            .get(order)
            .with_context(|| format!("order {order} has no check run yet"))?;
        (
            format!("Checks of order {order} at {}:", short(&evidence.head)),
            evidence.phases.clone(),
        )
    };
    if let Some(wanted) = profile
        && !phases.iter().any(|phase| phase.profile == wanted)
    {
        bail!("that run has no profile {wanted}");
    }
    Ok(describe(
        &title,
        &phases,
        profile,
        lines.clamp(1, MAX_LINES),
    ))
}

fn describe(title: &str, phases: &[Phase], profile: Option<&str>, lines: usize) -> String {
    let mut text = title.to_string();
    for phase in phases {
        text.push_str(&format!(
            "\n- {}: {:?}, exit {}, {} s. {}",
            phase.profile,
            phase.outcome,
            phase
                .exit_code
                .map_or_else(|| "none".to_string(), |code| code.to_string()),
            phase.seconds,
            phase.diagnostic
        ));
    }
    let shown = phases.iter().filter(|phase| match profile {
        Some(wanted) => phase.profile == wanted,
        None => phase.outcome != Outcome::Passed,
    });
    for phase in shown {
        let tail = log_tail(&phase.log, lines);
        text.push_str(&format!("\n\n{} log, last lines:\n", phase.profile));
        text.push_str(if tail.is_empty() {
            "(the log is empty or gone)"
        } else {
            &tail
        });
    }
    text
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_profiles::Evidence;
    use tempfile::TempDir;

    fn phase(dir: &TempDir, profile: &str, outcome: Outcome, log: &str) -> Phase {
        let path = dir.path().join(format!("{profile}.log"));
        std::fs::write(&path, log).unwrap();
        let exit_code = Some(if outcome == Outcome::Passed { 0 } else { 1 });
        Phase {
            profile: profile.into(),
            outcome,
            exit_code,
            seconds: 3,
            diagnostic: "Isolated test command failed".into(),
            log: path,
        }
    }

    #[test]
    fn the_leader_reads_the_failing_logs_of_an_orders_checks() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        let mut test_log: String = (1..=500).map(|line| format!("  ✓ test {line}\n")).collect();
        test_log.push_str(
            "\x1b[31m FAIL \x1b[39m ObserverPruneCommandTest\nTests: 1 failed, 1390 passed\n",
        );
        let phases = vec![
            phase(&dir, "lint", Outcome::Passed, "lint ok\n"),
            phase(&dir, "test", Outcome::TestFailure, &test_log),
        ];
        task.transaction(|tx| {
            tx.state.check_evidence.insert(
                "o695".into(),
                Evidence {
                    order: "o695".into(),
                    revision: 1,
                    base: "base".into(),
                    head: "4bd8a39a78dba9e7fdda5d2c".into(),
                    configuration: String::new(),
                    run: dir.path().join("evidence.json"),
                    phases,
                },
            );
            Ok(())
        })
        .unwrap();

        let text = read(&task, "o695", false, None, 5).unwrap();
        assert!(
            text.starts_with("Checks of order o695 at 4bd8a39a78db:"),
            "{text}"
        );
        assert!(
            text.contains("- lint: Passed") && text.contains("- test: TestFailure, exit 1"),
            "{text}"
        );
        assert!(
            text.contains("test log, last lines:")
                && text.contains("FAIL  ObserverPruneCommandTest"),
            "{text}"
        );
        assert!(
            !text.contains("lint log") && !text.contains("test 400"),
            "{text}"
        );
        assert!(!text.contains('\x1b'), "{text}");

        let lint = read(&task, "o695", false, Some("lint"), 5).unwrap();
        assert!(lint.contains("lint log, last lines:\nlint ok"), "{lint}");
        assert!(read(&task, "o695", false, Some("build"), 5).is_err());
        let error = read(&task, "o1", false, None, 5).unwrap_err().to_string();
        assert!(error.contains("no check run yet"), "{error}");
        let error = read(&task, "o1", true, None, 5).unwrap_err().to_string();
        assert!(error.contains("no order o1"), "{error}");
    }
}
