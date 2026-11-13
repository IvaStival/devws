//! Named checks share one isolated runner and order-bound evidence.
use crate::{checks, orders, state::Order, task::Task};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
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

pub fn validate(config: &RepositoryChecks) -> Result<()> {
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

    for name in &config.selected {
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
            .with_context(|| format!("unknown environment {}", profile.environment))?;
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
    config.check_profiles.insert(repo.into(), profiles);
    task.write_config(&config)
}

/// One check/report operation per order. OS locks release on process exit.
pub fn operation(task: &Task, order: &str) -> Result<File> {
    if !task
        .state()?
        .orders
        .iter()
        .any(|candidate| candidate.id == order)
    {
        bail!("unknown order {order}");
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
    lock.try_lock_exclusive()
        .context("a check or report for this order is already running")?;
    Ok(lock)
}

pub fn configuration(task: &Task, order: &Order) -> Result<Option<RepositoryChecks>> {
    let state = task.state()?;
    let agent = state.agent(&order.agent).context("order agent missing")?;
    let config = task.config()?;
    let mut selected = config.check_profiles.get(&agent.repo).cloned();
    if let Some(names) = state.order_profiles.get(&order.id) {
        if selected.is_none() && !names.is_empty() {
            bail!("named profile configuration was removed");
        }
        if let Some(config) = &mut selected {
            config.selected = names.clone();
        }
    }
    let selected = selected.filter(|config| !config.selected.is_empty());
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
    let state = task.state()?;
    orders::validate_snapshot(&state, order, state.revision(&order.id))?;
    let agent = state.agent(&order.agent).context("order agent missing")?;
    let repo = crate::lanes::workplace(task, agent)?;
    let config = configuration(task, order)?;
    let signature = serde_json::to_string(&(config.as_ref(), &order.check, baseline()))?;
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
                bail!("cached evidence is stale");
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
            evidence.phases.push(run_profile(
                name,
                &config.checks[name],
                &config.environments[&config.checks[name].environment],
                &repo.worktree,
                &folder,
                index,
            )?);
        }
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
        let current_config = task.config()?;
        let mut current_profiles = current_config.check_profiles.get(&agent.repo).cloned();
        if let Some(profiles) = &mut current_profiles
            && let Some(names) = tx.state.order_profiles.get(&order.id)
        {
            profiles.selected = names.clone();
        }
        if checks::head(&repo.worktree)? != evidence.head
            || current_profiles
                .filter(|profiles| !profiles.selected.is_empty())
                .as_ref()
                != config.as_ref()
            || checks::inspect(
                &repo.worktree,
                &order.base,
                &order.allowed,
                &order.forbidden,
            )? != work
        {
            bail!("check inputs changed; evidence is stale");
        }
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
    let mut selected = config.check_profiles.get(&agent.repo).cloned();
    if let Some(config) = &mut selected
        && let Some(names) = state.order_profiles.get(&order.id)
    {
        config.selected = names.clone();
    }
    let selected = selected.filter(|config| !config.selected.is_empty());
    let signature = serde_json::to_string(&(selected.as_ref(), &order.check, baseline()))?;
    if signature != evidence.configuration {
        bail!("check configuration changed; evidence is stale");
    }
    Ok(())
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
}
