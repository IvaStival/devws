use crate::home::Home;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
