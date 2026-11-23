//! Versioned application decisions, work contracts and milestone readiness.
use crate::{
    log::Kind,
    state::{State, StepStatus},
    task::Task,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const STANDARDS_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workflow {
    #[default]
    Legacy,
    Reviewed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Application {
    pub revision: u64,
    pub goal: String,
    pub non_goals: Vec<String>,
    pub decisions: Vec<Decision>,
    pub scopes: BTreeMap<String, String>,
    pub items: BTreeMap<String, Contract>,
    pub milestones: BTreeMap<String, Milestone>,
    pub check_configuration: BTreeMap<String, crate::check_profiles::RepositoryChecks>,
    /// What lanes agree on, written once: an item provides each and the
    /// items that use it consume it, so both sides read the same text.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub interfaces: BTreeMap<String, Interface>,
    /// Existing tracked acceptance files, immutable across all orders in a repo.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub existing_protected_tests: BTreeMap<String, Vec<String>>,
    pub confirmed_revision: Option<u64>,
    pub approved_revision: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub round: u64,
    pub question: String,
    pub recommendation: String,
    pub prerequisites: Vec<String>,
    pub answer: Option<String>,
    /// Hard to undo (an auth strategy, a data format): it needs written
    /// alternatives before anyone picks one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub one_way: bool,
    /// What the pick is judged on, written before the alternatives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
}

impl Decision {
    /// Settled: answered, or a reversible choice that takes its
    /// recommendation. Only a one-way decision waits for the user's answer.
    pub fn settled(&self) -> bool {
        self.answer
            .as_ref()
            .is_some_and(|answer| !answer.trim().is_empty())
            || !self.one_way
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub option: String,
    pub tradeoffs: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cost: String,
}

/// The longest interface spec: enough for signatures, a schema or an API
/// fragment, short enough to inline in every order that uses it.
const MAX_SPEC_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    Http,
    Function,
    Event,
    Schema,
    Cli,
    Config,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    /// Set by devAgentics: it goes up each time the spec changes.
    #[serde(default)]
    pub version: u32,
    /// The item that provides it.
    pub owner: String,
    pub kind: InterfaceKind,
    pub spec: String,
    /// A check profile of the owner's repo that tests it (a contract test).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Contract {
    pub scope: String,
    pub title: String,
    pub role: String,
    pub repo: String,
    pub lane: String,
    pub dependencies: Vec<String>,
    pub allowed: Vec<String>,
    pub supporting_files: Vec<String>,
    pub reuse: Vec<String>,
    pub interfaces: Vec<String>,
    pub behavior: Vec<String>,
    pub errors: Vec<String>,
    pub tests: Vec<String>,
    pub profiles: Vec<String>,
    pub acceptance: Vec<String>,
    pub non_goals: Vec<String>,
    pub questions: Vec<String>,
    pub standards: String,
    pub data_effects: Vec<String>,
    /// Test files (globs) that judge this item: an implementation may read
    /// them but not change them, so it cannot pass by weakening its tests.
    pub protected_tests: Vec<String>,
    /// This item writes the protected tests (of the items that depend on it).
    pub writes_tests: bool,
    /// Interfaces of the baseline this item builds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,
    /// Interfaces of the baseline this item uses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<String>,
    /// What kind of work it is; it routes the item with its rigor.
    pub kind: WorkKind,
    /// The rigor the planner asks for. The effective rigor is never below
    /// the floor devAgentics computes from the item's facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rigor: Option<Rigor>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Change,
    Bugfix,
    /// Older baselines have no kind: they keep today's full process.
    #[default]
    Feature,
    Interface,
    Investigation,
}

/// How much process an item gets, lowest first: the order is the ranking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rigor {
    Light,
    #[default]
    Standard,
    Strict,
    Critical,
}

impl Rigor {
    /// What an item at this rigor needs before it is accepted.
    pub fn required(self) -> &'static [&'static str] {
        match self {
            Rigor::Light => &["candidate_check", "accept_reason"],
            Rigor::Standard => &["candidate_check", "review"],
            Rigor::Strict | Rigor::Critical => &["test_first_item", "candidate_check", "review"],
        }
    }
}

/// Paths whose change is risky whatever its size: access control, stored
/// data, builds, CI and dependencies. A glob that names one makes its item
/// strict. Matching is by path part and errs on the strict side ("authors/"
/// counts as auth): too much rigor costs a review, too little a defect.
const SENSITIVE_WORDS: &[&str] = &["auth", "migration"];
const SENSITIVE_FILES: &[&str] = &[
    "cargo.toml",
    "cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "requirements.txt",
    "pyproject.toml",
    "go.mod",
    "go.sum",
    "gemfile",
];
const SENSITIVE_PREFIXES: &[&str] = &["dockerfile", ".github", ".gitlab-ci", "docker-compose"];
const SENSITIVE_SUFFIXES: &[&str] = &[".sql"];
/// The most globs a light item may change.
const LIGHT_GLOBS: usize = 3;
/// Work failures after which an item gets a stricter review.
const STRICT_AFTER_FAILURES: u32 = 2;

fn sensitive(glob: &str) -> bool {
    glob.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        SENSITIVE_WORDS.iter().any(|word| part.contains(word))
            || SENSITIVE_FILES.contains(&part.as_str())
            || SENSITIVE_PREFIXES
                .iter()
                .any(|prefix| part.starts_with(prefix))
            || SENSITIVE_SUFFIXES
                .iter()
                .any(|suffix| part.ends_with(suffix))
    })
}

/// Whether items are routed by risk (`task.toml` `routing`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    #[default]
    On,
    Off,
}

/// The rigor an item gets and why: the floor comes from facts about the item,
/// never from an agent's judgment, and a declared rigor may only raise it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Routing {
    pub kind: WorkKind,
    pub rigor: Rigor,
    pub floor: Rigor,
    pub why: Vec<String>,
}

impl Routing {
    /// The routing under the task's mode: with routing off nothing is light.
    pub fn under(mut self, mode: RoutingMode) -> Routing {
        if mode == RoutingMode::Off && self.rigor < Rigor::Standard {
            self.rigor = Rigor::Standard;
            self.why.push("routing is off for this task".into());
        }
        self
    }
}

impl Application {
    /// The routing of item `id` after `work_failures` failures of its own work.
    pub fn routing(&self, id: &str, work_failures: u32) -> Result<Routing> {
        let item = self
            .items
            .get(id)
            .with_context(|| format!("there is no item {id}"))?;
        let mut why = Vec::new();
        let touched: Vec<&String> = item.allowed.iter().filter(|glob| sensitive(glob)).collect();
        if !touched.is_empty() {
            why.push(format!(
                "sensitive path {}",
                touched
                    .iter()
                    .map(|glob| glob.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !item.data_effects.is_empty() {
            why.push("changes stored data".into());
        }
        for interface in self.shared_interfaces(id) {
            why.push(format!("shares interface {interface} with another lane"));
        }
        if work_failures >= STRICT_AFTER_FAILURES {
            why.push(format!("{work_failures} work failures"));
        }
        let floor = if !why.is_empty() {
            Rigor::Strict
        } else if self.light_shaped(id, item) {
            Rigor::Light
        } else {
            Rigor::Standard
        };
        Ok(Routing {
            kind: item.kind,
            rigor: item.rigor.map_or(floor, |declared| declared.max(floor)),
            floor,
            why,
        })
    }

    /// The interfaces item `id` provides or consumes that cross a lane.
    fn shared_interfaces(&self, id: &str) -> Vec<String> {
        let Some(item) = self.items.get(id) else {
            return Vec::new();
        };
        item.provides
            .iter()
            .chain(&item.consumes)
            .filter(|interface| {
                self.items.values().any(|other| {
                    other.lane != item.lane
                        && (other.provides.contains(interface)
                            || other.consumes.contains(interface))
                })
            })
            .cloned()
            .collect()
    }

    /// Items that provide or consume an interface whose spec this revision
    /// changes or removes: both sides start again on the new text.
    pub fn interface_changes(&self, old: &Application) -> BTreeSet<String> {
        let changed: BTreeSet<&String> = old
            .interfaces
            .iter()
            .filter(|(id, interface)| {
                self.interfaces
                    .get(*id)
                    .is_none_or(|new| new.spec != interface.spec)
            })
            .map(|(id, _)| id)
            .collect();
        self.items
            .iter()
            .chain(&old.items)
            .filter(|(_, item)| {
                item.provides
                    .iter()
                    .chain(&item.consumes)
                    .any(|interface| changed.contains(interface))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Sets each interface's version from `old`: kept while its spec stays,
    /// one up when it changes. The planner never writes versions.
    fn version_interfaces(&mut self, old: &Application) {
        for (id, interface) in &mut self.interfaces {
            interface.version = match old.interfaces.get(id) {
                Some(previous) if previous.spec == interface.spec => previous.version,
                Some(previous) => previous.version + 1,
                None => 1,
            };
        }
    }

    /// The interfaces an order for item `id` works against, for its text.
    pub fn interface_brief(&self, id: &str) -> String {
        let Some(item) = self.items.get(id) else {
            return String::new();
        };
        let mut brief = String::new();
        for (verb, ids) in [("You provide", &item.provides), ("You use", &item.consumes)] {
            for interface_id in ids {
                if let Some(interface) = self.interfaces.get(interface_id) {
                    brief.push_str(&format!(
                        "\n{verb} interface {interface_id} v{} ({:?}), exactly as specified; a change to it is a plan amendment:\n{}\n",
                        interface.version,
                        interface.kind,
                        interface.spec.trim_end()
                    ));
                }
            }
        }
        brief
    }

    fn validate_interfaces(&self, errors: &mut ValidationErrors) {
        for (id, interface) in &self.interfaces {
            errors.check(identifier(id));
            errors.check(text(&interface.spec, "spec").with_context(|| format!("interface {id}")));
            errors.require(interface.spec.len() <= MAX_SPEC_BYTES, || {
                format!("interface {id}: spec is over {MAX_SPEC_BYTES} bytes; keep the contract, link the rest")
            });
            match self.items.get(&interface.owner) {
                Some(owner) => {
                    errors.require(owner.provides.contains(id), || {
                        format!(
                            "interface {id}: its owner {} must list it in provides",
                            interface.owner
                        )
                    });
                    if let Some(check) = &interface.check {
                        errors.require(
                            self.check_configuration
                                .get(&owner.repo)
                                .is_some_and(|checks| checks.checks.contains_key(check)),
                            || {
                                format!(
                                    "interface {id}: check {check} is not a profile of {}",
                                    owner.repo
                                )
                            },
                        );
                    }
                }
                None => errors.require(false, || {
                    format!("interface {id}: unknown owner {}", interface.owner)
                }),
            }
        }
        for (id, item) in &self.items {
            for interface in &item.provides {
                match self.interfaces.get(interface) {
                    Some(found) => errors.require(found.owner == *id, || {
                        format!("{id} provides {interface}, owned by {}: an interface has exactly one owner", found.owner)
                    }),
                    None => errors.require(false, || format!("{id}: unknown interface {interface}")),
                }
            }
            for interface in &item.consumes {
                match self.interfaces.get(interface) {
                    Some(found) => errors.require(
                        found.owner != *id && item.dependencies.contains(&found.owner),
                        || format!("{id} consumes {interface}, owned by {}: add {} to its dependencies", found.owner, found.owner),
                    ),
                    None => errors.require(false, || format!("{id}: unknown interface {interface}")),
                }
            }
        }
    }

    /// Small work nothing else builds on: a change or bug fix in a few
    /// explicit globs that no item in another lane depends on.
    fn light_shaped(&self, id: &str, item: &Contract) -> bool {
        let depended_on_elsewhere = self.items.values().any(|other| {
            other.lane != item.lane && other.dependencies.iter().any(|dependency| dependency == id)
        });
        matches!(item.kind, WorkKind::Change | WorkKind::Bugfix)
            && item.allowed.len() <= LIGHT_GLOBS
            && item.allowed.iter().all(|glob| glob != "**" && glob != "*")
            && item.data_effects.is_empty()
            && !depended_on_elsewhere
    }
}

impl Contract {
    /// What an order for this item may not change: the leader's `requested`
    /// globs, and, unless the item writes them, its protected tests.
    pub fn forbidden_for_order(&self, requested: &[String]) -> Vec<String> {
        let mut forbidden = requested.to_vec();
        if self.writes_tests {
            return forbidden;
        }
        for glob in &self.protected_tests {
            if !forbidden.contains(glob) {
                forbidden.push(glob.clone());
            }
        }
        forbidden
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Milestone {
    pub version: String,
    pub title: String,
    pub required: Vec<String>,
    pub dependencies: Vec<String>,
    /// A release is never inferred from numbering, acceptance or a draft PR.
    pub release_record: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub previous: Application,
    pub reason: String,
    pub affected: Vec<String>,
}

fn text(value: &str, name: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{name} must not be empty");
    Ok(())
}
fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
        "invalid identifier: {value}"
    );
    Ok(())
}
fn paths(values: &[String]) -> Result<()> {
    for value in values {
        ensure!(
            !value.starts_with('/')
                && !value.contains('\\')
                && !value.split('/').any(|part| part == "..")
                && !value.trim().is_empty(),
            "path must stay in the project: {value}"
        );
    }
    crate::checks::glob_set(values)?;
    Ok(())
}
fn acyclic(graph: &BTreeMap<String, Vec<String>>) -> Result<()> {
    fn visit(
        name: &str,
        graph: &BTreeMap<String, Vec<String>>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        ensure!(active.insert(name.into()), "dependency cycle at {name}");
        for dependency in graph.get(name).context("unknown dependency")? {
            ensure!(
                graph.contains_key(dependency),
                "unknown dependency {dependency}"
            );
            visit(dependency, graph, active, done)?;
        }
        active.remove(name);
        done.insert(name.into());
        Ok(())
    }
    let (mut active, mut done) = (BTreeSet::new(), BTreeSet::new());
    for name in graph.keys() {
        visit(name, graph, &mut active, &mut done)?;
    }
    Ok(())
}
// Bound the feedback so a large invalid proposal cannot flood the next turn.
#[derive(Default)]
struct ValidationErrors {
    messages: Vec<String>,
    omitted: usize,
}

impl ValidationErrors {
    fn require(&mut self, valid: bool, message: impl FnOnce() -> String) {
        if !valid {
            if self.messages.len() < 16 {
                self.messages.push(message().chars().take(300).collect());
            } else {
                self.omitted += 1;
            }
        }
    }

    fn check<T>(&mut self, result: Result<T>) {
        if let Err(error) = result {
            self.require(false, || format!("{error:#}"));
        }
    }

    fn finish(mut self, label: &str) -> Result<()> {
        if self.messages.is_empty() {
            return Ok(());
        }
        if self.omitted > 0 {
            self.messages
                .push(format!("{} more issues omitted", self.omitted));
        }
        anyhow::bail!(
            "{label} failed:\n- {}\nUse proposal_example for a validated input example.",
            self.messages.join("\n- ")
        )
    }
}

impl Application {
    pub fn existing_tests(&self, repo: &str) -> &[String] {
        self.existing_protected_tests
            .get(repo)
            .map_or(&[], Vec::as_slice)
    }

    pub fn order_forbidden(&self, item: &Contract, requested: &[String]) -> Vec<String> {
        let mut forbidden = item.forbidden_for_order(requested);
        for path in self.existing_tests(&item.repo) {
            if !forbidden.contains(path) {
                forbidden.push(path.clone());
            }
        }
        forbidden
    }

    fn validate_existing_tests(&self, config: &crate::task::TaskConfig) -> Result<()> {
        let mut errors = ValidationErrors::default();
        for (repo, paths) in &self.existing_protected_tests {
            let Some(repository) = config.repos.iter().find(|entry| entry.name == *repo) else {
                errors.require(false, || {
                    format!("existing_protected_tests: unknown repo {repo}")
                });
                continue;
            };
            for path in paths {
                let literal = format!(":(literal){path}");
                let tracked = crate::checks::git(
                    &repository.worktree,
                    &["ls-tree", "-z", "HEAD", "--", &literal],
                )?;
                errors.require(
                    tracked.split_once('\t').is_some_and(|(header, name)| {
                        (header.starts_with("100644 blob ") || header.starts_with("100755 blob "))
                            && name.strip_suffix('\0') == Some(path)
                    }),
                    || format!("{repo}:{path} must be an existing tracked regular file"),
                );
            }
        }
        errors.finish("Existing protected tests")
    }

    pub fn validate(&self) -> Result<()> {
        let mut errors = ValidationErrors::default();
        errors.check(text(&self.goal, "application goal"));
        errors.require(self.revision > 0, || {
            "baseline revision must be positive".into()
        });
        let decision_graph = self
            .decisions
            .iter()
            .map(|decision| (decision.id.clone(), decision.prerequisites.clone()))
            .collect::<BTreeMap<_, _>>();
        errors.require(decision_graph.len() == self.decisions.len(), || {
            "duplicate decision IDs".into()
        });
        errors.check(acyclic(&decision_graph));
        for decision in &self.decisions {
            errors.check(identifier(&decision.id));
            errors.check(
                text(&decision.question, "question")
                    .with_context(|| format!("decision {}", decision.id)),
            );
            errors.check(
                text(&decision.recommendation, "recommendation")
                    .with_context(|| format!("decision {}", decision.id)),
            );
            if let Some(answer) = &decision.answer {
                errors.check(
                    text(answer, "answer").with_context(|| format!("decision {}", decision.id)),
                );
            }
            errors.require(
                !decision.one_way || decision.alternatives.len() >= 2,
                || {
                    format!(
                        "decision {} is one-way: give at least 2 alternatives with their tradeoffs",
                        decision.id
                    )
                },
            );
            for alternative in &decision.alternatives {
                errors.require(
                    !alternative.option.trim().is_empty()
                        && !alternative.tradeoffs.trim().is_empty(),
                    || {
                        format!(
                            "decision {}: each alternative needs an option and its tradeoffs",
                            decision.id
                        )
                    },
                );
            }
            errors.require(
                decision.criteria.len() <= 6
                    && decision
                        .criteria
                        .iter()
                        .all(|criterion| !criterion.trim().is_empty()),
                || {
                    format!(
                        "decision {}: give at most 6 non-empty criteria",
                        decision.id
                    )
                },
            );
        }
        self.validate_interfaces(&mut errors);
        errors.require(!self.scopes.is_empty(), || {
            "application needs scopes".into()
        });
        for paths_for_repo in self.existing_protected_tests.values() {
            errors.check(paths(paths_for_repo));
            for path in paths_for_repo {
                errors.require(
                    !path.ends_with('/') && !path.contains(['*', '?', '[', ']', '{', '}']),
                    || format!("existing protected test must name an exact file: {path}"),
                );
            }
        }
        for (id, title) in &self.scopes {
            errors.check(identifier(id));
            errors.check(text(title, "scope title").with_context(|| format!("scope {id}")));
        }
        for (id, item) in &self.items {
            errors.check(identifier(id));
            errors.require(self.scopes.contains_key(&item.scope), || {
                format!("{id}: unknown scope")
            });
            for (name, value) in [
                ("title", &item.title),
                ("role", &item.role),
                ("repo", &item.repo),
                ("lane", &item.lane),
            ] {
                errors.check(text(value, name).with_context(|| format!("item {id}")));
            }
            if let (Some(declared), Ok(routing)) = (item.rigor, self.routing(id, 0)) {
                errors.require(declared >= routing.floor, || {
                    format!(
                        "{id}: rigor {declared:?} is below its floor {:?} ({})",
                        routing.floor,
                        routing.why.join("; ")
                    )
                    .to_lowercase()
                });
            }
            errors.require(item.standards == STANDARDS_VERSION, || {
                format!("{id}: unknown coding standards version; expected {STANDARDS_VERSION}")
            });
            for (name, values) in [
                ("allowed", &item.allowed),
                ("reuse", &item.reuse),
                ("interfaces", &item.interfaces),
                ("behavior", &item.behavior),
                ("errors", &item.errors),
                ("tests", &item.tests),
                ("profiles", &item.profiles),
                ("acceptance", &item.acceptance),
            ] {
                errors.require(
                    !values.is_empty() && values.iter().all(|value| !value.trim().is_empty()),
                    || format!("{id}: missing {name}"),
                );
            }
            if let Some(checks) = self.check_configuration.get(&item.repo) {
                errors.require(
                    item.profiles
                        .iter()
                        .all(|profile| checks.checks.contains_key(profile)),
                    || {
                        format!(
                            "{id}: baseline check definitions are incomplete for repo {}",
                            item.repo
                        )
                    },
                );
            } else if !item.repo.trim().is_empty() {
                errors.require(false, || {
                    format!(
                        "{id}: baseline must declare required named check definitions for repo {}",
                        item.repo
                    )
                });
            }
            for (field, values) in [
                ("allowed", &item.allowed),
                ("supporting_files", &item.supporting_files),
                ("protected_tests", &item.protected_tests),
            ] {
                errors.check(paths(values).with_context(|| format!("{id}.{field}")));
            }
            if item.writes_tests && !self.existing_tests(&item.repo).is_empty() {
                errors.check((|| {
                    let allowed = crate::checks::glob_set(&item.allowed)?;
                    ensure!(
                        !self
                            .existing_tests(&item.repo)
                            .iter()
                            .any(|path| allowed.is_match(path)),
                        "{id}: test-writing scope overlaps an existing protected test"
                    );
                    Ok(())
                })());
            }
            errors.check(self.validate_protected_tests(id, item));
        }
        for repo in self
            .items
            .values()
            .map(|item| &item.repo)
            .collect::<BTreeSet<_>>()
        {
            if let Some(checks) = self.check_configuration.get(repo) {
                errors.check(
                    crate::check_profiles::validate(checks)
                        .with_context(|| format!("check_configuration.{repo}")),
                );
            }
        }
        errors.check(acyclic(
            &self
                .items
                .iter()
                .map(|(id, item)| (id.clone(), item.dependencies.clone()))
                .collect(),
        ));
        let mut versions = BTreeSet::new();
        for (id, milestone) in &self.milestones {
            errors.check(identifier(id));
            errors.check(
                text(&milestone.title, "milestone title")
                    .with_context(|| format!("milestone {id}")),
            );
            let parts = milestone.version.split('.').collect::<Vec<_>>();
            errors.require(
                parts.len() == 3
                    && parts.iter().all(|part| part.parse::<u32>().is_ok())
                    && versions.insert(&milestone.version),
                || format!("{id}: duplicate or invalid milestone version"),
            );
            errors.require(!milestone.required.is_empty(), || {
                format!("{id}: milestone requires implementation steps")
            });
            for item in &milestone.required {
                errors.require(self.items.contains_key(item), || {
                    format!("{id}: unknown milestone item {item}")
                });
            }
        }
        errors.require(
            !self.items.is_empty() && !self.milestones.is_empty(),
            || "baseline needs steps and milestones".into(),
        );
        for scope in self.scopes.keys() {
            errors.require(self.items.values().any(|item| &item.scope == scope), || {
                format!("scope {scope} has no execution items")
            });
        }
        for item in self.items.keys() {
            errors.require(
                self.milestones
                    .values()
                    .any(|milestone| milestone.required.contains(item)),
                || format!("item {item} has no milestone"),
            );
        }
        errors.check(acyclic(
            &self
                .milestones
                .iter()
                .map(|(id, milestone)| (id.clone(), milestone.dependencies.clone()))
                .collect(),
        ));
        errors.finish("Baseline validation")
    }
    /// A test-writing item can write its tests; an item judged by protected
    /// tests depends on the item that writes them, so they land first.
    fn validate_protected_tests(&self, id: &str, item: &Contract) -> Result<()> {
        if item.writes_tests || item.protected_tests.is_empty() {
            return Ok(());
        }
        let writers: Vec<&Contract> = item
            .dependencies
            .iter()
            .filter_map(|dependency| self.items.get(dependency))
            .filter(|dependency| dependency.writes_tests)
            .collect();
        ensure!(
            !writers.is_empty(),
            "{id}: must depend on the item that writes its protected tests"
        );
        for glob in &item.protected_tests {
            let protected = crate::checks::glob_set(std::slice::from_ref(glob))?;
            ensure!(
                !self
                    .existing_tests(&item.repo)
                    .iter()
                    .any(|path| protected.is_match(path)),
                "{id}: newly written protected tests overlap existing protected tests ({glob})"
            );
            let mut writable = false;
            for writer in &writers {
                writable |= crate::checks::within(&writer.allowed, glob)?;
            }
            ensure!(
                writable,
                "{id}: the test-writing item may not write its protected tests ({glob}); declare existing tracked tests separately in application.existing_protected_tests"
            );
        }
        Ok(())
    }
    pub fn resolved(&self) -> bool {
        self.decisions.iter().all(Decision::settled)
            && self.items.values().all(|item| item.questions.is_empty())
    }
    pub fn approved(&self) -> bool {
        self.approved_revision == Some(self.revision)
            && self.confirmed_revision == Some(self.revision)
            && self.resolved()
    }
    pub fn ready(&self, id: &str, state: &State) -> Result<&Contract> {
        ensure!(
            self.approved(),
            "application baseline is unresolved or not approved at this revision"
        );
        let item = self.items.get(id).with_context(|| {
            format!("unknown execution item {id}: reviewed orders require an item in the accepted application baseline")
        })?;
        for dependency in &item.dependencies {
            ensure!(
                state
                    .steps
                    .iter()
                    .any(|step| step.id == *dependency && step.status == StepStatus::Accepted)
                    || self.tests_to_build_on(dependency, item, state),
                "dependency {dependency} is not accepted"
            );
        }
        Ok(item)
    }

    /// Whether `item` may be built on its test-first dependency before that
    /// is accepted. Its tests fail until the implementation exists, so it
    /// cannot pass a whole-suite merge check alone: when its own checks
    /// passed and it is in the same lane, the implementation goes on top and
    /// the two merge together, the protected tests unchanged.
    fn tests_to_build_on(&self, dependency: &str, item: &Contract, state: &State) -> bool {
        let Some(tests) = self.items.get(dependency) else {
            return false;
        };
        let waiting = state.steps.iter().any(|step| {
            step.id == dependency
                && matches!(step.status, StepStatus::Reported | StepStatus::Failed)
        });
        let checked = state
            .orders
            .iter()
            .rev()
            .find(|order| order.step == dependency)
            .and_then(|order| state.check_evidence.get(&order.id))
            .is_some_and(|evidence| evidence.passed());
        tests.writes_tests
            && tests.repo == item.repo
            && tests.lane == item.lane
            && waiting
            && checked
    }
}

/// One small test-first proposal, served only when requested.
pub fn proposal_example() -> Result<serde_json::Value> {
    Ok(serde_json::from_str(include_str!(
        "../roles/standards/proposal-example.json"
    ))?)
}

/// A baseline the leader proposes itself for small work: one light item, no
/// planner, no test-first item.
pub fn light_proposal_example() -> Result<serde_json::Value> {
    Ok(serde_json::from_str(include_str!(
        "../roles/standards/proposal-example-light.json"
    ))?)
}

fn check_configuration_shape(errors: &mut ValidationErrors, value: &serde_json::Value) {
    use crate::check_profiles::{Profile, RepositoryChecks};
    use serde_json::Value;

    fn check_profiles(errors: &mut ValidationErrors, path: &str, profiles: &Value) -> bool {
        let mut missing_environment = false;
        if let Some(entries) = profiles.as_object() {
            for (name, value) in entries {
                let path = format!("{path}[{name:?}]");
                if value.is_object() && value.get("environment").is_none() {
                    missing_environment = true;
                    errors.require(false, || format!("{path}.environment is required"));
                    if value.get("command").and_then(Value::as_str).is_none() {
                        errors.require(false, || format!("{path}.command must be a string"));
                    }
                } else if let Err(error) = Profile::deserialize(value) {
                    errors.require(false, || format!("{path}: {error}"));
                }
            }
        }
        missing_environment
    }

    let Some(repositories) = value.as_object() else {
        return;
    };
    for (repo, value) in repositories {
        let path = format!("application.check_configuration[{repo:?}]");
        let Err(error) = RepositoryChecks::deserialize(value) else {
            continue;
        };
        errors.require(false, || {
            format!("{path}: {error}; use a repository object with environments and checks")
        });
        let mut missing_environment =
            check_profiles(errors, &format!("{path}.checks"), &value["checks"]);
        if let Some(lanes) = value["lanes"].as_object() {
            for (lane, profiles) in lanes {
                missing_environment |=
                    check_profiles(errors, &format!("{path}.lanes[{lane:?}]"), profiles);
            }
        }
        errors.require(!missing_environment, || {
            format!("{path}: set each profile's environment to a name declared under environments, e.g. \"default\" with environments: {{\"default\":{{}}}}")
        });
    }
}

/// Decode full submissions in one place for the Leader and worker adapters.
pub fn store_input(task: &Task, input: &serde_json::Value) -> Result<Vec<String>> {
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    fn check_entries<T: DeserializeOwned>(
        errors: &mut ValidationErrors,
        field: &str,
        value: &Value,
    ) {
        if let Some(entries) = value.as_object() {
            for (id, entry) in entries {
                errors.check(
                    T::deserialize(entry).with_context(|| format!("application.{field}.{id}")),
                );
            }
        }
    }

    let reason = input["reason"].as_str().unwrap_or_default();
    let application = &input["application"];
    let proposal = Application::deserialize(application);
    let mut errors = ValidationErrors::default();
    errors.check(text(reason, "reason"));
    if let Err(error) = &proposal {
        errors.require(false, || format!("application: {error}"));
        if let Some(decisions) = application["decisions"].as_array() {
            for (index, decision) in decisions.iter().enumerate() {
                errors.check(
                    Decision::deserialize(decision)
                        .with_context(|| format!("application.decisions[{index}]")),
                );
            }
        }
        check_entries::<Contract>(&mut errors, "items", &application["items"]);
        check_entries::<Milestone>(&mut errors, "milestones", &application["milestones"]);
        check_entries::<Vec<String>>(
            &mut errors,
            "existing_protected_tests",
            &application["existing_protected_tests"],
        );
        check_configuration_shape(&mut errors, &application["check_configuration"]);
    }
    errors.finish("Proposal input")?;
    store(task, proposal?, reason)
}

pub fn store(task: &Task, mut proposal: Application, reason: &str) -> Result<Vec<String>> {
    let _control = task.control_lock()?;
    text(reason, "change reason")?;
    let config = task.config()?;
    for repo in proposal
        .items
        .values()
        .map(|item| item.repo.clone())
        .collect::<BTreeSet<_>>()
    {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            proposal.check_configuration.entry(repo.clone())
        {
            if let Some(checks) = config.check_profiles.get(&repo) {
                entry.insert(checks.clone());
            }
        }
    }
    proposal.version_interfaces(&task.state()?.application);
    proposal.validate()?;
    proposal.validate_existing_tests(&config)?;
    proposal.confirmed_revision = None;
    proposal.approved_revision = None;
    // Release evidence is not writable through a plan submission.
    ensure!(
        proposal
            .milestones
            .values()
            .all(|milestone| milestone.release_record.is_none()),
        "release records cannot be supplied by a planner"
    );
    task.transaction(|tx| {
        let old = &tx.state.application;
        ensure!(
            proposal.revision == old.revision + 1,
            "baseline proposal must be the next revision"
        );
        let affected = old
            .items
            .keys()
            .chain(proposal.items.keys())
            .filter(|id| {
                old.items.get(*id) != proposal.items.get(*id)
                    || old.items.get(*id).is_some_and(|item| {
                        old.existing_tests(&item.repo) != proposal.existing_tests(&item.repo)
                    })
            })
            .cloned()
            .chain(proposal.interface_changes(old))
            .collect::<BTreeSet<_>>();
        for (id, milestone) in &old.milestones {
            if milestone.release_record.is_some() {
                ensure!(
                    proposal.milestones.get(id) == Some(milestone),
                    "released milestone cannot change"
                );
            }
        }
        tx.state.application_revisions.push(Revision {
            previous: old.clone(),
            reason: reason.into(),
            affected: affected.iter().cloned().collect(),
        });
        tx.state.application_proposal = Some(proposal);
        tx.log(
            Kind::Plan,
            None,
            None,
            &format!("application baseline revised: {reason}"),
        );
        Ok(affected.into_iter().collect())
    })
}
/// The changes to a baseline, and only those: what an amendment sends
/// instead of the whole application, which may be too big for one call.
/// `None` in a map removes that entry; a decision replaces the one with its id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Amendment {
    pub goal: Option<String>,
    pub non_goals: Option<Vec<String>>,
    pub scopes: BTreeMap<String, Option<String>>,
    pub items: BTreeMap<String, Option<Contract>>,
    pub milestones: BTreeMap<String, Option<Milestone>>,
    pub decisions: Vec<Decision>,
    pub check_configuration: BTreeMap<String, Option<crate::check_profiles::RepositoryChecks>>,
    pub existing_protected_tests: BTreeMap<String, Option<Vec<String>>>,
    pub interfaces: BTreeMap<String, Option<Interface>>,
}

/// Applies `changes` to `base` as the next revision after `current`.
fn amended(base: &Application, current: u64, changes: Amendment) -> Application {
    fn apply<T>(entries: &mut BTreeMap<String, T>, changes: BTreeMap<String, Option<T>>) {
        for (id, change) in changes {
            match change {
                Some(value) => entries.insert(id, value),
                None => entries.remove(&id),
            };
        }
    }
    let mut next = base.clone();
    next.revision = current + 1;
    if let Some(goal) = changes.goal {
        next.goal = goal;
    }
    if let Some(non_goals) = changes.non_goals {
        next.non_goals = non_goals;
    }
    apply(&mut next.scopes, changes.scopes);
    apply(&mut next.items, changes.items);
    apply(&mut next.milestones, changes.milestones);
    apply(&mut next.check_configuration, changes.check_configuration);
    apply(&mut next.interfaces, changes.interfaces);
    apply(
        &mut next.existing_protected_tests,
        changes.existing_protected_tests,
    );
    for decision in changes.decisions {
        match next.decisions.iter_mut().find(|old| old.id == decision.id) {
            Some(old) => *old = decision,
            None => next.decisions.push(decision),
        }
    }
    next
}

/// Proposes the next revision as `changes` to the pending proposal, when
/// there is one, so amendments add up, or to the baseline. It is checked and
/// stored as any proposal is.
pub fn amend(task: &Task, changes: Amendment, reason: &str) -> Result<Vec<String>> {
    let state = task.state()?;
    let current = state.application.revision;
    let base = state
        .application_proposal
        .as_ref()
        .filter(|proposal| proposal.revision == current + 1)
        .unwrap_or(&state.application);
    store(task, amended(base, current, changes), reason)
}

/// The baseline without its contracts' bodies, small enough to read in one
/// call: each item is its title, scope, role, lane and dependencies; `read`
/// gives the full contracts asked for.
pub fn outline(application: &Application) -> serde_json::Value {
    let mut outline = serde_json::to_value(application).unwrap_or_default();
    outline["items"] = application
        .items
        .iter()
        .map(|(id, item)| {
            (
                id.clone(),
                serde_json::json!({"title":item.title,"scope":item.scope,"role":item.role,"lane":item.lane,"dependencies":item.dependencies}),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    if !application.interfaces.is_empty() {
        outline["interfaces"] = application
            .interfaces
            .iter()
            .map(|(id, interface)| {
                (
                    id.clone(),
                    serde_json::json!({"version":interface.version,"owner":interface.owner,"kind":interface.kind}),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into();
    }
    outline
}

/// The full contracts of `ids`, with the revision they belong to.
pub fn items(application: &Application, ids: &[String]) -> Result<serde_json::Value> {
    let mut contracts = serde_json::Map::new();
    for id in ids {
        let item = application
            .items
            .get(id)
            .with_context(|| format!("revision {} has no item {id}", application.revision))?;
        contracts.insert(id.clone(), serde_json::to_value(item)?);
    }
    let interfaces: BTreeMap<&String, &Interface> = ids
        .iter()
        .filter_map(|id| application.items.get(id))
        .flat_map(|item| item.provides.iter().chain(&item.consumes))
        .filter_map(|id| application.interfaces.get_key_value(id))
        .collect();
    Ok(
        serde_json::json!({"revision":application.revision,"items":contracts,"interfaces":interfaces}),
    )
}

pub fn confirm(task: &Task, revision: u64, approve: bool) -> Result<()> {
    let _control = task.control_lock()?;
    let config = task.config()?;
    task.transaction(|tx| {
        let mut application = tx
            .state
            .application_proposal
            .take()
            .context("no baseline proposal awaiting decision")?;
        application.validate()?;
        application.validate_existing_tests(&config)?;
        ensure!(
            application.revision == revision,
            "baseline revision changed"
        );
        ensure!(
            application.resolved(),
            "resolve open interview/contract questions first"
        );
        if approve {
            let needs_user = user_approval_needed(&tx.state.application, &application);
            // A revision of the leader's has no understanding to confirm.
            ensure!(
                needs_user.is_none() || application.confirmed_revision == Some(revision),
                "confirm shared understanding before approving baseline"
            );
            let approver = match &needs_user {
                Some(_) => "user approved exact baseline revision".to_string(),
                None => "the leader approved revision".to_string(),
            };
            let old = &tx.state.application;
            let global_change = old.revision > 0
                && (old.goal != application.goal || old.non_goals != application.non_goals);
            // Only what the revision changed is reset: work that depends on it
            // keeps its status, and the leader checks whether it still fits.
            let removed: BTreeSet<String> = old
                .items
                .keys()
                .filter(|id| !application.items.contains_key(*id))
                .cloned()
                .collect();
            let changed: BTreeSet<String> = old
                .items
                .iter()
                .filter(|(id, item)| {
                    application.items.get(*id).is_some_and(|new| {
                        new != *item || old.existing_tests(&item.repo) != application.existing_tests(&item.repo)
                    })
                })
                .map(|(id, _)| id.clone())
                .chain(
                    application
                        .interface_changes(old)
                        .into_iter()
                        .filter(|id| application.items.contains_key(id)),
                )
                .collect();
            let mut dependents = BTreeSet::new();
            loop {
                let additional = application
                    .items
                    .iter()
                    .filter(|(id, item)| {
                        !changed.contains(*id)
                            && !dependents.contains(*id)
                            && item.dependencies.iter().any(|dependency| {
                                changed.contains(dependency) || dependents.contains(dependency)
                            })
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                if additional.is_empty() {
                    break;
                }
                dependents.extend(additional);
            }
            let affected: BTreeSet<String> = changed.union(&removed).cloned().collect();
            let orders = tx
                .state
                .orders
                .iter()
                .filter(|order| affected.contains(&order.step))
                .map(|order| order.id.clone())
                .collect::<BTreeSet<_>>();
            for order in &mut tx.state.orders {
                if orders.contains(&order.id) {
                    order.status = crate::state::OrderStatus::Closed;
                }
            }
            tx.state.queue.retain(|message| {
                !message
                    .order
                    .as_ref()
                    .is_some_and(|order| orders.contains(order))
            });
            tx.state
                .check_evidence
                .retain(|order, _| !orders.contains(order));
            let now = crate::log::now();
            let mut reset = Vec::new();
            for step in &mut tx.state.steps {
                if removed.contains(&step.id) {
                    step.set_status(crate::state::StepStatus::Closed, now);
                } else if changed.contains(&step.id) {
                    step.set_status(crate::state::StepStatus::Rejected, now);
                    reset.push(step.id.clone());
                }
            }
            for step in &reset {
                crate::failures::record(
                    &mut tx.state,
                    step,
                    None,
                    crate::failures::FailureKind::RevisionReset,
                    &format!("revision {revision} changed its contract"),
                    now,
                );
            }
            let standing = |id: &String| {
                let status = tx
                    .state
                    .steps
                    .iter()
                    .find(|step| &step.id == id)
                    .map(|step| format!("{:?}", step.status).to_lowercase())
                    .unwrap_or_else(|| "not started".into());
                format!("{id} ({status})")
            };
            let mut told = format!("Revision {revision} is approved.");
            if !reset.is_empty() {
                told.push_str(&format!(
                    " It changed {}: reset (their orders closed, their evidence dropped); order them again, no diagnosis needed.",
                    reset.join(", ")
                ));
            }
            if !removed.is_empty() {
                told.push_str(&format!(
                    " It removed {}: closed.",
                    removed.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !dependents.is_empty() {
                told.push_str(&format!(
                    " Work that depends on the changes kept its status: {}. Check each still fits (application status items, read_file, git_diff) and re-order only what needs it.",
                    dependents.iter().map(standing).collect::<Vec<_>>().join(", ")
                ));
            }
            if global_change {
                told.push_str(
                    " The goal or non-goals changed: check every item not accepted yet against them.",
                );
            }
            if !affected.is_empty() || !dependents.is_empty() || global_change {
                tx.state.say(crate::state::LEADER, told);
            }
            // The leader's own revision has no confirmation round: approving
            // it is the confirmation, or `approved()` would refuse every order.
            if needs_user.is_none() {
                application.confirmed_revision = Some(revision);
            }
            application.approved_revision = Some(revision);
            tx.state.application = application;
            tx.state.approved = true;
            tx.log(
                Kind::Approve,
                None,
                None,
                &format!(
                    "{approver} {revision}; affected contracts: {}",
                    affected.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        } else {
            application.confirmed_revision = Some(revision);
            tx.state.application_proposal = Some(application);
            tx.log(
                Kind::Approve,
                None,
                None,
                "user confirmed planning understanding",
            );
        }
        Ok(())
    })
}
/// Why `proposal` needs the user's approval, or None when the leader may
/// approve it. The user approves what the work is for and what judges it:
/// the first baseline; the goal, non-goals, decisions, scopes and milestones;
/// removing an item; an item's protected tests taken away or whether it
/// writes tests; a weaker check. How a step is carried out (its files,
/// behavior, acceptance details, tests) and new steps are the leader's, who
/// tells the user in its next summary.
pub fn user_approval_needed(approved: &Application, proposal: &Application) -> Option<String> {
    if approved.revision == 0 {
        return Some("it is the first baseline".into());
    }
    for (repo, paths) in &approved.existing_protected_tests {
        if paths
            .iter()
            .any(|path| !proposal.existing_tests(repo).contains(path))
        {
            return Some(format!(
                "it takes existing protected tests away from {repo}"
            ));
        }
    }
    for (id, old) in &approved.items {
        let Some(new) = proposal.items.get(id) else {
            return Some(format!("it removes the item {id}"));
        };
        if old
            .protected_tests
            .iter()
            .any(|glob| !new.protected_tests.contains(glob))
        {
            return Some(format!("it takes protected tests away from {id}"));
        }
        if old.writes_tests != new.writes_tests {
            return Some(format!("it changes whether {id} writes tests"));
        }
        // The rigor an item is routed at, not only what was written down:
        // a new kind or fewer globs can make an approved item light too.
        if let (Ok(before), Ok(after)) = (approved.routing(id, 0), proposal.routing(id, 0))
            && after.rigor < before.rigor
        {
            return Some(format!("it lowers the rigor of {id}"));
        }
    }
    if approved.goal != proposal.goal
        || approved.non_goals != proposal.non_goals
        || approved.decisions != proposal.decisions
        || approved.scopes != proposal.scopes
        || approved.milestones != proposal.milestones
    {
        return Some(
            "it changes the plan (goal, non-goals, decisions, scopes or milestones)".into(),
        );
    }
    for (repo, old) in &approved.check_configuration {
        let Some(new) = proposal.check_configuration.get(repo) else {
            return Some(format!("it removes {repo}'s checks"));
        };
        if let Some(why) = weakened(old, new) {
            return Some(format!("it {why} in {repo}"));
        }
    }
    None
}

/// What a check change takes away from `old`, if anything: additions are fine.
fn weakened(
    old: &crate::check_profiles::RepositoryChecks,
    new: &crate::check_profiles::RepositoryChecks,
) -> Option<String> {
    fn kept<T: PartialEq>(old: &BTreeMap<String, T>, new: &BTreeMap<String, T>) -> Option<String> {
        old.iter()
            .find(|(name, value)| new.get(*name) != Some(*value))
            .map(|(name, _)| name.clone())
    }
    if let Some(name) = kept(&old.environments, &new.environments) {
        return Some(format!("changes or removes the environment {name}"));
    }
    if let Some(name) = kept(&old.checks, &new.checks) {
        return Some(format!("changes or removes the check {name}"));
    }
    for (list, old_names, new_names) in [
        ("selected", &old.selected, &new.selected),
        ("candidate", &old.candidate, &new.candidate),
    ] {
        if let Some(name) = old_names.iter().find(|name| !new_names.contains(name)) {
            return Some(format!("takes {name} out of the {list} checks"));
        }
    }
    for (lane, profiles) in &old.lanes {
        let empty = BTreeMap::new();
        if let Some(name) = kept(profiles, new.lanes.get(lane).unwrap_or(&empty)) {
            return Some(format!("changes or removes the lane {lane}'s check {name}"));
        }
    }
    None
}

/// The questions the user is asked now: one-way decisions without an answer
/// whose prerequisites are settled. A reversible decision is not asked; it
/// takes its recommendation (`assumed`), which the user may still overrule.
pub fn question_frontier(application: &Application) -> Vec<&Decision> {
    answerable(application)
        .into_iter()
        .filter(|decision| decision.one_way)
        .collect()
}

/// Decisions the user may answer now: unanswered, prerequisites settled.
pub fn answerable(application: &Application) -> Vec<&Decision> {
    application
        .decisions
        .iter()
        .filter(|decision| {
            decision.answer.is_none()
                && decision.prerequisites.iter().all(|id| {
                    application
                        .decisions
                        .iter()
                        .any(|prerequisite| prerequisite.id == *id && prerequisite.settled())
                })
        })
        .collect()
}

/// Reversible decisions taking their recommendation: shown at confirmation,
/// so the user sees what was assumed for them.
pub fn assumed(application: &Application) -> Vec<serde_json::Value> {
    application
        .decisions
        .iter()
        .filter(|decision| !decision.one_way && decision.answer.is_none())
        .map(|decision| {
            serde_json::json!({"id": decision.id, "question": decision.question, "recommendation": decision.recommendation})
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn approved() -> Application {
        Application {
            revision: 6,
            approved_revision: Some(6),
            confirmed_revision: Some(6),
            ..baseline()
        }
    }

    fn next(change: impl FnOnce(&mut Application)) -> Application {
        let mut proposal = approved();
        proposal.revision = 7;
        proposal.approved_revision = None;
        proposal.confirmed_revision = None;
        change(&mut proposal);
        proposal
    }

    #[test]
    fn proposal_example_passes_the_submission_validator() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let example = proposal_example().unwrap();
        assert_eq!(store_input(&task, &example).unwrap().len(), 2);
        let state = task.state().unwrap();
        let pending = state.application_proposal.unwrap();
        assert_eq!(pending.revision, 1);
        assert_eq!(pending.approved_revision, None);
        assert_eq!(
            pending.items["status-command"].protected_tests,
            ["tests/test_status.py"]
        );
    }

    #[test]
    fn the_light_proposal_example_passes_the_validator_and_routes_light() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        store_input(&task, &light_proposal_example().unwrap()).unwrap();
        let pending = task.state().unwrap().application_proposal.unwrap();
        let routing = pending.routing("readme-port", 0).unwrap();
        assert_eq!(routing.rigor, Rigor::Light);
    }

    #[test]
    fn malformed_proposal_reports_independent_input_errors_without_writing() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let before = task.state().unwrap();
        let input = serde_json::json!({"application":{
            "items":{"api":{"allowed":"api.py"}},
            "decisions":[{"id":"choice"}],
            "check_configuration":{"app":"python3 -m unittest"}
        }});
        let result = store_input(&task, &input).unwrap_err().to_string();
        for expected in [
            "reason",
            "application.items.api",
            "application.decisions[0]",
            "application.check_configuration[\"app\"]",
        ] {
            assert!(result.contains(expected), "{result}");
        }
        assert_eq!(task.state().unwrap(), before);
    }

    #[test]
    fn malformed_named_checks_identify_profiles_in_the_repo_and_lane() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let before = task.state().unwrap();
        let input = serde_json::json!({
            "reason": "Check the diagnostic",
            "application": {"check_configuration": {"notes": {
                "checks": {"unit": {"command": "python3 -m unittest"}},
                "lanes": {"backend": {"lint": {"command": 41, "environment": "default"}}}
            }}}
        });
        let error = store_input(&task, &input).unwrap_err().to_string();
        assert!(
            error.contains("check_configuration[\"notes\"].checks[\"unit\"].environment"),
            "{error}"
        );
        assert!(error.contains("environments: {\"default\":{}}"), "{error}");
        assert!(
            error.contains("check_configuration[\"notes\"].lanes[\"backend\"][\"lint\"]"),
            "{error}"
        );
        assert_eq!(task.state().unwrap(), before);
    }

    #[test]
    fn baseline_reports_multiple_semantic_errors_within_a_contract() {
        let mut proposal = baseline();
        let api = proposal.items.get_mut("S01-B01").unwrap();
        api.reuse.clear();
        api.interfaces.clear();
        api.standards = "unknown".into();
        api.allowed = vec!["../outside".into()];
        let result = proposal.validate().unwrap_err().to_string();
        for expected in [
            "S01-B01: missing reuse",
            "S01-B01: missing interfaces",
            "expected 1",
            "path must stay in the project",
        ] {
            assert!(result.contains(expected), "{result}");
        }
    }

    #[test]
    fn feedback_stays_bounded_for_large_invalid_proposals() {
        let mut errors = ValidationErrors::default();
        for _ in 0..40 {
            errors.require(false, || "a".repeat(1_000));
        }
        let result = errors.finish("Proposal").unwrap_err().to_string();
        assert!(result.contains("24 more issues omitted"));
        assert!(result.len() < 5_000);
    }

    #[test]
    fn a_revision_that_only_adds_checks_is_the_leaders_to_approve() {
        let added = next(|proposal| {
            let checks = proposal.check_configuration.get_mut("app").unwrap();
            let lint = checks.checks["backend"].clone();
            checks.lanes.insert(
                "back".into(),
                BTreeMap::from([("lint".into(), lint.clone())]),
            );
            checks.checks.insert("pint".into(), lint);
            checks.selected.push("pint".into());
        });
        assert_eq!(user_approval_needed(&approved(), &added), None);
        let new_repo = next(|proposal| {
            let checks = proposal.check_configuration["app"].clone();
            proposal.check_configuration.insert("api".into(), checks);
        });
        assert_eq!(user_approval_needed(&approved(), &new_repo), None);
    }

    // A test-first item's tests fail until their implementation exists, so
    // it cannot pass a whole-suite merge check alone: the implementation is
    // built on it in the same lane once its own checks passed, and the two
    // merge together, the branch never red.
    #[test]
    fn an_implementation_starts_on_its_test_first_item_in_the_same_lane() {
        use crate::check_profiles::{Evidence, Outcome, Phase};
        use crate::state::{Order, OrderStatus, State, Step, StepStatus};
        let mut application = approved();
        let base = application.items["S01-B01"].clone();
        let item = |lane: &str, writes_tests: bool, dependencies: Vec<String>| Contract {
            lane: lane.into(),
            writes_tests,
            dependencies,
            ..base.clone()
        };
        application
            .items
            .insert("B4-AT".into(), item("front", true, vec![]));
        application
            .items
            .insert("B4".into(), item("front", false, vec!["B4-AT".into()]));
        application
            .items
            .insert("D4".into(), item("docs", false, vec!["B4-AT".into()]));
        application
            .items
            .insert("B5".into(), item("front", false, vec!["B4".into()]));
        let state = |status: StepStatus, passed: bool| {
            let mut state = State::default();
            for (id, status) in [("B4-AT", status), ("B4", StepStatus::Reported)] {
                state.steps.push(Step {
                    id: id.into(),
                    title: id.into(),
                    status,
                    attempts: 1,
                    spec: None,
                    review_since: None,
                });
                state.orders.push(Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: format!("o-{id}"),
                    step: id.into(),
                    agent: "hammer-28".into(),
                    text: String::new(),
                    allowed: Vec::new(),
                    forbidden: Vec::new(),
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Closed,
                    delivered: true,
                    stalled: false,
                    plan: None,
                });
                state.check_evidence.insert(
                    format!("o-{id}"),
                    Evidence {
                        order: format!("o-{id}"),
                        revision: 1,
                        base: "abc".into(),
                        head: "def".into(),
                        configuration: String::new(),
                        run: std::path::PathBuf::new(),
                        phases: vec![Phase {
                            profile: "lint".into(),
                            outcome: if passed {
                                Outcome::Passed
                            } else {
                                Outcome::TestFailure
                            },
                            exit_code: Some(0),
                            seconds: 1,
                            diagnostic: String::new(),
                            log: std::path::PathBuf::new(),
                        }],
                    },
                );
            }
            state
        };

        for status in [StepStatus::Reported, StepStatus::Failed] {
            application.ready("B4", &state(status, true)).unwrap();
        }
        let error = |id: &str, state: State| application.ready(id, &state).unwrap_err().to_string();
        assert!(
            error("D4", state(StepStatus::Reported, true)).contains("B4-AT is not accepted"),
            "another lane waits for the merge"
        );
        assert!(
            error("B4", state(StepStatus::Reported, false)).contains("not accepted"),
            "its own checks must pass"
        );
        assert!(
            error("B4", state(StepStatus::Rejected, true)).contains("not accepted"),
            "tests sent back are not built on"
        );
        assert!(
            error("B5", state(StepStatus::Accepted, true)).contains("B4 is not accepted"),
            "only test-first items"
        );
    }

    // How a step is carried out (its files, behavior, acceptance details,
    // tests) and new steps are the leader's: the user approved what the work
    // is for, not each step's contract.
    #[test]
    fn an_items_execution_change_or_a_new_item_is_the_leaders() {
        let changed = next(|proposal| {
            let item = proposal.items.get_mut("S01-B01").unwrap();
            item.title = "Token service, clock frozen".into();
            item.allowed
                .push("backend/tests/Support/Counting.php".into());
            item.behavior
                .push("Delete the leftover support file".into());
            item.acceptance = vec!["The full back suite has no failures".into()];
            item.protected_tests
                .push("backend/tests/Feature/New.php".into());
        });
        assert_eq!(user_approval_needed(&approved(), &changed), None);
        let added = next(|proposal| {
            let mut item = proposal.items["S01-B01"].clone();
            item.title = "Fix time rot".into();
            proposal.items.insert("S01-B02".into(), item);
        });
        assert_eq!(user_approval_needed(&approved(), &added), None);
    }

    #[test]
    fn a_plan_change_or_a_weaker_check_is_the_users() {
        let why = |proposal: Application| user_approval_needed(&approved(), &proposal).unwrap();
        assert!(
            why(next(|proposal| proposal.goal = "Another".into())).contains("changes the plan")
        );
        assert!(
            why(next(|proposal| {
                proposal.items.remove("S01-B01");
                for milestone in proposal.milestones.values_mut() {
                    milestone.required.retain(|item| item != "S01-B01");
                }
            }))
            .contains("removes the item S01-B01")
        );
        assert!(
            why(next(|proposal| {
                proposal.milestones.values_mut().next().unwrap().title = "Another".into();
            }))
            .contains("changes the plan")
        );
        let mut guarded = approved();
        guarded.items.get_mut("S01-B01").unwrap().protected_tests =
            vec!["backend/tests/Feature/Token.php".into()];
        let mut unguarded = guarded.clone();
        unguarded.revision = 7;
        unguarded
            .items
            .get_mut("S01-B01")
            .unwrap()
            .protected_tests
            .clear();
        assert!(
            user_approval_needed(&guarded, &unguarded)
                .unwrap()
                .contains("takes protected tests away from S01-B01")
        );
        assert!(
            why(next(|proposal| {
                let item = proposal.items.get_mut("S01-B01").unwrap();
                item.writes_tests = !item.writes_tests;
            }))
            .contains("whether S01-B01 writes tests")
        );
        assert!(
            why(next(|proposal| {
                proposal
                    .check_configuration
                    .get_mut("app")
                    .unwrap()
                    .checks
                    .get_mut("backend")
                    .unwrap()
                    .command = "true # skip".into();
            }))
            .contains("changes or removes the check backend in app")
        );
        assert!(
            why(next(|proposal| {
                proposal
                    .check_configuration
                    .get_mut("app")
                    .unwrap()
                    .selected
                    .clear();
            }))
            .contains("takes backend out of the selected checks")
        );
        assert!(
            why(next(|proposal| {
                proposal.check_configuration.remove("app");
            }))
            .contains("removes app's checks")
        );
        let first = Application {
            revision: 1,
            ..baseline()
        };
        assert!(
            user_approval_needed(&Application::default(), &first)
                .unwrap()
                .contains("first baseline")
        );
    }
    pub fn baseline() -> Application {
        let item = Contract {
            scope: "S01".into(),
            title: "Token service".into(),
            role: "backend-data".into(),
            repo: "app".into(),
            lane: "back".into(),
            allowed: vec!["backend/**".into()],
            reuse: vec!["ExistingRepository".into()],
            interfaces: vec!["issue() -> Token".into()],
            behavior: vec!["Issues token".into()],
            errors: vec!["Reject invalid account".into()],
            tests: vec!["Cannot access another tenant".into()],
            profiles: vec!["backend".into()],
            acceptance: vec!["Service contract passes".into()],
            standards: STANDARDS_VERSION.into(),
            ..Default::default()
        };
        Application {
            revision: 1,
            check_configuration:BTreeMap::from([("app".into(),serde_json::from_value(serde_json::json!({"environments":{"testing":{}},"checks":{"backend":{"command":"true","environment":"testing"}},"selected":["backend"]})).unwrap())]),
            goal: "Application".into(),
            scopes: BTreeMap::from([("S01".into(), "Accounts".into())]),
            items: BTreeMap::from([("S01-B01".into(), item)]),
            milestones: BTreeMap::from([(
                "M01".into(),
                Milestone {
                    version: "1.1.0".into(),
                    title: "Accounts ready".into(),
                    required: vec!["S01-B01".into()],
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
    }
    fn routed(change: impl FnOnce(&mut Contract)) -> Application {
        let mut application = baseline();
        change(application.items.get_mut("S01-B01").unwrap());
        application
    }

    #[test]
    fn a_small_change_nothing_builds_on_is_light() {
        let application = routed(|item| {
            item.kind = WorkKind::Change;
            item.allowed = vec!["README.md".into()];
        });
        let routing = application.routing("S01-B01", 0).unwrap();
        assert_eq!((routing.floor, routing.rigor), (Rigor::Light, Rigor::Light));
        assert_eq!(
            Rigor::Light.required(),
            ["candidate_check", "accept_reason"]
        );
    }

    #[test]
    fn sensitive_paths_and_data_effects_make_an_item_strict_whatever_its_size() {
        let application = routed(|item| {
            item.kind = WorkKind::Change;
            item.allowed = vec!["api/auth/**".into(), "migrations/0002.sql".into()];
            item.data_effects = vec!["adds sessions table".into()];
        });
        let routing = application.routing("S01-B01", 0).unwrap();
        assert_eq!(routing.floor, Rigor::Strict);
        assert_eq!(
            routing.why,
            [
                "sensitive path api/auth/**, migrations/0002.sql",
                "changes stored data"
            ]
        );
        for glob in [
            "Dockerfile",
            ".github/workflows/ci.yml",
            "web/package.json",
            "db/schema.sql",
        ] {
            assert!(sensitive(glob), "{glob}");
        }
        assert!(!sensitive("docs/guide.md"));
    }

    #[test]
    fn features_broad_globs_and_shared_work_stay_standard() {
        let feature = routed(|item| item.allowed = vec!["src/notes.py".into()]);
        assert_eq!(
            feature.routing("S01-B01", 0).unwrap().floor,
            Rigor::Standard,
            "kind defaults to feature"
        );

        let broad = routed(|item| {
            item.kind = WorkKind::Bugfix;
            item.allowed = vec!["**".into()];
        });
        assert_eq!(broad.routing("S01-B01", 0).unwrap().floor, Rigor::Standard);

        let mut shared = routed(|item| {
            item.kind = WorkKind::Change;
            item.allowed = vec!["api/notes.py".into()];
        });
        let mut client = shared.items["S01-B01"].clone();
        client.lane = "front".into();
        client.dependencies = vec!["S01-B01".into()];
        shared.items.insert("S01-F01".into(), client);
        assert_eq!(
            shared.routing("S01-B01", 0).unwrap().floor,
            Rigor::Standard,
            "another lane builds on it"
        );
    }

    #[test]
    fn repeated_work_failures_raise_the_rigor_and_a_declared_rigor_only_raises_it() {
        let application = routed(|item| {
            item.kind = WorkKind::Change;
            item.allowed = vec!["README.md".into()];
            item.rigor = Some(Rigor::Standard);
        });
        let routing = application.routing("S01-B01", 0).unwrap();
        assert_eq!(
            (routing.floor, routing.rigor),
            (Rigor::Light, Rigor::Standard)
        );
        let routing = application.routing("S01-B01", 2).unwrap();
        assert_eq!(routing.rigor, Rigor::Strict);
        assert_eq!(routing.why, ["2 work failures"]);
    }

    #[test]
    fn with_routing_off_nothing_is_light() {
        let application = routed(|item| {
            item.kind = WorkKind::Change;
            item.allowed = vec!["README.md".into()];
        });
        let routing = application
            .routing("S01-B01", 0)
            .unwrap()
            .under(RoutingMode::Off);
        assert_eq!(routing.rigor, Rigor::Standard);
        assert_eq!(routing.why, ["routing is off for this task"]);
        let strict = routed(|item| item.allowed = vec!["api/auth/**".into()])
            .routing("S01-B01", 0)
            .unwrap()
            .under(RoutingMode::Off);
        assert_eq!(strict.rigor, Rigor::Strict, "off never lowers");
        let config: crate::task::TaskConfig =
            toml::from_str("id = \"t\"\nbranch = \"b\"\nworkspace = \"w\"\n").unwrap();
        assert_eq!(config.routing, RoutingMode::On, "older task.toml routes");
    }

    #[test]
    fn a_declared_rigor_below_the_floor_is_refused() {
        let application = routed(|item| {
            item.allowed = vec!["api/auth/**".into()];
            item.rigor = Some(Rigor::Light);
        });
        let error = application.validate().unwrap_err().to_string();
        assert!(
            error.contains(
                "s01-b01: rigor light is below its floor strict (sensitive path api/auth/**)"
            ),
            "{error}"
        );
    }

    #[test]
    fn lowering_an_approved_items_rigor_needs_the_user() {
        let mut approved = approved();
        approved.items.get_mut("S01-B01").unwrap().rigor = Some(Rigor::Strict);
        let mut proposal = approved.clone();
        proposal.revision = 7;
        proposal.items.get_mut("S01-B01").unwrap().rigor = None;
        assert_eq!(
            user_approval_needed(&approved, &proposal).as_deref(),
            Some("it lowers the rigor of S01-B01")
        );
        proposal.items.get_mut("S01-B01").unwrap().rigor = Some(Rigor::Critical);
        assert_eq!(
            user_approval_needed(&approved, &proposal),
            None,
            "raising is free"
        );
    }

    #[test]
    fn a_revision_that_makes_an_approved_item_light_needs_the_user() {
        let mut approved = approved();
        approved.items.get_mut("S01-B01").unwrap().allowed = vec!["src/a.py".into()];
        let mut proposal = approved.clone();
        proposal.revision = 7;
        proposal.items.get_mut("S01-B01").unwrap().kind = WorkKind::Change;
        assert_eq!(
            user_approval_needed(&approved, &proposal).as_deref(),
            Some("it lowers the rigor of S01-B01"),
            "feature to change drops it from standard to light"
        );

        let mut light = approved.clone();
        light.items.get_mut("S01-B01").unwrap().kind = WorkKind::Change;
        let mut written = light.clone();
        written.revision = 7;
        written.items.get_mut("S01-B01").unwrap().rigor = Some(Rigor::Light);
        assert_eq!(
            user_approval_needed(&light, &written),
            None,
            "writing down the rigor it already had lowers nothing"
        );
    }

    #[test]
    fn an_old_baseline_without_kind_or_rigor_loads_as_a_standard_feature() {
        let mut value = serde_json::to_value(baseline().items["S01-B01"].clone()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("kind");
        assert!(object.get("rigor").is_none());
        let item: Contract = serde_json::from_value(value).unwrap();
        assert_eq!((item.kind, item.rigor), (WorkKind::Feature, None));
    }

    /// A back item that provides notes-api and a front item that uses it.
    fn interfaced() -> Application {
        let mut application = baseline();
        let back = application.items.get_mut("S01-B01").unwrap();
        back.provides = vec!["notes-api".into()];
        let mut front = back.clone();
        front.lane = "front".into();
        front.provides.clear();
        front.consumes = vec!["notes-api".into()];
        front.dependencies = vec!["S01-B01".into()];
        application.items.insert("S01-F01".into(), front);
        application
            .milestones
            .get_mut("M01")
            .unwrap()
            .required
            .push("S01-F01".into());
        application.interfaces.insert(
            "notes-api".into(),
            Interface {
                version: 1,
                owner: "S01-B01".into(),
                kind: InterfaceKind::Http,
                spec: "GET /notes -> 200 {notes: [{id: int, title: str}]}".into(),
                check: None,
            },
        );
        application
    }

    #[test]
    fn an_interface_has_one_owner_and_its_consumers_depend_on_it() {
        interfaced().validate().unwrap();
        let errors = |change: fn(&mut Application)| {
            let mut application = interfaced();
            change(&mut application);
            application.validate().unwrap_err().to_string()
        };
        let error = errors(|application| {
            application
                .items
                .get_mut("S01-F01")
                .unwrap()
                .dependencies
                .clear();
        });
        assert!(
            error.contains(
                "S01-F01 consumes notes-api, owned by S01-B01: add S01-B01 to its dependencies"
            ),
            "{error}"
        );
        let error = errors(|application| {
            application.items.get_mut("S01-F01").unwrap().provides = vec!["notes-api".into()];
        });
        assert!(
            error.contains("an interface has exactly one owner"),
            "{error}"
        );
        let error = errors(|application| {
            application.items.get_mut("S01-F01").unwrap().consumes = vec!["auth-api".into()];
        });
        assert!(
            error.contains("S01-F01: unknown interface auth-api"),
            "{error}"
        );
        let error = errors(|application| {
            application.interfaces.get_mut("notes-api").unwrap().spec = "x".repeat(5_000);
        });
        assert!(error.contains("spec is over 4096 bytes"), "{error}");
        let error = errors(|application| {
            application.interfaces.get_mut("notes-api").unwrap().check = Some("contract".into());
        });
        assert!(
            error.contains("check contract is not a profile of app"),
            "{error}"
        );
    }

    #[test]
    fn items_sharing_an_interface_across_lanes_are_strict() {
        let application = interfaced();
        for id in ["S01-B01", "S01-F01"] {
            let routing = application.routing(id, 0).unwrap();
            assert_eq!(routing.floor, Rigor::Strict, "{id}");
            assert_eq!(
                routing.why,
                ["shares interface notes-api with another lane"]
            );
        }
    }

    #[test]
    fn a_spec_change_raises_the_version_and_resets_both_sides() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let approved = Application {
            confirmed_revision: Some(1),
            approved_revision: Some(1),
            ..interfaced()
        };
        task.transaction(|tx| {
            tx.state.application = approved.clone();
            for (id, status) in [
                ("S01-B01", crate::state::StepStatus::Accepted),
                ("S01-F01", crate::state::StepStatus::Reported),
            ] {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: id.into(),
                    status,
                    attempts: 1,
                    spec: None,
                    review_since: None,
                });
            }
            Ok(())
        })
        .unwrap();

        let mut same = approved.clone();
        same.revision = 2;
        same.interfaces.get_mut("notes-api").unwrap().version = 9;
        store(&task, same, "only the goal wording").unwrap();
        assert_eq!(
            task.state()
                .unwrap()
                .application_proposal
                .unwrap()
                .interfaces["notes-api"]
                .version,
            1,
            "the planner never sets the version; an unchanged spec keeps it"
        );

        let mut changed = approved;
        changed.revision = 2;
        changed.interfaces.get_mut("notes-api").unwrap().spec =
            "GET /notes -> 200 {notes: [{id: str, title: str}]}".into();
        let affected = store(&task, changed, "ids become strings").unwrap();
        assert_eq!(affected, ["S01-B01", "S01-F01"]);
        let pending = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(pending.interfaces["notes-api"].version, 2);

        confirm(&task, 2, false).unwrap();
        confirm(&task, 2, true).unwrap();
        let state = task.state().unwrap();
        assert!(
            state
                .steps
                .iter()
                .all(|step| step.status == crate::state::StepStatus::Rejected),
            "the provider and its consumer start again on the new spec: {:?}",
            state.steps
        );
    }

    #[test]
    fn an_item_reads_the_specs_it_builds_or_uses() {
        let application = interfaced();
        let brief = application.interface_brief("S01-F01");
        assert!(
            brief.contains("You use interface notes-api v1 (Http)"),
            "{brief}"
        );
        assert!(brief.contains("GET /notes -> 200"), "{brief}");
        assert!(
            application
                .interface_brief("S01-B01")
                .contains("You provide interface notes-api")
        );
        assert!(baseline().interface_brief("S01-B01").is_empty());

        let read = items(&application, &["S01-F01".into()]).unwrap();
        assert_eq!(read["interfaces"]["notes-api"]["owner"], "S01-B01");
        let outline = outline(&application);
        assert_eq!(outline["interfaces"]["notes-api"]["version"], 1);
        assert!(outline["interfaces"]["notes-api"].get("spec").is_none());
    }

    #[test]
    fn a_one_way_decision_needs_alternatives() {
        let mut application = baseline();
        application.decisions = vec![Decision {
            id: "auth".into(),
            round: 1,
            question: "How do clients authenticate?".into(),
            recommendation: "Cookie session".into(),
            one_way: true,
            criteria: vec!["Works for the browser".into()],
            alternatives: vec![Alternative {
                option: "Cookie session".into(),
                tradeoffs: "Simple in a browser; needs CSRF care".into(),
                cost: "low".into(),
            }],
            ..Default::default()
        }];
        let error = application.validate().unwrap_err().to_string();
        assert!(
            error.contains("decision auth is one-way: give at least 2 alternatives"),
            "{error}"
        );
        application.decisions[0].alternatives.push(Alternative {
            option: "JWT bearer".into(),
            tradeoffs: "Works for CLI and mobile; hard to revoke".into(),
            cost: "medium".into(),
        });
        application.validate().unwrap();
    }

    #[test]
    fn an_old_baseline_without_interfaces_or_alternatives_loads() {
        let mut value = serde_json::to_value(baseline()).unwrap();
        assert!(value.get("interfaces").is_none(), "nothing new is written");
        value["decisions"] = serde_json::json!([{"id":"Q1","round":1,"question":"Goal?","recommendation":"Accounts","prerequisites":[],"answer":null}]);
        let application: Application = serde_json::from_value(value).unwrap();
        assert!(application.interfaces.is_empty());
        assert!(!application.decisions[0].one_way);
    }

    #[test]
    fn reviewed_order_errors_identify_the_unknown_item() {
        let mut application = baseline();
        application.confirmed_revision = Some(application.revision);
        application.approved_revision = Some(application.revision);
        let error = application
            .ready("opencode-response-test", &State::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("opencode-response-test"), "{error}");
        assert!(error.contains("accepted application baseline"), "{error}");
    }

    #[test]
    fn the_outline_keeps_ids_and_titles_without_contract_bodies() {
        let outline = outline(&baseline());
        assert_eq!(outline["items"]["S01-B01"]["title"], "Token service");
        assert_eq!(outline["items"]["S01-B01"]["lane"], "back");
        assert!(outline["items"]["S01-B01"].get("acceptance").is_none());
        assert_eq!(outline["milestones"]["M01"]["title"], "Accounts ready");
        assert_eq!(outline["revision"], 1);

        let read = items(&baseline(), &["S01-B01".into()]).unwrap();
        assert_eq!(
            read["items"]["S01-B01"]["acceptance"][0],
            "Service contract passes"
        );
        let error = items(&baseline(), &["S09".into()]).unwrap_err().to_string();
        assert!(error.contains("no item S09"), "{error}");
    }

    #[test]
    fn an_amendment_changes_only_what_it_names_on_the_next_revision() {
        let mut base = baseline();
        base.items
            .insert("S01-B02".into(), base.items["S01-B01"].clone());
        let mut added = base.items["S01-B01"].clone();
        added.title = "Token refresh".into();
        let mut renamed = base.items["S01-B01"].clone();
        renamed.title = "Token issue".into();
        let changes = Amendment {
            items: BTreeMap::from([
                ("S01-B03".into(), Some(added)),
                ("S01-B01".into(), Some(renamed)),
                ("S01-B02".into(), None),
            ]),
            ..Default::default()
        };
        let next = amended(&base, 3, changes);
        assert_eq!(next.revision, 4);
        assert_eq!(next.items["S01-B03"].title, "Token refresh");
        assert_eq!(next.items["S01-B01"].title, "Token issue");
        assert!(!next.items.contains_key("S01-B02"));
        assert_eq!(
            (&next.goal, &next.milestones),
            (&base.goal, &base.milestones)
        );
    }

    #[test]
    fn amendments_add_up_on_the_pending_proposal_and_are_checked() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        store(&task, baseline(), "first").unwrap();
        confirm(&task, 1, false).unwrap();
        confirm(&task, 1, true).unwrap();
        let goal = Amendment {
            goal: Some("Application with billing".into()),
            ..Default::default()
        };
        amend(&task, goal, "billing goal").unwrap();
        let non_goals = Amendment {
            non_goals: Some(vec!["No payments".into()]),
            ..Default::default()
        };
        amend(&task, non_goals, "no payments").unwrap();
        let proposal = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(proposal.revision, 2);
        assert_eq!(
            proposal.goal, "Application with billing",
            "the first amendment stays"
        );
        assert_eq!(proposal.non_goals, ["No payments"]);
        assert!(proposal.approved_revision.is_none());

        let broken = Amendment {
            items: BTreeMap::from([("S01-B01".into(), Some(Contract::default()))]),
            ..Default::default()
        };
        assert!(amend(&task, broken, "empty contract").is_err());
        assert_eq!(
            task.state().unwrap().application_proposal.unwrap(),
            proposal
        );
    }
    #[test]
    fn contracts_reject_incomplete_escaping_and_cyclic_work() {
        let mut application = baseline();
        application.validate().unwrap();
        application.items.get_mut("S01-B01").unwrap().allowed = vec!["../other/**".into()];
        assert!(application.validate().is_err());
        application = baseline();
        application.items.get_mut("S01-B01").unwrap().dependencies = vec!["S01-B01".into()];
        assert!(application.validate().is_err());
        application = baseline();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .interfaces
            .clear();
        assert!(application.validate().is_err());
    }
    /// The baseline with its acceptance tests written by an item of their
    /// own, which the implementation depends on.
    fn with_protected_tests() -> Application {
        let mut application = baseline();
        let implementation = application.items.get_mut("S01-B01").unwrap();
        implementation.protected_tests = vec!["backend/tests/acceptance/**".into()];
        implementation.dependencies = vec!["S01-T01".into()];
        let tests = Contract {
            title: "Acceptance tests for the token service".into(),
            allowed: vec!["backend/tests/acceptance/**".into()],
            writes_tests: true,
            dependencies: vec![],
            protected_tests: vec![],
            ..implementation.clone()
        };
        application.items.insert("S01-T01".into(), tests);
        application
            .milestones
            .get_mut("M01")
            .unwrap()
            .required
            .push("S01-T01".into());
        application
    }

    #[test]
    fn existing_acceptance_file_needs_no_writer_and_applies_to_every_order_in_its_repo() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let config = task.config().unwrap();
        let repo = &config.repos[0];
        let acceptance = "tests/existing-acceptance.py";
        crate::checks::tests::commit_file(
            &repo.worktree,
            acceptance,
            "# acceptance\n",
            "existing acceptance",
        );

        let mut proposal = with_protected_tests();
        let checks = proposal.check_configuration.remove("app").unwrap();
        proposal
            .check_configuration
            .insert(repo.name.clone(), checks);
        for item in proposal.items.values_mut() {
            item.repo = repo.name.clone();
        }
        proposal
            .existing_protected_tests
            .insert(repo.name.clone(), vec![acceptance.into()]);
        store(
            &task,
            proposal.clone(),
            "protect existing acceptance and author new tests",
        )
        .unwrap();
        confirm(&task, 1, false).unwrap();
        confirm(&task, 1, true).unwrap();
        let approved = task.state().unwrap().application;
        for item in approved.items.values() {
            assert!(
                approved
                    .order_forbidden(item, &[])
                    .contains(&acceptance.into())
            );
        }
        assert!(
            approved
                .order_forbidden(
                    &Contract {
                        repo: "other".into(),
                        ..Default::default()
                    },
                    &[]
                )
                .is_empty()
        );
        assert!(
            user_approval_needed(
                &approved,
                &Application {
                    existing_protected_tests: BTreeMap::new(),
                    ..approved.clone()
                }
            )
            .unwrap()
            .contains("takes existing protected tests away")
        );

        proposal.revision = 2;
        proposal
            .existing_protected_tests
            .insert(repo.name.clone(), vec!["tests/not-tracked.py".into()]);
        let error = store(&task, proposal, "untracked file")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("must be an existing tracked regular file"),
            "{error}"
        );
    }

    #[test]
    fn existing_files_cannot_be_globs_or_part_of_new_test_authorization() {
        let mut proposal = with_protected_tests();
        proposal.existing_protected_tests.insert(
            "app".into(),
            vec!["backend/tests/acceptance/test.py".into()],
        );
        let error = proposal.validate().unwrap_err().to_string();
        assert!(
            error.contains("test-writing scope overlaps an existing protected test"),
            "{error}"
        );
        assert!(
            error.contains("newly written protected tests overlap existing protected tests"),
            "{error}"
        );
        proposal
            .existing_protected_tests
            .insert("app".into(), vec!["tests/**".into()]);
        let error = proposal.validate().unwrap_err().to_string();
        assert!(error.contains("must name an exact file"), "{error}");
    }

    #[test]
    fn protected_tests_come_from_an_item_that_writes_them_first() {
        with_protected_tests().validate().unwrap();

        let mut application = with_protected_tests();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .dependencies
            .clear();
        let error = application.validate().unwrap_err().to_string();
        assert!(error.contains("depend on the item that writes"), "{error}");

        let mut application = with_protected_tests();
        application.items.get_mut("S01-T01").unwrap().allowed = vec!["backend/src/**".into()];
        let error = application.validate().unwrap_err().to_string();
        assert!(
            error.contains("may not write its protected tests"),
            "{error}"
        );

        let mut application = with_protected_tests();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .protected_tests = vec!["../escape/**".into()];
        assert!(application.validate().is_err());
    }

    #[test]
    fn an_implementation_order_may_not_touch_its_protected_tests() {
        let application = with_protected_tests();
        let implementation = &application.items["S01-B01"];
        assert_eq!(
            implementation.forbidden_for_order(&["secrets/**".into()]),
            ["secrets/**", "backend/tests/acceptance/**"]
        );
        assert_eq!(
            implementation.forbidden_for_order(&["backend/tests/acceptance/**".into()]),
            ["backend/tests/acceptance/**"],
            "listed once"
        );
        let tests = &application.items["S01-T01"];
        assert!(
            tests.forbidden_for_order(&[]).is_empty(),
            "the item that writes the tests may change them"
        );
    }

    #[test]
    fn interview_only_asks_settled_frontier_and_approval_is_revision_bound() {
        let mut application = baseline();
        application.decisions = vec![
            Decision {
                id: "Q1".into(),
                round: 1,
                question: "Goal?".into(),
                recommendation: "Accounts".into(),
                prerequisites: vec![],
                answer: None,
                one_way: true,
                ..Default::default()
            },
            Decision {
                id: "Q2".into(),
                round: 2,
                question: "Fields?".into(),
                recommendation: "Name".into(),
                prerequisites: vec!["Q1".into()],
                answer: None,
                one_way: true,
                ..Default::default()
            },
        ];
        assert_eq!(question_frontier(&application)[0].id, "Q1");
        application.decisions[0].answer = Some("yes".into());
        assert_eq!(question_frontier(&application)[0].id, "Q2");
        application.confirmed_revision = Some(1);
        application.approved_revision = Some(1);
        assert!(!application.approved());
        application.decisions[1].answer = Some("yes".into());
        assert!(application.approved());
        application.revision += 1;
        assert!(!application.approved());
    }
    #[test]
    fn only_one_way_decisions_wait_for_the_user_and_the_rest_take_their_recommendation() {
        let mut application = baseline();
        application.decisions = vec![
            Decision {
                id: "format".into(),
                round: 1,
                question: "Output format?".into(),
                recommendation: "One plain-text line".into(),
                ..Default::default()
            },
            Decision {
                id: "auth".into(),
                round: 1,
                question: "How do clients authenticate?".into(),
                recommendation: "Cookie session".into(),
                prerequisites: vec!["format".into()],
                one_way: true,
                ..Default::default()
            },
        ];
        let asked: Vec<&str> = question_frontier(&application)
            .iter()
            .map(|decision| decision.id.as_str())
            .collect();
        assert_eq!(
            asked,
            ["auth"],
            "a reversible prerequisite does not hold it back"
        );
        assert_eq!(
            assumed(&application),
            [
                serde_json::json!({"id":"format","question":"Output format?","recommendation":"One plain-text line"})
            ]
        );
        assert!(!application.resolved(), "the one-way decision is open");
        assert!(
            answerable(&application)
                .iter()
                .any(|decision| decision.id == "format"),
            "the user may still overrule an assumed one"
        );
        application.decisions[1].answer = Some("Cookie session".into());
        assert!(application.resolved());
        assert!(assumed(&application).len() == 1 && question_frontier(&application).is_empty());
    }

    #[test]
    fn old_application_defaults_to_unapproved() {
        let application: Application = serde_json::from_str("{}").unwrap();
        assert!(!application.approved());
    }
}
