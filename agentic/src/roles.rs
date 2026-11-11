use crate::agent_args::{AUTO, Kind};
use crate::home::Home;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Only the leader is built in: every other role is the user's own file.
/// `roles/samples/` holds examples to copy.
const BUILTIN: [(&str, &str); 1] = [("leader", include_str!("../roles/leader.md"))];

/// The sample roles, which the tests install into a home as the user's own.
#[cfg(test)]
pub const SAMPLES: [(&str, &str); 2] = [
    ("planner", include_str!("../roles/samples/planner.md")),
    ("executor", include_str!("../roles/samples/executor.md")),
];

/// Where a role came from. A repo's own beats the user's, which beats the
/// ones shipped in the binary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Builtin,
    Home,
    Repo,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Builtin => "built in",
            Source::Home => "~/.devagentics/roles",
            Source::Repo => ".devws/roles",
        }
    }
}

/// Where a role sits in the team. The CTO (the leader) runs everything;
/// seniors plan the work and answer questions about it, and never write the
/// repo; the factory floor writes it, and only runs steps from a senior's
/// accepted plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Cto,
    Senior,
    Floor,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Cto => "CTO",
            Group::Senior => "Seniors",
            Group::Floor => "Factory floor",
        }
    }

    /// The group workspace its agents open in; the CTO is the team's own
    /// workspace.
    pub fn workspace(self) -> &'static str {
        match self {
            Group::Cto => "CTO",
            Group::Senior => "Seniors",
            Group::Floor => "Floor",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Group::Cto => "cto",
            Group::Senior => "senior",
            Group::Floor => "floor",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    pub name: String,
    /// A `DEVWS_AGENTS` name: `claude`, `codex`, `claude_work`…
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Whether the role changes the repo it works in.
    pub writes: bool,
    pub group: Group,
    pub sandbox: Option<String>,
    pub approvals: Option<String>,
    /// One line for the board: what the role is for.
    pub description: Option<String>,
    pub instructions: String,
    pub source: Source,
    /// Where the leader moves when its account runs out (leader.rs).
    pub fallback: Option<Fallback>,
}

/// Another program for the same role: `[fallback]` in a role's settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fallback {
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Its own: the role's suit the role's program, not this one.
    pub approvals: Option<String>,
}

impl Role {
    /// The same role on its fallback's agent, model and effort.
    pub fn on_fallback(&self) -> Option<Role> {
        let fallback = self.fallback.as_ref()?;
        Some(Role {
            agent: fallback.agent.clone(),
            model: fallback.model.clone(),
            effort: fallback.effort.clone(),
            approvals: fallback.approvals.clone(),
            sandbox: None,
            fallback: None,
            ..self.clone()
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    agent: String,
    model: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    writes: bool,
    group: Option<Group>,
    sandbox: Option<String>,
    approvals: Option<String>,
    description: Option<String>,
    fallback: Option<Fallback>,
}

const FENCE: &str = "+++";

/// A role file: a TOML block between `+++` lines, then the instructions.
fn parse(name: &str, text: &str, source: Source, origin: &str) -> Result<Role> {
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| rest.strip_prefix('\n'))
        .with_context(|| format!("{origin}: a role starts with a {FENCE} line and its settings"))?;
    let end = rest
        .find(&format!("\n{FENCE}\n"))
        .with_context(|| format!("{origin}: the settings do not end with a {FENCE} line"))?;
    let front: FrontMatter = toml::from_str(&rest[..end])
        .with_context(|| format!("{origin}: the settings are wrong"))?;
    let instructions = rest[end + FENCE.len() + 2..].trim().to_string();
    if instructions.is_empty() {
        bail!("{origin}: the role has no instructions");
    }
    // Old role files have no group: the leader is the CTO, a role that writes
    // is on the floor, any other is a senior.
    let group = front.group.unwrap_or(if name == "leader" {
        Group::Cto
    } else if front.writes {
        Group::Floor
    } else {
        Group::Senior
    });
    match group {
        Group::Senior if front.writes => bail!(
            "{origin}: a senior plans and answers but never writes the repo (writes = false, or group = \"floor\")"
        ),
        Group::Floor if !front.writes => bail!(
            "{origin}: the factory floor writes the repo (writes = true, or group = \"senior\")"
        ),
        _ => {}
    }
    // The other roles are refused when their account is near its limit; only
    // the leader, who keeps the team going, moves.
    if front.fallback.is_some() && name != "leader" {
        bail!("{origin}: only the leader has a fallback");
    }
    Ok(Role {
        name: name.to_string(),
        agent: front.agent,
        model: front.model,
        effort: front.effort,
        writes: front.writes,
        group,
        sandbox: front.sandbox,
        approvals: front.approvals,
        description: front.description,
        instructions,
        source,
        fallback: front.fallback,
    })
}

/// The roles a task can use, by name.
pub struct Roles {
    roles: Vec<Role>,
}

impl Roles {
    /// The built-in roles, overridden by name by the user's
    /// (`~/.devagentics/roles/*.md`), then by the repo's (`.devws/roles/`).
    pub fn load(home: &Home, repo: Option<&Path>) -> Result<Self> {
        let mut roles = Vec::new();
        for (name, text) in BUILTIN {
            roles.push(parse(
                name,
                text,
                Source::Builtin,
                &format!("the built-in {name} role"),
            )?);
        }
        Self::overlay(&mut roles, &home.roles_dir(), Source::Home)?;
        if let Some(repo) = repo {
            Self::overlay(&mut roles, &repo.join(".devws").join("roles"), Source::Repo)?;
        }
        roles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { roles })
    }

    fn overlay(roles: &mut Vec<Role>, dir: &Path, source: Source) -> Result<()> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
            {
                bail!(
                    "{}: a role's file name uses lowercase letters, digits, - and _",
                    path.display()
                );
            }
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let role = parse(&name, &text, source, &path.display().to_string())?;
            roles.retain(|existing| existing.name != name);
            roles.push(role);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|role| role.name == name)
    }

    pub fn all(&self) -> &[Role] {
        &self.roles
    }
}

/// A setting of a role that `devagentics role set` (the devws menu's Team
/// roles) changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    Model,
    Effort,
    Approvals,
}

impl Setting {
    pub fn key(self) -> &'static str {
        match self {
            Setting::Model => "model",
            Setting::Effort => "effort",
            Setting::Approvals => "approvals",
        }
    }

    /// The values the role's program takes; for a model, the known ones, as
    /// any other may be typed (new models come out faster than this list).
    pub fn choices(self, kind: Kind) -> &'static [&'static str] {
        match (self, kind) {
            (Setting::Model, Kind::Claude) => &[
                "claude-opus-5-5",
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
                "claude-fable-5-1",
            ],
            (Setting::Model, Kind::Codex) => &["gpt-6.1-sol", "gpt-6-luna", "gpt-5.6-luna"],
            (Setting::Effort, Kind::Claude) => &["low", "medium", "high", "xhigh", "max"],
            (Setting::Effort, Kind::Codex) => &["minimal", "low", "medium", "high", "xhigh"],
            // Claude's permission modes; Codex's -a, and devws's auto review.
            (Setting::Approvals, Kind::Claude) => {
                &["default", "acceptEdits", "plan", AUTO, "bypassPermissions"]
            }
            (Setting::Approvals, Kind::Codex) => &["untrusted", "on-request", "never", AUTO],
        }
    }
}

/// Changes settings of the role `name` in the file it comes from (the user's
/// roles, or the repo's): each named key's line is rewritten, added at the end
/// of the settings, or, for an empty value, removed, so the program's default
/// applies. Everything else in the file stays as it was. `kind` is the role's
/// program. Returns the file it changed.
pub fn set(
    home: &Home,
    repo: Option<&Path>,
    name: &str,
    kind: Kind,
    changes: &[(Setting, String)],
) -> Result<PathBuf> {
    if name == "leader" {
        bail!("the leader's program changes with Switch leader model, not here");
    }
    let roles = Roles::load(home, repo)?;
    let role = roles
        .get(name)
        .with_context(|| format!("no role is named {name}"))?;
    let path = match (role.source, repo) {
        (Source::Repo, Some(repo)) => repo.join(".devws").join("roles"),
        _ => home.roles_dir(),
    }
    .join(format!("{name}.md"));
    let mut text =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    for (setting, value) in changes {
        if *setting != Setting::Model && !value.is_empty() {
            let choices = setting.choices(kind);
            if !choices.contains(&value.as_str()) {
                bail!(
                    "{name}'s program takes {} {}, not {value}",
                    setting.key(),
                    choices.join(", ")
                );
            }
        }
        text = with_setting(&text, setting.key(), value);
    }
    parse(name, &text, role.source, &path.display().to_string())?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// `text`, a role file, with `key = "value"` in its settings: in place of the
/// key's line, else after the last top-level setting; none for an empty value.
fn with_setting(text: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let end = lines
        .iter()
        .skip(1)
        .position(|line| line == FENCE)
        .map_or(lines.len(), |index| index + 1);
    // A [table] (the leader's [fallback]) ends the top-level settings.
    let top_end = lines[1..end]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(end, |index| index + 1);
    let is_key = |line: &String| {
        line.split_once('=')
            .is_some_and(|(name, _)| name.trim() == key)
    };
    let replacement = format!("{key} = {}", toml::Value::String(value.to_string()));
    match lines[1..top_end]
        .iter()
        .position(is_key)
        .map(|index| index + 1)
    {
        Some(index) if value.is_empty() => {
            lines.remove(index);
        }
        Some(index) => lines[index] = replacement,
        None if value.is_empty() => {}
        None => lines.insert(top_end, replacement),
    }
    let mut changed = lines.join("\n");
    if text.ends_with('\n') {
        changed.push('\n');
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn home_with(files: &[(&str, &str)]) -> (TempDir, Home) {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().to_path_buf());
        fs::create_dir_all(home.roles_dir()).unwrap();
        for (name, text) in files {
            fs::write(home.roles_dir().join(name), text).unwrap();
        }
        (dir, home)
    }

    #[test]
    fn only_the_leader_is_built_in_and_the_samples_parse() {
        let (_dir, home) = home_with(&[]);
        let roles = Roles::load(&home, None).unwrap();
        let names: Vec<_> = roles.all().iter().map(|role| role.name.as_str()).collect();
        assert_eq!(names, ["executor", "leader", "planner"]);
        assert_eq!(roles.get("leader").unwrap().source, Source::Builtin);
        assert!(!roles.get("leader").unwrap().writes);
        assert!(roles.get("executor").unwrap().writes);
        assert!(!roles.get("planner").unwrap().writes);
        assert_eq!(roles.get("planner").unwrap().source, Source::Home);
    }

    #[test]
    fn a_role_without_a_group_gets_one_from_what_it_does() {
        let (_dir, home) = home_with(&[
            (
                "robson.md",
                "+++\nagent = \"claude\"\ngroup = \"senior\"\n+++\nTest plans.\n",
            ),
            (
                "hammer.md",
                "+++\nagent = \"codex\"\nwrites = true\n+++\nBuild it.\n",
            ),
        ]);
        let roles = Roles::load(&home, None).unwrap();
        let group = |name: &str| roles.get(name).unwrap().group;
        assert_eq!(group("leader"), Group::Cto);
        assert_eq!(group("planner"), Group::Senior);
        assert_eq!(group("executor"), Group::Floor);
        assert_eq!(group("robson"), Group::Senior);
        assert_eq!(group("hammer"), Group::Floor);
    }

    #[test]
    fn the_leader_falls_back_to_codex_sol_and_only_the_leader_has_a_fallback() {
        let (_dir, home) = home_with(&[]);
        let roles = Roles::load(&home, None).unwrap();
        let leader = roles.get("leader").unwrap();
        let fallback = leader
            .fallback
            .clone()
            .expect("the built-in leader has one");
        assert_eq!(
            (
                fallback.agent.as_str(),
                fallback.model.as_deref(),
                fallback.effort.as_deref()
            ),
            ("codex", Some("gpt-6.1-sol"), Some("medium"))
        );
        let moved = leader.on_fallback().unwrap();
        assert_eq!(
            (moved.agent.as_str(), moved.model.as_deref()),
            ("codex", Some("gpt-6.1-sol"))
        );
        assert_eq!(
            moved.approvals.as_deref(),
            Some("auto"),
            "the codex leader asks no one"
        );
        assert_eq!(
            moved.instructions, leader.instructions,
            "the same leader, another program"
        );
        assert!(moved.fallback.is_none(), "no fallback of the fallback");

        let (_dir, home) = home_with(&[(
            "leader.md",
            "+++\nagent = \"claude\"\n+++\nLead without a fallback.\n",
        )]);
        assert!(
            Roles::load(&home, None)
                .unwrap()
                .get("leader")
                .unwrap()
                .fallback
                .is_none()
        );

        let (_dir, home) = home_with(&[(
            "hammer.md",
            "+++\nagent = \"claude\"\nwrites = true\n[fallback]\nagent = \"codex\"\n+++\nBuild.\n",
        )]);
        let message = format!("{:#}", Roles::load(&home, None).err().unwrap());
        assert!(message.contains("only the leader"), "{message}");
    }

    #[test]
    fn a_senior_never_writes_and_the_floor_always_does() {
        for (text, wanted) in [
            (
                "+++\nagent = \"claude\"\ngroup = \"senior\"\nwrites = true\n+++\nBody\n",
                "never writes",
            ),
            (
                "+++\nagent = \"claude\"\ngroup = \"floor\"\n+++\nBody\n",
                "writes the repo",
            ),
            (
                "+++\nagent = \"claude\"\ngroup = \"boss\"\n+++\nBody\n",
                "settings are wrong",
            ),
        ] {
            let (_dir, home) = home_with(&[("odd.md", text)]);
            let message = format!("{:#}", Roles::load(&home, None).err().unwrap());
            assert!(message.contains(wanted), "{message} should say {wanted}");
        }
    }

    #[test]
    fn the_leader_knows_its_tools_and_the_executor_its_report() {
        let (_dir, home) = home_with(&[]);
        let roles = Roles::load(&home, None).unwrap();
        let leader = &roles.get("leader").unwrap().instructions;
        for tool in [
            "task_status",
            "spawn_agent",
            "send_order",
            "add_lane",
            "read_plan",
            "answer_question",
            "approve_plan",
            "accept_step",
            "reject_step",
            "add_repo",
            "open_prs",
            "escalate",
        ] {
            assert!(
                leader.contains(tool),
                "the leader's instructions do not name {tool}"
            );
        }
        assert!(
            roles
                .get("executor")
                .unwrap()
                .instructions
                .contains("devagentics report done")
        );
        assert!(
            roles
                .get("planner")
                .unwrap()
                .instructions
                .contains("devagentics plan write")
        );
    }

    #[test]
    fn the_users_roles_add_and_override_and_a_repos_win() {
        let (_dir, home) = home_with(&[
            (
                "tester.md",
                "+++\nagent = \"codex\"\nwrites = true\n+++\nWrite the tests.\n",
            ),
            (
                "executor.md",
                "+++\nagent = \"codex\"\nmodel = \"gpt\"\nwrites = true\n+++\nUser executor.\n",
            ),
        ]);
        let repo = TempDir::new().unwrap();
        fs::create_dir_all(repo.path().join(".devws/roles")).unwrap();
        fs::write(
            repo.path().join(".devws/roles/executor.md"),
            "+++\nagent = \"claude_work\"\nwrites = true\n+++\nRepo executor.\n",
        )
        .unwrap();

        let roles = Roles::load(&home, Some(repo.path())).unwrap();
        let tester = roles.get("tester").unwrap();
        assert_eq!(
            (tester.agent.as_str(), tester.source),
            ("codex", Source::Home)
        );
        let executor = roles.get("executor").unwrap();
        assert_eq!(
            (
                executor.agent.as_str(),
                executor.instructions.as_str(),
                executor.source
            ),
            ("claude_work", "Repo executor.", Source::Repo)
        );
        assert_eq!(roles.get("leader").unwrap().source, Source::Builtin);

        let without_repo = Roles::load(&home, None).unwrap();
        assert_eq!(
            without_repo.get("executor").unwrap().model.as_deref(),
            Some("gpt")
        );
    }

    const HAMMER: &str = "+++\n# The floor's first tier.\nagent = \"claude\"\nmodel = \"claude-haiku-5-5\"\nwrites = true\n+++\nBuild one step.\n";

    fn change(setting: Setting, value: &str) -> (Setting, String) {
        (setting, value.to_string())
    }

    #[test]
    fn set_rewrites_adds_and_removes_only_the_named_settings() {
        let (_dir, home) = home_with(&[("hammer.md", HAMMER)]);
        let path = set(
            &home,
            None,
            "hammer",
            Kind::Claude,
            &[
                change(Setting::Model, "claude-sonnet-5-5"),
                change(Setting::Effort, "low"),
            ],
        )
        .unwrap();
        assert_eq!(path, home.roles_dir().join("hammer.md"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "+++\n# The floor's first tier.\nagent = \"claude\"\nmodel = \"claude-sonnet-5-5\"\nwrites = true\neffort = \"low\"\n+++\nBuild one step.\n"
        );
        let roles = Roles::load(&home, None).unwrap();
        let hammer = roles.get("hammer").unwrap();
        assert_eq!(
            (hammer.model.as_deref(), hammer.effort.as_deref()),
            (Some("claude-sonnet-5-5"), Some("low"))
        );

        set(
            &home,
            None,
            "hammer",
            Kind::Claude,
            &[change(Setting::Effort, "")],
        )
        .unwrap();
        set(
            &home,
            None,
            "hammer",
            Kind::Claude,
            &[change(Setting::Model, "")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "+++\n# The floor's first tier.\nagent = \"claude\"\nwrites = true\n+++\nBuild one step.\n",
            "an empty value leaves the program's default"
        );
    }

    #[test]
    fn set_takes_only_values_the_roles_program_knows() {
        let (_dir, home) = home_with(&[("hammer.md", HAMMER)]);
        for (kind, setting, value) in [
            (Kind::Claude, Setting::Approvals, "never"),
            (Kind::Codex, Setting::Approvals, "acceptEdits"),
            (Kind::Claude, Setting::Effort, "minimal"),
        ] {
            let message = format!(
                "{:#}",
                set(&home, None, "hammer", kind, &[change(setting, value)])
                    .err()
                    .unwrap()
            );
            assert!(message.contains(value), "{message}");
            assert!(
                message.contains(setting.choices(kind)[0]),
                "{message} should name the allowed values"
            );
        }
        set(
            &home,
            None,
            "hammer",
            Kind::Codex,
            &[change(Setting::Approvals, "never")],
        )
        .unwrap();
        set(
            &home,
            None,
            "hammer",
            Kind::Claude,
            &[change(Setting::Model, "a-model-from-next-week")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(home.roles_dir().join("hammer.md")).unwrap(),
            "+++\n# The floor's first tier.\nagent = \"claude\"\nmodel = \"a-model-from-next-week\"\nwrites = true\napprovals = \"never\"\n+++\nBuild one step.\n"
        );
    }

    #[test]
    fn set_refuses_the_leader_and_unknown_roles_and_writes_a_repos_own() {
        let (_dir, home) = home_with(&[("hammer.md", HAMMER)]);
        let refusal = |name: &str| {
            format!(
                "{:#}",
                set(
                    &home,
                    None,
                    name,
                    Kind::Claude,
                    &[change(Setting::Model, "x")]
                )
                .err()
                .unwrap()
            )
        };
        assert!(refusal("leader").contains("Switch leader model"));
        assert!(refusal("nobody").contains("no role is named nobody"));

        let repo = TempDir::new().unwrap();
        fs::create_dir_all(repo.path().join(".devws/roles")).unwrap();
        fs::write(repo.path().join(".devws/roles/hammer.md"), HAMMER).unwrap();
        let path = set(
            &home,
            Some(repo.path()),
            "hammer",
            Kind::Claude,
            &[change(Setting::Effort, "high")],
        )
        .unwrap();
        assert_eq!(path, repo.path().join(".devws/roles/hammer.md"));
        assert_eq!(
            fs::read_to_string(home.roles_dir().join("hammer.md")).unwrap(),
            HAMMER,
            "the user's own role is not the one the repo uses"
        );
    }

    #[test]
    fn a_broken_role_names_its_file() {
        for (text, wanted) in [
            ("no settings\n", "starts with a +++ line"),
            ("+++\nagent = \"claude\"\nBody\n", "do not end"),
            ("+++\nmodel = \"x\"\n+++\nBody\n", "settings are wrong"),
            (
                "+++\nagent = \"claude\"\ncolour = \"red\"\n+++\nBody\n",
                "settings are wrong",
            ),
            ("+++\nagent = \"claude\"\n+++\n\n", "no instructions"),
        ] {
            let (_dir, home) = home_with(&[("broken.md", text)]);
            let error = Roles::load(&home, None)
                .err()
                .expect("a broken role must not load");
            let message = format!("{error:#}");
            assert!(message.contains("broken.md"), "{message}");
            assert!(message.contains(wanted), "{message} should say {wanted}");
        }
        let (_dir, home) = home_with(&[("Bad Name.md", "+++\nagent = \"claude\"\n+++\nBody\n")]);
        assert!(Roles::load(&home, None).is_err());
    }
}
