//! Named checks share one isolated runner and order-bound evidence.
use crate::{checks, orders, state::Order, task::Task};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RepositoryChecks {
    pub environments: BTreeMap<String, Environment>,
    pub checks: BTreeMap<String, Profile>,
    pub selected: Vec<String>,
    /// Profiles run on each reviewed merge candidate (the lane's work merged
    /// with the task's branch) before its review: a fast build/smoke set that
    /// finds two lanes that pass alone and break together at that merge, not
    /// at the milestone.
    pub candidate: Vec<String>,
    /// Profiles that prove the application works: each starts it and runs
    /// smoke or end-to-end tests against it in one container (loopback only,
    /// no network). Every milestone that covers this repo runs them, and
    /// nothing earlier does. Left out when empty, like `lanes`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<String>,
    /// A lane's own commands for profile names: in that lane they replace the
    /// repo's (`test` runs `cd backend && …` in the back lane). Left out when
    /// empty, so a repo without them keeps its evidence signatures.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub lanes: BTreeMap<String, BTreeMap<String, Profile>>,
}

impl RepositoryChecks {
    /// The checks an order in `lane` runs: the repo's, with the lane's own
    /// profiles in place of those of the same name.
    pub fn for_lane(&self, lane: Option<&str>) -> RepositoryChecks {
        let mut checks = self.clone();
        checks.lanes = BTreeMap::new();
        if let Some(own) = lane.and_then(|name| self.lanes.get(name)) {
            checks.checks.extend(own.clone());
        }
        checks
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Environment {
    pub values: BTreeMap<String, String>,
    /// Child variable -> explicitly selected parent secret variable.
    pub sources: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub command: String,
    pub environment: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Require the JSON test-count contract at DEVAGENTICS_RESULT.
    #[serde(default)]
    pub test_evidence: bool,
}
fn default_timeout() -> u64 {
    900
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    TestFailure,
    RequiredTestsSkipped,
    EnvironmentFailure,
    FilePolicyFailure,
    Timeout,
    RunnerFailure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub profile: String,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    pub seconds: u64,
    pub diagnostic: String,
    pub log: PathBuf,
}

/// The lines of a failed check's log that say what failed (FAIL, errors,
/// expected and received values), or its last lines when none do (a crash),
/// without terminal colours: what the leader and reviewers, who cannot read
/// the log, are told. Empty when the log cannot be read.
pub fn failure_excerpt(log: &std::path::Path) -> String {
    const LINES: usize = 12;
    const MARKERS: [&str; 9] = [
        "FAIL", "Error", "error:", "Failed", "failed", "✗", "⨯", "Expected", "Received",
    ];
    let Ok(text) = fs::read_to_string(log) else {
        return String::new();
    };
    let lines: Vec<String> = text
        .lines()
        .map(without_colours)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let failing: Vec<&String> = lines
        .iter()
        .filter(|line| !line.starts_with('✓') && MARKERS.iter().any(|marker| line.contains(marker)))
        .take(LINES)
        .collect();
    let chosen: Vec<&String> = if failing.is_empty() {
        lines[lines.len().saturating_sub(LINES)..].iter().collect()
    } else {
        failing
    };
    let excerpt = chosen
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    excerpt.chars().take(1500).collect()
}

/// The last `lines` lines of a check's log, without terminal colours; empty
/// when the log cannot be read.
pub fn log_tail(log: &std::path::Path, lines: usize) -> String {
    let Ok(text) = fs::read_to_string(log) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| without_colours(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn without_colours(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            for next in characters.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(character);
        }
    }
    plain
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub order: String,
    pub revision: u64,
    pub base: String,
    pub head: String,
    pub configuration: String,
    pub run: PathBuf,
    pub phases: Vec<Phase>,
}
impl Evidence {
    pub fn response(&self) -> serde_json::Value {
        serde_json::json!({ "order": self.order, "revision": self.revision, "base": self.base, "head": self.head, "passed": self.passed(), "run": self.run, "phases": self.phases })
    }
    pub fn passed(&self) -> bool {
        self.phases
            .iter()
            .all(|phase| phase.outcome == Outcome::Passed)
    }
}

/// The definitions of the profiles `names`, each with its environment.
pub(crate) fn named(
    definitions: &RepositoryChecks,
    names: impl IntoIterator<Item = String>,
) -> Result<Vec<(String, Profile, Environment)>> {
    names
        .into_iter()
        .map(|name| {
            let profile = definitions
                .checks
                .get(&name)
                .with_context(|| format!("check profile {name} is not defined"))?
                .clone();
            let environment = definitions
                .environments
                .get(&profile.environment)
                .with_context(|| format!("environment {} is not defined", profile.environment))?
                .clone();
            Ok((name, profile, environment))
        })
        .collect()
}

pub fn validate(config: &RepositoryChecks) -> Result<()> {
    for lane in config.lanes.keys() {
        validate(&config.for_lane(Some(lane))).with_context(|| format!("lane {lane}'s checks"))?;
    }
    if config.selected.len() > 32 {
        bail!("select at most 32 check profiles");
    }
    let mut selected = std::collections::BTreeSet::new();
    for name in &config.selected {
        if !selected.insert(name) {
            bail!("duplicate selected profile {name}");
        }
    }
    for (name, profile) in &config.checks {
        if name.is_empty()
            || name.len() > 64
            || !name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            bail!("profile names must be 1-64 letters, digits, - or _");
        }
        if profile.requires.len() > 32 {
            bail!("at most 32 requirements per profile");
        }
        if profile.command.trim().is_empty()
            || profile.command.contains('\0')
            || profile.timeout_seconds == 0
        {
            bail!("{name}: valid command and positive timeout required");
        }
    }
    for environment in config.environments.values() {
        for (key, value) in &environment.values {
            if value.contains('\0') {
                bail!("environment {key} contains a NUL byte");
            }
        }
        for source in environment.sources.values() {
            if source.is_empty() || source.len() > 128 || source.contains(['=', '\0']) {
                bail!("invalid environment source name");
            }
        }
    }

    let mut candidate = std::collections::BTreeSet::new();
    for name in &config.candidate {
        if !candidate.insert(name) {
            bail!("duplicate candidate profile {name}");
        }
    }
    if config.candidate.len() > 32 {
        bail!("select at most 32 candidate profiles");
    }

    let mut system = std::collections::BTreeSet::new();
    for name in &config.system {
        if !system.insert(name) {
            bail!("duplicate system profile {name}");
        }
    }
    if config.system.len() > 8 {
        bail!("select at most 8 system profiles");
    }

    for name in config
        .selected
        .iter()
        .chain(&config.candidate)
        .chain(&config.system)
    {
        let profile = config
            .checks
            .get(name)
            .with_context(|| format!("unknown check profile {name}"))?;
        if profile.command.trim().is_empty() || profile.timeout_seconds == 0 {
            bail!("{name}: command and positive timeout required");
        }
        let environment = config
            .environments
            .get(&profile.environment)
            .with_context(|| {
                format!("unknown environment {:?} for check {name}; declare it in this repository's environments", profile.environment)
            })?;
        for key in environment.values.keys().chain(environment.sources.keys()) {
            if key.is_empty()
                || key.len() > 128
                || !key
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
                || key == "DEVAGENTICS_RESULT"
            {
                bail!("invalid/reserved environment variable {key}");
            }
            if environment.values.contains_key(key) && environment.sources.contains_key(key) {
                bail!("ambiguous environment variable {key}");
            }
        }
        if profile
            .requires
            .iter()
            .any(|requirement| requirement.trim().is_empty())
        {
            bail!("empty preflight command");
        }
    }
    Ok(())
}

pub fn configure(task: &Task, repo: &str, profiles: RepositoryChecks) -> Result<()> {
    validate(&profiles)?;
    let _control = task.control_lock()?;
    let mut config = task.config()?;
    let repository = config
        .repos
        .iter()
        .find(|candidate| candidate.name == repo)
        .context("unknown repo")?;
    if repository.check.is_some() && !profiles.selected.is_empty() {
        bail!("remove the legacy repo check before selecting named profiles");
    }
    for lane in profiles.lanes.keys() {
        ensure!(
            config
                .lanes
                .iter()
                .any(|found| found.name == *lane && found.repo == repo),
            "{repo} has no lane {lane}: add_lane it first"
        );
    }
    let state = task.state()?;
    if state.workflow == crate::execution::Workflow::Reviewed && state.application.approved() {
        ensure!(
            state.application.check_configuration.get(repo) == Some(&profiles),
            "check configuration differs from the approved baseline; propose and obtain approval for the material gate change first"
        );
    }
    config.check_profiles.insert(repo.into(), profiles);
    task.write_config(&config)
}

/// One check/report operation per order. OS locks release on process exit.
pub fn operation(task: &Task, order: &str) -> Result<File> {
    let state = task.state()?;
    if !state.orders.iter().any(|candidate| candidate.id == order) {
        // The right IDs, so the caller does not try others one by one.
        let open: Vec<String> = state
            .orders
            .iter()
            .filter(|candidate| candidate.status == crate::state::OrderStatus::Open)
            .map(|candidate| {
                format!(
                    "{} ({}, step {})",
                    candidate.id, candidate.agent, candidate.step
                )
            })
            .collect();
        bail!(
            "unknown order {order}; the open orders are {}",
            if open.is_empty() {
                "none".to_string()
            } else {
                open.join(", ")
            }
        );
    }
    if !order
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        bail!("invalid order ID");
    }
    let folder = task.dir().join("operations");
    fs::create_dir_all(&folder)?;
    let lock = File::create(folder.join(format!("{order}.lock")))?;
    let deadline = std::time::Instant::now() + OPERATION_LOCK_WAIT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => {
                return Err(error).context("a check or report for this order is already running");
            }
        }
    }
}

/// How long `operation` waits for the order's lock before refusing. A
/// process another thread of this program is starting holds a copy of the
/// lock's file until it runs its program, keeping a lock just released for
/// a moment; a real check or report holds it for seconds or minutes.
const OPERATION_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// The checks an order runs: its repo's, as its agent's lane sees them, with
/// the profiles the order selected; none when it selects none. A run, its
/// staleness check and every later validation compare this same value.
fn order_checks(
    config: &crate::task::TaskConfig,
    state: &crate::state::State,
    order: &Order,
    agent: &crate::state::Agent,
) -> Option<RepositoryChecks> {
    let mut selected = config
        .check_profiles
        .get(&agent.repo)
        .map(|checks| checks.for_lane(agent.lane.as_deref()));
    if let (Some(checks), Some(names)) = (&mut selected, state.order_profiles.get(&order.id)) {
        checks.selected = names.clone();
    }
    selected.filter(|checks| !checks.selected.is_empty())
}

pub fn configuration(task: &Task, order: &Order) -> Result<Option<RepositoryChecks>> {
    let state = task.state()?;
    let agent = state.agent(&order.agent).context("order agent missing")?;
    let config = task.config()?;
    if state.workflow == crate::execution::Workflow::Reviewed {
        ensure!(
            state.application.check_configuration.get(&agent.repo)
                == config.check_profiles.get(&agent.repo),
            "configured checks differ from approved baseline"
        );
    }
    if !config.check_profiles.contains_key(&agent.repo)
        && state
            .order_profiles
            .get(&order.id)
            .is_some_and(|names| !names.is_empty())
    {
        bail!("named profile configuration was removed");
    }
    let selected = order_checks(&config, &state, order, agent);
    if order.check.is_some() && selected.is_some() {
        bail!("legacy command and named check profiles cannot be mixed");
    }
    if let Some(config) = &selected {
        validate(config)?;
    }
    Ok(selected)
}

pub fn run(task: &Task, id: &str, pane: Option<&str>) -> Result<Evidence> {
    let _operation = operation(task, id)?;
    let state = task.state()?;
    let order = state
        .orders
        .iter()
        .find(|order| order.id == id)
        .context("unknown order")?;
    if let Some(pane) = pane {
        let agent = state.agent_in_pane(pane).context("not a team agent pane")?;
        orders::authoritative(&state, &agent.id, id)?;
    }
    orders::authoritative(&state, &order.agent, id)?;
    execute(task, order, orders::check_timeout())
}

/// Caller holds the order operation lock; report and explicit run share this path.
pub fn execute(task: &Task, order: &Order, timeout: Duration) -> Result<Evidence> {
    let timing_start = crate::log::now();
    let state = task.state()?;
    orders::validate_snapshot(&state, order, state.revision(&order.id))?;
    let agent = state.agent(&order.agent).context("order agent missing")?;
    let repo = crate::lanes::workplace(task, agent)?;
    let config = configuration(task, order)?;
    let signature = signature(&state, config.as_ref(), order)?;
    let head = checks::head(&repo.worktree)?;
    let work = checks::inspect(
        &repo.worktree,
        &order.base,
        &order.allowed,
        &order.forbidden,
    )?;
    let lane_ok = if let Some(name) = &agent.lane {
        let lane = crate::lanes::get(task, name)?;
        checks::inspect(&repo.worktree, &order.base, &lane.globs, &[])?.follows_the_rules()
    } else {
        true
    };
    let clean = work.uncommitted.is_empty() && work.follows_the_rules() && lane_ok;
    let reusable = config.as_ref().is_some_and(|config| {
        config.selected.iter().all(|name| {
            config.environments[&config.checks[name].environment]
                .sources
                .is_empty()
        })
    });
    if clean
        && reusable
        && let Some(old) = state.check_evidence.get(&order.id)
        && old.revision == state.revision(&order.id)
        && old.base == order.base
        && old.head == head
        && old.configuration == signature
        && old.passed()
        && old.run.is_file()
    {
        let _control = task.control_lock()?;
        task.transaction(|tx| {
            orders::validate_snapshot(&tx.state, order, old.revision)?;
            validate_configuration(task, &tx.state, order, old)?;
            if checks::head(&repo.worktree)? != old.head
                || checks::inspect(
                    &repo.worktree,
                    &order.base,
                    &order.allowed,
                    &order.forbidden,
                )? != work
            {
                bail!(crate::failures::Stale("cached evidence is stale".into()));
            }
            Ok(())
        })?;
        return Ok(old.clone());
    }
    let logs = task.dir().join("checks");
    fs::create_dir_all(&logs)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let folder = logs.join(format!("{}-{nonce}-{}", order.id, std::process::id()));
    fs::create_dir(&folder)?;
    let mut evidence = Evidence {
        order: order.id.clone(),
        revision: state.revision(&order.id),
        base: order.base.clone(),
        head,
        configuration: signature,
        run: folder.join("evidence.json"),
        phases: Vec::new(),
    };
    if !clean {
        evidence.phases.push(Phase {
            profile: "file-policy".into(),
            outcome: Outcome::FilePolicyFailure,
            exit_code: None,
            seconds: 0,
            diagnostic: "Work must be committed and within order, forbidden and lane policies."
                .into(),
            log: folder.clone(),
        });
    } else if let Some(config) = &config {
        for (index, name) in config.selected.iter().enumerate() {
            evidence
                .phases
                .push(if state.workflow == crate::execution::Workflow::Reviewed {
                    run_container_profile(
                        task,
                        crate::check_profiles::Container {
                            agent: &agent.id,
                            lane: agent.lane.as_deref(),
                            targets: &order.targets,
                        },
                        name,
                        &config.checks[name],
                        &config.environments[&config.checks[name].environment],
                        &folder,
                        index,
                    )?
                } else {
                    run_profile(
                        name,
                        &config.checks[name],
                        &config.environments[&config.checks[name].environment],
                        &repo.worktree,
                        &folder,
                        index,
                    )?
                });
        }
    } else if state.workflow == crate::execution::Workflow::Reviewed {
        bail!("reviewed execution requires named checks");
    } else if let Some(command) = &order.check {
        let log = folder.join("legacy.log");
        let result = checks::run_check(command, &repo.worktree, &log, timeout);
        evidence.phases.push(match result {
            Ok(run) => Phase {
                profile: "legacy".into(),
                outcome: if run.passed {
                    Outcome::Passed
                } else if run.timed_out {
                    Outcome::Timeout
                } else {
                    Outcome::TestFailure
                },
                exit_code: run.exit_code,
                seconds: run.seconds,
                diagnostic: "Legacy command executed with inherited environment.".into(),
                log,
            },
            Err(error) => Phase {
                profile: "legacy".into(),
                outcome: Outcome::RunnerFailure,
                exit_code: None,
                seconds: 0,
                diagnostic: format!("{error:#}").chars().take(512).collect(),
                log,
            },
        });
    }
    fs::write(&evidence.run, serde_json::to_vec_pretty(&evidence)?)?;
    let _control = task.control_lock()?;
    task.transaction(|tx| {
        orders::validate_snapshot(&tx.state, order, evidence.revision)?;
        let current_profiles = order_checks(&task.config()?, &tx.state, order, agent);
        if checks::head(&repo.worktree)? != evidence.head
            || current_profiles.as_ref() != config.as_ref()
            || checks::inspect(
                &repo.worktree,
                &order.base,
                &order.allowed,
                &order.forbidden,
            )? != work
        {
            bail!(crate::failures::Stale(
                "check inputs changed; evidence is stale".into()
            ));
        }
        tx.state
            .timing
            .record("checks", timing_start, crate::log::now().max(timing_start))?;
        tx.state
            .check_evidence
            .insert(order.id.clone(), evidence.clone());
        Ok(())
    })?;
    Ok(evidence)
}

pub fn validate_configuration(
    task: &Task,
    state: &crate::state::State,
    order: &Order,
    evidence: &Evidence,
) -> Result<()> {
    let agent = state.agent(&order.agent).context("order agent missing")?;
    let config = task.config()?;
    if state.workflow == crate::execution::Workflow::Reviewed {
        ensure!(
            state.application.check_configuration.get(&agent.repo)
                == config.check_profiles.get(&agent.repo),
            "checks no longer match approved baseline"
        );
    }

    let selected = order_checks(&config, state, order, agent);
    let signature = signature(state, selected.as_ref(), order)?;
    if !same_configuration(state.workflow, &signature, &evidence.configuration)? {
        bail!(crate::failures::Stale(
            "check configuration changed; evidence is stale".into()
        ));
    }
    Ok(())
}

/// Whether evidence was made under the configuration checks have now. A
/// reviewed check runs in the runner's container, its environment set by the
/// runner configuration (part of the signature): the host process's own
/// PATH and temp folders do not count. Under Codex each process has its own
/// (a random `codex-arg0…` folder), so the worker's evidence would be stale
/// to the leader at once.
fn same_configuration(
    workflow: crate::execution::Workflow,
    current: &str,
    recorded: &str,
) -> Result<bool> {
    if workflow != crate::execution::Workflow::Reviewed {
        return Ok(current == recorded);
    }
    let parts = |signature: &str| -> Result<Vec<serde_json::Value>> {
        let mut parts: Vec<serde_json::Value> = serde_json::from_str(signature)?;
        // (profiles, check, host environment, runner, targets): leave the
        // host out. Evidence from before targets has four parts: it differs.
        ensure!(
            (4..=5).contains(&parts.len()),
            "unreadable check configuration signature"
        );
        parts.remove(2);
        Ok(parts)
    };
    Ok(parts(current)? == parts(recorded)?)
}

fn signature(
    state: &crate::state::State,
    config: Option<&RepositoryChecks>,
    order: &Order,
) -> Result<String> {
    let runner = if state.workflow == crate::execution::Workflow::Reviewed {
        state.runner.clone()
    } else {
        None
    };
    // A pass on the targets never stands in for one everywhere.
    Ok(serde_json::to_string(&(
        config,
        &order.check,
        baseline(),
        runner,
        &order.targets,
    ))?)
}
/// The script a check runs in its container: the environment, where its
/// result goes, the order's targets (validated paths, orders.rs
/// `validate_paths`; empty: everywhere), then the command.
fn container_script(prefix: &str, targets: &[String], command: &str) -> String {
    format!(
        "{prefix}export DEVAGENTICS_RESULT=/artifacts/result.json\nexport DEVAGENTICS_TARGETS={}\n{command}\n",
        shell_quote(&targets.join(" "))
    )
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
/// Where a check runs: the runner workspace of `agent`, in `lane`'s image,
/// on `targets` (an order's; none: everywhere).
pub(crate) struct Container<'a> {
    pub agent: &'a str,
    pub lane: Option<&'a str>,
    pub targets: &'a [String],
}

pub(crate) fn run_container_profile(
    task: &Task,
    container: Container<'_>,
    name: &str,
    profile: &Profile,
    environment: &Environment,
    folder: &Path,
    index: usize,
) -> Result<Phase> {
    let log = folder.join(format!("phase-{index}.log"));
    let mut phase = Phase {
        profile: name.into(),
        outcome: Outcome::EnvironmentFailure,
        exit_code: None,
        seconds: 0,
        diagnostic: String::new(),
        log: log.clone(),
    };
    if !environment.sources.is_empty() {
        phase.diagnostic="Host environment/secret sources are not available to reviewed tool containers. Configure an isolated disposable test environment.".into();
        fs::write(&log, &phase.diagnostic)?;
        return Ok(phase);
    }
    crate::runner::assert_checkpointed(task, container.agent)?;
    let mut prefix = String::new();
    for (key, value) in &environment.values {
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            bail!("invalid isolated environment key");
        }
        prefix.push_str(&format!("export {key}={};", shell_quote(value)));
        prefix.push('\n');
    }
    let started = Instant::now();
    for (command, preflight) in profile
        .requires
        .iter()
        .map(|command| (command, true))
        .chain(std::iter::once((&profile.command, false)))
    {
        let artifacts = folder.join(format!("results-{index}"));
        fs::create_dir_all(&artifacts)?;
        let result_path = artifacts.join("result.json");
        if result_path.symlink_metadata().is_ok() {
            crate::runner::result_bytes(&result_path)?;
            fs::remove_file(&result_path)?;
        }
        let command = container_script(&prefix, container.targets, command);
        let state = task.state()?;
        let workspace = state
            .runner_workspaces
            .get(container.agent)
            .context("isolated check source missing")?;
        let mut runner = state
            .runner
            .as_ref()
            .context("runner missing")?
            .for_lane(container.lane);
        runner.timeout_seconds = runner.timeout_seconds.min(profile.timeout_seconds);
        let result = crate::runner::run_container_artifacts(
            &runner,
            &workspace.snapshot,
            false,
            &command,
            Some(&artifacts),
        )?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?;
        writeln!(file, "{}\n{}", result.stdout, result.stderr)?;
        if !result.success {
            phase.outcome = if result.timed_out {
                Outcome::Timeout
            } else if preflight {
                Outcome::EnvironmentFailure
            } else {
                Outcome::TestFailure
            };
            phase.diagnostic = if preflight {
                "Required capability unavailable in isolated runner"
            } else {
                "Isolated test command failed"
            }
            .into();
            phase.exit_code = Some(1);
            phase.seconds = started.elapsed().as_secs();
            return Ok(phase);
        }
        if !preflight && profile.test_evidence {
            let counts = crate::runner::result_bytes(&result_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<TestEvidence>(&bytes).ok());
            if !counts.is_some_and(|counts| {
                counts.tests > 0 && counts.failures == 0 && counts.required_skipped == 0
            }) {
                phase.outcome = Outcome::RequiredTestsSkipped;
                phase.diagnostic =
                    "Required test-count evidence missing or required tests skipped".into();
                phase.seconds = started.elapsed().as_secs();
                return Ok(phase);
            }
        }
    }
    crate::runner::assert_checkpointed(task, container.agent)?;
    phase.outcome = Outcome::Passed;
    phase.exit_code = Some(0);
    phase.seconds = started.elapsed().as_secs();
    phase.diagnostic = "Named check passed in source-only, non-networked container".into();
    Ok(phase)
}

fn baseline() -> BTreeMap<String, String> {
    ["PATH", "TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.into(), value)))
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestEvidence {
    tests: u64,
    failures: u64,
    required_skipped: u64,
}

fn run_profile(
    name: &str,
    profile: &Profile,
    environment: &Environment,
    worktree: &Path,
    folder: &Path,
    index: usize,
) -> Result<Phase> {
    let log = folder.join(format!("phase-{index}.log"));
    let result_file = folder.join(format!("phase-{index}.json"));
    let mut phase = Phase {
        profile: name.into(),
        outcome: Outcome::RunnerFailure,
        exit_code: None,
        seconds: 0,
        diagnostic: String::new(),
        log: log.clone(),
    };
    File::create(&log)?;
    let mut variables = baseline();
    variables.extend(environment.values.clone());
    let mut secrets = Vec::new();
    for (child, parent) in &environment.sources {
        match std::env::var(parent) {
            Ok(value) if !value.is_empty() => {
                secrets.push(value.clone());
                variables.insert(child.clone(), value);
            }
            _ => {
                phase.outcome = Outcome::EnvironmentFailure;
                phase.diagnostic = format!("Required environment source {parent} is unavailable.");
                return Ok(phase);
            }
        }
    }
    variables.insert(
        "DEVAGENTICS_RESULT".into(),
        result_file.display().to_string(),
    );
    let started = Instant::now();
    let deadline = Duration::from_secs(profile.timeout_seconds);
    for (command, preflight) in profile
        .requires
        .iter()
        .map(|command| (command.as_str(), true))
        .chain(std::iter::once((profile.command.as_str(), false)))
    {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?;
        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(worktree)
            .env_clear()
            .envs(&variables)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(file)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(_) => {
                phase.diagnostic = "Could not start check process.".into();
                break;
            }
        };
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() >= deadline {
                let killed = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                if killed != 0 {
                    child.kill().context("killing timed-out phase")?;
                }
                child.wait().context("reaping timed-out phase")?;
                break None;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        phase.exit_code = status.and_then(|status| status.code());
        let Some(status) = status else {
            phase.outcome = Outcome::Timeout;
            phase.diagnostic = "Check exceeded its phase timeout.".into();
            break;
        };
        if !status.success() {
            phase.outcome = if preflight {
                Outcome::EnvironmentFailure
            } else {
                Outcome::TestFailure
            };
            phase.diagnostic = if preflight {
                "Declared prerequisite failed."
            } else {
                "Test command failed."
            }
            .into();
            break;
        }
        if preflight {
            continue;
        }
        phase.outcome = Outcome::Passed;
        phase.diagnostic = "Check passed.".into();
        if profile.test_evidence {
            match fs::read(&result_file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<TestEvidence>(&bytes).ok())
            {
                Some(result) if result.required_skipped > 0 => {
                    phase.outcome = Outcome::RequiredTestsSkipped;
                    phase.diagnostic =
                        format!("{} required tests skipped.", result.required_skipped);
                }
                Some(result) if result.failures > 0 => {
                    phase.outcome = Outcome::TestFailure;
                    phase.diagnostic = format!("{} tests failed.", result.failures);
                }
                Some(result) if result.tests > 0 => {}
                _ => {
                    phase.outcome = Outcome::RunnerFailure;
                    phase.diagnostic =
                        "Missing, invalid or empty required test-count evidence.".into();
                }
            }
        }
    }
    phase.seconds = started.elapsed().as_secs();
    // Keep full output, redacting explicitly supplied secrets before artifacts are exposed.
    if let Ok(bytes) = fs::read(&log) {
        let output = secrets.iter().fold(
            String::from_utf8_lossy(&bytes).into_owned(),
            |output, secret| output.replace(secret, "[REDACTED]"),
        );
        if phase.outcome != Outcome::Passed && !output.trim().is_empty() {
            let tail: String = output
                .chars()
                .rev()
                .take(512)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            phase.diagnostic.push_str(" Last output: ");
            phase.diagnostic.push_str(tail.trim());
        }
        fs::write(&log, output)?;
    }
    if result_file.is_file() {
        let bytes = fs::read(&result_file)?;
        let output = secrets.iter().fold(
            String::from_utf8_lossy(&bytes).into_owned(),
            |output, secret| output.replace(secret, "[REDACTED]"),
        );
        fs::write(result_file, output)?;
    }
    Ok(phase)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_check_gets_its_orders_targets_and_its_evidence_says_them() {
        let script = super::container_script(
            "export A='1';\n",
            &["web/src/FileMenu.test.tsx".into(), "web/src/menu".into()],
            "npm test -- $DEVAGENTICS_TARGETS",
        );
        assert_eq!(
            script,
            "export A='1';\nexport DEVAGENTICS_RESULT=/artifacts/result.json\nexport DEVAGENTICS_TARGETS='web/src/FileMenu.test.tsx web/src/menu'\nnpm test -- $DEVAGENTICS_TARGETS\n"
        );
        assert!(
            super::container_script("", &[], "true").contains("export DEVAGENTICS_TARGETS=''\n"),
            "no targets: everywhere"
        );
        use crate::execution::Workflow;
        let targeted = r#"[{"selected":["test"]},null,{},{"image":"node"},["web/src/a.test.tsx"]]"#;
        let everywhere = r#"[{"selected":["test"]},null,{},{"image":"node"},[]]"#;
        let before = r#"[{"selected":["test"]},null,{},{"image":"node"}]"#;
        assert!(!super::same_configuration(Workflow::Reviewed, targeted, everywhere).unwrap());
        assert!(
            !super::same_configuration(Workflow::Reviewed, everywhere, before).unwrap(),
            "older evidence runs again"
        );
    }

    // The leader has no shell and reviewers' sandboxes hold only the
    // worktree: a failed check's own lines are all they get of its log.
    #[test]
    fn a_failure_excerpt_keeps_the_failing_lines_without_colours() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("phase-2.log");
        std::fs::write(
            &log,
            "  \x1b[32m✓\x1b[39m it passes\n\n\x1b[41m FAIL \x1b[49m src/a.test.tsx > keeps the mutation\n\x1b[31mAssertionError\x1b[39m: expected undefined to be 'mutation remains'\n  ✓ another pass\n",
        )
        .unwrap();
        let excerpt = super::failure_excerpt(&log);
        assert!(
            excerpt.contains("FAIL  src/a.test.tsx > keeps the mutation")
                && excerpt.contains("AssertionError: expected undefined"),
            "{excerpt}"
        );
        assert!(
            !excerpt.contains('\x1b') && !excerpt.contains("it passes"),
            "{excerpt}"
        );

        // Nothing says which test failed (a crash): its last lines.
        std::fs::write(&log, "   PASS  Tests\\Unit\\A\nback-check: note: backend/.env.example is not in the snapshot\n").unwrap();
        let excerpt = super::failure_excerpt(&log);
        assert!(
            excerpt.contains("Tests\\Unit\\A") && excerpt.contains(".env.example"),
            "{excerpt}"
        );
        assert_eq!(super::failure_excerpt(&dir.path().join("missing.log")), "");
    }

    #[test]
    fn a_reviewed_check_does_not_go_stale_in_another_process() {
        use crate::execution::Workflow;
        let worker =
            r#"[{"selected":["lint"]},null,{"PATH":"/codex/arg0-AAA:/usr/bin"},{"image":"node"}]"#;
        let leader =
            r#"[{"selected":["lint"]},null,{"PATH":"/codex/arg0-BBB:/usr/bin"},{"image":"node"}]"#;
        assert!(same_configuration(Workflow::Reviewed, leader, worker).unwrap());
        let other_profiles = r#"[{"selected":["test"]},null,{"PATH":"/usr/bin"},{"image":"node"}]"#;
        assert!(!same_configuration(Workflow::Reviewed, other_profiles, worker).unwrap());
        let other_runner = r#"[{"selected":["lint"]},null,{"PATH":"/codex/arg0-AAA:/usr/bin"},{"image":"python"}]"#;
        assert!(!same_configuration(Workflow::Reviewed, other_runner, worker).unwrap());
        assert!(
            !same_configuration(Workflow::Legacy, leader, worker).unwrap(),
            "legacy checks run on the host: its environment counts"
        );
    }

    use super::*;
    use tempfile::TempDir;

    fn profile(command: &str) -> Profile {
        Profile {
            command: command.into(),
            environment: "testing".into(),
            timeout_seconds: 3,
            requires: Vec::new(),
            test_evidence: false,
        }
    }

    fn phase(command: &str) -> Phase {
        let dir = TempDir::new().unwrap();
        run_profile(
            "fixture",
            &profile(command),
            &Environment::default(),
            dir.path(),
            dir.path(),
            0,
        )
        .unwrap()
    }

    #[test]
    fn a_system_check_names_a_declared_profile_once() {
        let mut checks = web_and_back();
        checks.system = vec!["lint".into()];
        validate(&checks).unwrap();
        checks.system = vec!["e2e".into()];
        let error = format!("{:#}", validate(&checks).unwrap_err());
        assert!(error.contains("e2e"), "{error}");
        checks.system = vec!["lint".into(), "lint".into()];
        let error = format!("{:#}", validate(&checks).unwrap_err());
        assert!(error.contains("duplicate system profile"), "{error}");
        let empty = serde_json::to_value(web_and_back()).unwrap();
        assert!(
            empty.get("system").is_none(),
            "old evidence signatures stay the same"
        );
    }

    fn web_and_back() -> RepositoryChecks {
        RepositoryChecks {
            environments: BTreeMap::from([("testing".into(), Environment::default())]),
            checks: BTreeMap::from([
                ("lint".into(), profile("cd web && npm run lint")),
                ("test".into(), profile("cd web && npm test")),
            ]),
            selected: vec!["lint".into(), "test".into()],
            candidate: vec!["lint".into()],
            system: Vec::new(),
            lanes: BTreeMap::from([(
                "back".into(),
                BTreeMap::from([("test".into(), profile("cd backend && php artisan test"))]),
            )]),
        }
    }

    #[test]
    fn a_lane_runs_its_own_command_for_a_profile_and_the_repos_for_the_rest() {
        let checks = web_and_back();
        validate(&checks).unwrap();
        let back = checks.for_lane(Some("back"));
        assert_eq!(
            back.checks["test"].command,
            "cd backend && php artisan test"
        );
        assert_eq!(back.checks["lint"].command, "cd web && npm run lint");
        assert_eq!(
            checks.for_lane(Some("front")).checks["test"].command,
            "cd web && npm test"
        );
        assert_ne!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(&checks.for_lane(Some("front"))).unwrap(),
            "evidence of one lane never counts for the other"
        );

        let mut broken = web_and_back();
        broken
            .lanes
            .get_mut("back")
            .unwrap()
            .get_mut("test")
            .unwrap()
            .command = " ".into();
        let error = validate(&broken).unwrap_err();
        assert!(format!("{error:#}").contains("lane back"), "{error:#}");
    }

    #[test]
    fn checks_for_a_lane_the_task_does_not_have_are_refused() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        let repo = task.config().unwrap().repos[0].name.clone();
        let error = configure(&task, &repo, web_and_back())
            .unwrap_err()
            .to_string();
        assert!(error.contains("no lane back"), "{error}");
        crate::lanes::add(&task, "back", &["backend/**".into()], None).unwrap();
        configure(&task, &repo, web_and_back()).unwrap();
    }

    #[test]
    fn phases_do_not_inherit_home_or_each_others_database_variables() {
        let dir = TempDir::new().unwrap();
        let environment = Environment {
            values: BTreeMap::from([
                ("DB_CONNECTION".into(), "pgsql".into()),
                ("PGHOST".into(), "test-host".into()),
            ]),
            sources: BTreeMap::new(),
        };
        let first = run_profile("postgres", &profile("test \"$DB_CONNECTION\" = pgsql && test \"$PGHOST\" = test-host && test -z \"${HOME+x}\""), &environment, dir.path(), dir.path(), 0).unwrap();
        assert_eq!(first.outcome, Outcome::Passed);
        let second = run_profile(
            "sqlite",
            &profile("test -z \"${DB_CONNECTION+x}${PGHOST+x}${HOME+x}\""),
            &Environment::default(),
            dir.path(),
            dir.path(),
            1,
        )
        .unwrap();
        assert_eq!(second.outcome, Outcome::Passed);
    }

    #[test]
    fn failures_are_classified_without_parsing_logs() {
        assert_eq!(
            phase("echo environment failure; exit 4").outcome,
            Outcome::TestFailure
        );
        let dir = TempDir::new().unwrap();
        let mut check = profile("true");
        check.requires.push("exit 2".into());
        assert_eq!(
            run_profile(
                "fixture",
                &check,
                &Environment::default(),
                dir.path(),
                dir.path(),
                0
            )
            .unwrap()
            .outcome,
            Outcome::EnvironmentFailure
        );
        check.requires.clear();
        check.test_evidence = true;
        assert_eq!(
            run_profile(
                "fixture",
                &check,
                &Environment::default(),
                dir.path(),
                dir.path(),
                1
            )
            .unwrap()
            .outcome,
            Outcome::RunnerFailure
        );
        check.command = "printf '%s' '{\"tests\":2,\"failures\":0,\"required_skipped\":1}' > \"$DEVAGENTICS_RESULT\"".into();
        assert_eq!(
            run_profile(
                "fixture",
                &check,
                &Environment::default(),
                dir.path(),
                dir.path(),
                2
            )
            .unwrap()
            .outcome,
            Outcome::RequiredTestsSkipped
        );
    }

    #[test]
    fn missing_sources_fail_before_launch_and_secret_outputs_are_redacted() {
        let dir = TempDir::new().unwrap();
        let mut environment = Environment {
            values: BTreeMap::new(),
            sources: BTreeMap::from([(
                "TOKEN".into(),
                "DEVAGENTICS_NONEXISTENT_TEST_SECRET_3719".into(),
            )]),
        };
        let result = run_profile(
            "fixture",
            &profile("touch launched"),
            &environment,
            dir.path(),
            dir.path(),
            0,
        )
        .unwrap();
        assert_eq!(result.outcome, Outcome::EnvironmentFailure);
        assert!(!dir.path().join("launched").exists());
        environment.sources.insert("TOKEN".into(), "PATH".into());
        let result = run_profile(
            "fixture",
            &profile("printf '%s' \"$TOKEN\""),
            &environment,
            dir.path(),
            dir.path(),
            1,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(result.log).unwrap(), "[REDACTED]");
    }

    #[test]
    fn timeout_terminates_the_shell_and_its_background_child() {
        let dir = TempDir::new().unwrap();
        let mut check = profile("sleep 30 & echo $! > child.pid; wait");
        check.timeout_seconds = 1;
        let result = run_profile(
            "fixture",
            &check,
            &Environment::default(),
            dir.path(),
            dir.path(),
            0,
        )
        .unwrap();
        assert_eq!(result.outcome, Outcome::Timeout);
        let pid = fs::read_to_string(dir.path().join("child.pid")).unwrap();
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "child still running: {state}"
        );
    }

    #[test]
    fn invalid_profiles_are_rejected_before_execution() {
        let mut config = RepositoryChecks {
            selected: vec!["sqlite".into()],
            ..Default::default()
        };
        assert!(validate(&config).is_err());
        config.checks.insert("sqlite".into(), profile("true"));
        assert!(validate(&config).is_err());
        config
            .environments
            .insert("testing".into(), Environment::default());
        assert!(validate(&config).is_ok());
        config
            .environments
            .get_mut("testing")
            .unwrap()
            .values
            .insert("DEVAGENTICS_RESULT".into(), "override".into());
        assert!(validate(&config).is_err());
    }

    #[test]
    fn an_operation_waits_out_a_lock_held_for_an_instant_but_not_a_running_one() {
        let directory = TempDir::new().unwrap();
        let home = crate::home::Home::at(directory.path().join("home"));
        let task = Task::create(
            &home,
            &crate::task::TaskConfig {
                id: "t".into(),
                branch: "feat/x".into(),
                workspace: "w".into(),
                max_agents: 3,
                max_attempts: crate::task::DEFAULT_MAX_ATTEMPTS,
                max_seats: crate::task::DEFAULT_MAX_SEATS,
                idle_close_minutes: crate::task::DEFAULT_IDLE_CLOSE_MINUTES,
                check_profiles: Default::default(),
                repos: vec![],
                lanes: vec![],
                routing: Default::default(),
                budget: Default::default(),
            },
        )
        .unwrap();
        task.transaction(|tx| {
            tx.state.orders.push(Order {
                read: Vec::new(),
                targets: Vec::new(),
                id: "o1".into(),
                step: "s1".into(),
                agent: "a".into(),
                text: String::new(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: String::new(),
                status: crate::state::OrderStatus::Open,
                delivered: true,
                stalled: false,
                plan: None,
            });
            Ok(())
        })
        .unwrap();

        // A process another thread is starting holds a copy of the lock's
        // file until it runs its program: a moment, not an operation.
        let held = operation(&task, "o1").unwrap();
        let copy = held.try_clone().unwrap();
        drop(held);
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(copy);
        });
        operation(&task, "o1").expect("a lock held for an instant is waited out");
        release.join().unwrap();

        let running = operation(&task, "o1").unwrap();
        let error = operation(&task, "o1").unwrap_err().to_string();
        assert!(error.contains("already running"), "{error}");
        drop(running);
    }

    #[test]
    fn candidate_profiles_must_be_defined_once() {
        let mut config = RepositoryChecks {
            checks: BTreeMap::from([("smoke".into(), profile("true"))]),
            environments: BTreeMap::from([("testing".into(), Environment::default())]),
            candidate: vec!["smoke".into()],
            ..Default::default()
        };
        validate(&config).unwrap();
        config.candidate = vec!["build".into()];
        assert!(validate(&config).unwrap_err().to_string().contains("build"));
        config.candidate = vec!["smoke".into(), "smoke".into()];
        assert!(validate(&config).is_err());
        let old: RepositoryChecks = serde_json::from_str(r#"{"selected":[]}"#).unwrap();
        assert!(old.candidate.is_empty(), "configurations from before load");
    }
}
