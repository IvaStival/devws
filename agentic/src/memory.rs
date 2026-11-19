//! The team's memory, in ai-memory: what the agents learned and handed over,
//! for the agents that come after them. devAgentics talks to ai-memory itself
//! (its CLI), so every program has the same memory, whichever MCP servers its
//! account has. The task folder keeps the exact state; memory keeps notes.
//!
//! A task's notes live in the `devagentics-tasks` project under
//! `tasks/<task>/`; a note about one repo goes to that repo's project. What
//! comes back is history written by agents: it is handed on fenced and
//! labelled as untrusted, never as instructions.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::process::{Command, Stdio};

/// The project a task's own notes go to (docs/design.md).
pub const TASKS_PROJECT: &str = "devagentics-tasks";
const WORKSPACE: &str = "default";
/// The most of a note's body kept: a note is a summary, not a transcript.
pub const MAX_BODY: usize = 6000;
/// How many recent pages are looked through for a path's newest notes.
const RECENT_WINDOW: usize = 200;

/// What a note is, for whoever reads it next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Decision,
    Gotcha,
    Handoff,
    Finding,
    /// A worker's report on an order (devAgentics saves it).
    Report,
    /// The task's state when the leader moved (devAgentics writes it).
    Digest,
}

impl NoteKind {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "decision" => Self::Decision,
            "gotcha" => Self::Gotcha,
            "handoff" => Self::Handoff,
            "finding" => Self::Finding,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Gotcha => "gotcha",
            Self::Handoff => "handoff",
            Self::Finding => "finding",
            Self::Report => "report",
            Self::Digest => "digest",
        }
    }

    /// ai-memory's own kinds are fact, rule, decision and gotcha.
    fn ai_memory_kind(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Gotcha => "gotcha",
            _ => "fact",
        }
    }
}

/// A note to save: devAgentics adds who wrote it and where it belongs.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub kind: NoteKind,
    pub title: String,
    pub body: String,
    /// The agent that wrote it (`leader`, `hammer-12`).
    pub author: String,
    pub step: Option<String>,
}

/// A page read back.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Page {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    /// The whole body for a page read; the matching part for a search hit.
    #[serde(default, alias = "snippet")]
    pub body: String,
}

/// Where the notes are kept.
pub trait Memory: Send + Sync {
    /// Whether notes are kept at all (ai-memory is installed).
    fn is_on(&self) -> bool;
    fn save(&self, project: &str, path: &str, note: &Note) -> Result<()>;
    /// The newest pages whose path starts with `prefix`, newest first, read
    /// whole.
    fn recent(&self, project: &str, prefix: &str, limit: usize) -> Result<Vec<Page>>;
    /// The pages matching `query` (full text), best first, with the matching
    /// part as their body.
    fn search(&self, project: &str, query: &str, limit: usize) -> Result<Vec<Page>>;
}

/// The page's markdown: its title, who wrote it, then the body, cut to
/// `MAX_BODY`.
pub fn render(note: &Note, at: &str) -> String {
    let mut body = note.body.trim().to_string();
    if body.len() > MAX_BODY {
        let mut end = MAX_BODY;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        body.push_str("\n\n(cut)");
    }
    let step = note
        .step
        .as_ref()
        .map(|step| format!(" · step {step}"))
        .unwrap_or_default();
    format!(
        "# {}\n\n_{} by {}{step} · {at}_\n\n{body}\n",
        note.title.trim(),
        note.kind.name(),
        note.author
    )
}

/// A path part from free text: lowercase letters, digits and `-`.
pub fn slug(text: &str) -> String {
    let mut slug = String::new();
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() { "note".into() } else { slug }
}

/// Where a task's note of that kind goes: `tasks/<task>/<kind>/<time>-<title>.md`,
/// so the newest of a kind sort last by name and come first in `recent`.
pub fn task_path(task: &str, kind: NoteKind, stamp: &str, title: &str) -> String {
    format!("tasks/{task}/{}/{stamp}-{}.md", kind.name(), slug(title))
}

/// The CLI (`DEVAGENTICS_AI_MEMORY` names another).
pub struct AiMemory {
    command: String,
}

impl AiMemory {
    pub fn new(command: String) -> Self {
        Self { command }
    }

    fn run(&self, arguments: &[&str], stdin: Option<&str>) -> Result<String> {
        use std::io::Write;
        let mut child = Command::new(&self.command)
            .args(arguments)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", self.command))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "{} {} failed: {}",
                self.command,
                arguments.first().unwrap_or(&""),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn read(&self, project: &str, path: &str) -> Result<Page> {
        let text = self.run(
            &[
                "read-page",
                "--workspace",
                WORKSPACE,
                "--project",
                project,
                "--path",
                path,
                "--json",
            ],
            None,
        )?;
        serde_json::from_str(&text).with_context(|| format!("ai-memory's page {path}"))
    }
}

impl Memory for AiMemory {
    fn is_on(&self) -> bool {
        true
    }

    fn save(&self, project: &str, path: &str, note: &Note) -> Result<()> {
        let at = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
        let tag = format!("devagentics-{}", note.kind.name());
        self.run(
            &[
                "write-page",
                "--workspace",
                WORKSPACE,
                "--project",
                project,
                "--path",
                path,
                "--kind",
                note.kind.ai_memory_kind(),
                "--tier",
                "episodic",
                "-t",
                "devagentics",
                "-t",
                &tag,
                "--body",
                "-",
            ],
            Some(&render(note, &at)),
        )?;
        Ok(())
    }

    fn recent(&self, project: &str, prefix: &str, limit: usize) -> Result<Vec<Page>> {
        let window = RECENT_WINDOW.to_string();
        let text = self.run(
            &[
                "recent",
                "--workspace",
                WORKSPACE,
                "--project",
                project,
                "-n",
                &window,
                "--json",
            ],
            None,
        )?;
        let listed: Vec<Page> = serde_json::from_str(&text).context("ai-memory's recent pages")?;
        listed
            .into_iter()
            .filter(|page| page.path.starts_with(prefix))
            .take(limit)
            .map(|page| self.read(project, &page.path))
            .collect()
    }

    fn search(&self, project: &str, query: &str, limit: usize) -> Result<Vec<Page>> {
        let limit = limit.to_string();
        let text = self.run(
            &[
                "search",
                "--workspace",
                WORKSPACE,
                "--project",
                project,
                "-n",
                &limit,
                "--json",
                query,
            ],
            None,
        )?;
        let hits: Vec<Page> = serde_json::from_str(&text).context("ai-memory's search hits")?;
        // Its highlighting is for a browser, not for an agent's prompt.
        Ok(hits
            .into_iter()
            .map(|page| Page {
                body: page.body.replace("<mark>", "").replace("</mark>", ""),
                title: page
                    .title
                    .map(|title| title.replace("<mark>", "").replace("</mark>", "")),
                ..page
            })
            .collect())
    }
}

/// No ai-memory: nothing is kept, nothing comes back.
pub struct NoMemory;

impl Memory for NoMemory {
    fn is_on(&self) -> bool {
        false
    }

    fn save(&self, _project: &str, _path: &str, _note: &Note) -> Result<()> {
        Ok(())
    }

    fn recent(&self, _project: &str, _prefix: &str, _limit: usize) -> Result<Vec<Page>> {
        Ok(Vec::new())
    }

    fn search(&self, _project: &str, _query: &str, _limit: usize) -> Result<Vec<Page>> {
        Ok(Vec::new())
    }
}

/// ai-memory when it is installed, else nothing.
pub fn from_env() -> Box<dyn Memory> {
    let command = std::env::var("DEVAGENTICS_AI_MEMORY").unwrap_or_else(|_| "ai-memory".into());
    if is_installed(&command) {
        Box::new(AiMemory::new(command))
    } else {
        Box::new(NoMemory)
    }
}

fn is_installed(command: &str) -> bool {
    if command.contains('/') {
        return std::path::Path::new(command).is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|folder| folder.join(command).is_file())
    })
}

/// Notes kept in memory, for tests: saved pages, newest last.
#[cfg(test)]
#[derive(Default)]
pub struct Recording {
    pages: std::sync::Mutex<Vec<(String, String, Note)>>,
    is_on: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl Recording {
    /// Keeps notes from now on (it starts as if ai-memory were missing).
    pub fn turn_on(&self) {
        self.is_on.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Every note saved, with its project and path.
    pub fn saved(&self) -> Vec<(String, String, Note)> {
        self.pages.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl Memory for Recording {
    fn is_on(&self) -> bool {
        self.is_on.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn save(&self, project: &str, path: &str, note: &Note) -> Result<()> {
        let mut pages = self.pages.lock().unwrap();
        pages.retain(|(kept_project, kept_path, _)| {
            (kept_project, kept_path) != (&project.to_string(), &path.to_string())
        });
        pages.push((project.into(), path.into(), note.clone()));
        Ok(())
    }

    fn recent(&self, project: &str, prefix: &str, limit: usize) -> Result<Vec<Page>> {
        Ok(self
            .pages
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|(kept, path, _)| kept == project && path.starts_with(prefix))
            .take(limit)
            .map(|(_, path, note)| Page {
                path: path.clone(),
                title: Some(note.title.clone()),
                body: render(note, "then"),
            })
            .collect())
    }

    fn search(&self, project: &str, query: &str, limit: usize) -> Result<Vec<Page>> {
        let query = query.to_lowercase();
        Ok(self
            .pages
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|(kept, _, note)| {
                kept == project
                    && (note.title.to_lowercase().contains(&query)
                        || note.body.to_lowercase().contains(&query))
            })
            .take(limit)
            .map(|(_, path, note)| Page {
                path: path.clone(),
                title: Some(note.title.clone()),
                body: note.body.clone(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn note(kind: NoteKind, title: &str) -> Note {
        Note {
            kind,
            title: title.into(),
            body: "The registry needs the token first.".into(),
            author: "hammer-12".into(),
            step: Some("B3".into()),
        }
    }

    /// An ai-memory that records its arguments and stdin, and answers from
    /// files named after its command (`recent.json`, `read-page.json`…).
    fn stub(dir: &TempDir) -> AiMemory {
        let script = dir.path().join("ai-memory");
        let calls = dir.path().join("calls");
        crate::checks::tests::write_script(
            &script,
            &format!(
                "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> '{calls}'\n[ \"$1\" = write-page ] && cat >> '{calls}.stdin'\nanswer='{dir}/'\"$1\"'.json'\n[ ! -f \"$answer\" ] || cat \"$answer\"\n",
                calls = calls.display(),
                dir = dir.path().display()
            ),
        );
        AiMemory::new(script.display().to_string())
    }

    fn calls(dir: &TempDir) -> String {
        std::fs::read_to_string(dir.path().join("calls")).unwrap_or_default()
    }

    #[test]
    fn a_note_is_written_to_its_project_with_who_wrote_it() {
        let dir = TempDir::new().unwrap();
        let memory = stub(&dir);
        let path = task_path("feat-x", NoteKind::Gotcha, "20261118-2100", "Token first!");
        assert_eq!(path, "tasks/feat-x/gotcha/20261118-2100-token-first.md");
        memory
            .save(
                TASKS_PROJECT,
                &path,
                &note(NoteKind::Gotcha, "Token first!"),
            )
            .unwrap();
        let call = calls(&dir);
        assert!(
            call.starts_with(&format!(
                "write-page --workspace default --project devagentics-tasks --path {path} --kind gotcha"
            )),
            "{call}"
        );
        assert!(call.contains("-t devagentics-gotcha"), "{call}");
        let written = std::fs::read_to_string(dir.path().join("calls.stdin")).unwrap();
        assert!(
            written.starts_with("# Token first!\n\n_gotcha by hammer-12 · step B3 · "),
            "{written}"
        );
        assert!(written.contains("The registry needs the token first."));
    }

    #[test]
    fn the_newest_notes_under_a_path_are_read_whole() {
        let dir = TempDir::new().unwrap();
        let memory = stub(&dir);
        std::fs::write(
            dir.path().join("recent.json"),
            r#"[{"path":"tasks/feat-x/handoff/2.md","title":"Two","snippet":"…"},
                {"path":"sessions/s.md","title":"A session","snippet":"…"},
                {"path":"tasks/feat-x/handoff/1.md","title":"One","snippet":"…"}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("read-page.json"),
            r##"{"path":"tasks/feat-x/handoff/2.md","title":"Two","body":"# Two\n\nall of it"}"##,
        )
        .unwrap();
        let pages = memory
            .recent(TASKS_PROJECT, "tasks/feat-x/handoff/", 1)
            .unwrap();
        assert_eq!(pages.len(), 1, "only the newest one asked for");
        assert_eq!(pages[0].body, "# Two\n\nall of it");
        assert!(calls(&dir).contains("read-page --workspace default --project devagentics-tasks --path tasks/feat-x/handoff/2.md --json"));
    }

    #[test]
    fn a_search_hit_carries_its_matching_part() {
        let dir = TempDir::new().unwrap();
        let memory = stub(&dir);
        std::fs::write(
            dir.path().join("search.json"),
            r#"[{"path":"tasks/feat-x/gotcha/1.md","title":"Token","snippet":"needs the <mark>token</mark>","rank":-1.0}]"#,
        )
        .unwrap();
        let hits = memory.search(TASKS_PROJECT, "token", 5).unwrap();
        assert_eq!(hits[0].body, "needs the token", "without its highlighting");
        assert!(
            calls(&dir).contains(
                "search --workspace default --project devagentics-tasks -n 5 --json token"
            )
        );
    }

    #[test]
    fn a_failing_ai_memory_says_why() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("ai-memory");
        crate::checks::tests::write_script(
            &script,
            "#!/usr/bin/env bash\necho 'server down' >&2\nexit 3\n",
        );
        let error = AiMemory::new(script.display().to_string())
            .save(
                TASKS_PROJECT,
                "tasks/x/finding/1.md",
                &note(NoteKind::Finding, "x"),
            )
            .unwrap_err();
        assert!(error.to_string().contains("server down"), "{error}");
    }

    #[test]
    fn a_long_body_is_cut_and_a_title_becomes_a_path_part() {
        let mut long = note(NoteKind::Report, "B3 · o475");
        long.body = "é".repeat(MAX_BODY);
        let rendered = render(&long, "now");
        assert!(rendered.len() < MAX_BODY + 200);
        assert!(rendered.trim_end().ends_with("(cut)"));
        assert_eq!(slug("B3 · o475: done!"), "b3-o475-done");
        assert_eq!(slug("···"), "note");
    }

    #[test]
    fn without_ai_memory_nothing_is_kept() {
        let memory = NoMemory;
        assert!(!memory.is_on());
        memory
            .save(TASKS_PROJECT, "a.md", &note(NoteKind::Finding, "x"))
            .unwrap();
        assert!(memory.recent(TASKS_PROJECT, "", 5).unwrap().is_empty());
    }
}
