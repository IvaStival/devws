//! Design references: before a designer plans an interface, it shows the
//! user a few directions (images or static HTML mockups), and the user
//! approves or rejects each one. The user can give images of their own,
//! which every designer can read. Only the user answers: the leader has no
//! tool for it.

use crate::log::Kind as LogKind;
use crate::state::{LEADER, State};
use crate::task::Task;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The most a reference file may weigh.
const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// The most references a round may show.
const MAX_PER_ROUND: usize = 8;
/// Where a read-only worker's scratch folder is mounted in its container.
const SCRATCH_MOUNT: &str = "/scratch";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Png,
    Jpeg,
    Webp,
    Svg,
    Html,
}

impl FileKind {
    /// What a file is, from its first bytes: its name can say anything.
    pub fn of(bytes: &[u8]) -> Option<FileKind> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(FileKind::Png);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(FileKind::Jpeg);
        }
        if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            return Some(FileKind::Webp);
        }
        let text = std::str::from_utf8(&bytes[..bytes.len().min(4096)]).ok()?;
        let start = text.trim_start().to_ascii_lowercase();
        if start.starts_with("<svg") || (start.starts_with("<?xml") && start.contains("<svg")) {
            return Some(FileKind::Svg);
        }
        if start.starts_with("<!doctype html") || start.starts_with("<html") {
            return Some(FileKind::Html);
        }
        None
    }

    pub fn extension(self) -> &'static str {
        match self {
            FileKind::Png => "png",
            FileKind::Jpeg => "jpg",
            FileKind::Webp => "webp",
            FileKind::Svg => "svg",
            FileKind::Html => "html",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            FileKind::Png => "image/png",
            FileKind::Jpeg => "image/jpeg",
            FileKind::Webp => "image/webp",
            FileKind::Svg => "image/svg+xml",
            FileKind::Html => "text/html; charset=utf-8",
        }
    }

    pub fn is_image(self) -> bool {
        self != FileKind::Html
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetStatus {
    /// The designer is adding references.
    Drafting,
    /// Shown to the user, who has not answered.
    Waiting,
    /// The user approved at least one: the designer may plan.
    Approved,
    /// The user rejected them all: the designer shows a new round.
    Rejected,
}

/// One direction shown to the user, or one image the user gave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub id: String,
    /// Its file name in its folder (`r1.html`).
    pub file: String,
    pub kind: FileKind,
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// The references a designer shows for one step, round by round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceSet {
    pub step: String,
    pub agent: String,
    pub round: u32,
    pub status: SetStatus,
    pub items: Vec<Reference>,
    /// The user's comment on the whole round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl ReferenceSet {
    pub fn folder(&self, task: &Task) -> PathBuf {
        round_folder(task, &self.step, self.round)
    }
}

fn round_folder(task: &Task, step: &str, round: u32) -> PathBuf {
    task.dir()
        .join("references")
        .join(step)
        .join(round.to_string())
}

pub fn user_folder(task: &Task) -> PathBuf {
    task.dir().join("references").join("user")
}

/// The latest round of a step.
pub fn latest<'a>(state: &'a State, step: &str) -> Option<&'a ReferenceSet> {
    state
        .references
        .iter()
        .filter(|set| set.step == step)
        .max_by_key(|set| set.round)
}

/// The round the user approved for a step, if any.
pub fn approved_for<'a>(state: &'a State, step: &str) -> Option<&'a ReferenceSet> {
    latest(state, step).filter(|set| set.status == SetStatus::Approved)
}

/// The sets waiting for the user's answer.
pub fn waiting(state: &State) -> Vec<&ReferenceSet> {
    state
        .references
        .iter()
        .filter(|set| set.status == SetStatus::Waiting)
        .collect()
}

/// The agent showed references for its order and waits for the user: its
/// idle turn is not a stall.
pub fn waits_for_user(state: &State, agent: &str) -> bool {
    state.open_order_of(agent).is_some_and(|order| {
        latest(state, &order.step)
            .is_some_and(|set| set.agent == agent && set.status == SetStatus::Waiting)
    })
}

/// Why a planner's plan for `step` must wait, if its role shows references.
pub fn plan_refusal(state: &State, agent: &str, step: &str) -> Option<String> {
    let shows = state
        .agent(agent)
        .is_some_and(|agent| agent.shows_references);
    if !shows || approved_for(state, step).is_some() {
        return None;
    }
    let now = match latest(state, step).map(|set| set.status) {
        Some(SetStatus::Waiting) => "the user has not answered yet: end your turn and wait",
        Some(SetStatus::Rejected) => {
            "the user rejected the last round: read their comments (references_status) and show a new one"
        }
        _ => "show the user 2–3 directions first (references_add, then references_submit)",
    };
    Some(format!(
        "your role plans only from references the user approved; {now}"
    ))
}

/// Reads a reference file, refusing what is not an image or HTML page.
fn read_reference(path: &Path) -> Result<(Vec<u8>, FileKind)> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .len();
    ensure!(
        size <= MAX_BYTES,
        "{} is {} MB; a reference may be at most {} MB",
        path.display(),
        size / (1024 * 1024),
        MAX_BYTES / (1024 * 1024)
    );
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let kind = FileKind::of(&bytes).with_context(|| {
        format!(
            "{} is not a PNG, JPEG, WebP or SVG image or an HTML page",
            path.display()
        )
    })?;
    Ok((bytes, kind))
}

fn write_file(folder: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    let file = folder.join(name);
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, &file)?;
    Ok(())
}

/// A worker's `/scratch/…` path on this machine; refuses anything outside
/// its scratch folder.
pub fn scratch_file(task: &Task, agent: &str, path: &str) -> Result<PathBuf> {
    let scratch = scratch_folder(task, agent);
    let relative = path
        .strip_prefix(SCRATCH_MOUNT)
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(path);
    let file = scratch
        .join(relative)
        .canonicalize()
        .with_context(|| format!("{path} is not a file in your /scratch"))?;
    let root = scratch.canonicalize().context("you have no /scratch")?;
    ensure!(
        file.starts_with(&root) && file.is_file(),
        "{path} is not a file in your /scratch"
    );
    Ok(file)
}

fn scratch_folder(task: &Task, agent: &str) -> PathBuf {
    task.dir().join("workers").join(agent).join("scratch")
}

/// Adds a reference to the round a designer is drafting for its order's
/// step. The file is copied, so what the user sees cannot change after.
pub fn add(task: &Task, agent: &str, file: &Path, note: &str) -> Result<String> {
    ensure!(
        !note.trim().is_empty(),
        "say in the note what this direction takes from the user's images and what it changes"
    );
    let (bytes, kind) = read_reference(file)?;
    task.transaction(|tx| {
        let shows = tx
            .state
            .agent(agent)
            .is_some_and(|agent| agent.shows_references);
        ensure!(shows, "your role does not show references");
        let step = tx
            .state
            .open_order_of(agent)
            .map(|order| order.step.clone())
            .context("references are for the step of your open order")?;
        let round = match latest(&tx.state, &step).map(|set| (set.status, set.round)) {
            Some((SetStatus::Drafting, round)) => round,
            Some((SetStatus::Waiting, _)) => bail!("the user has not answered your references yet"),
            Some((SetStatus::Approved, _)) => {
                bail!("the user approved references for {step}: write your plan")
            }
            Some((SetStatus::Rejected, round)) => {
                tx.state.references.push(ReferenceSet {
                    step: step.clone(),
                    agent: agent.to_string(),
                    round: round + 1,
                    status: SetStatus::Drafting,
                    items: Vec::new(),
                    comment: None,
                });
                round + 1
            }
            None => {
                tx.state.references.push(ReferenceSet {
                    step: step.clone(),
                    agent: agent.to_string(),
                    round: 1,
                    status: SetStatus::Drafting,
                    items: Vec::new(),
                    comment: None,
                });
                1
            }
        };
        let set = tx
            .state
            .references
            .iter_mut()
            .find(|set| set.step == step && set.round == round)
            .expect("the round exists");
        ensure!(
            set.items.len() < MAX_PER_ROUND,
            "a round shows at most {MAX_PER_ROUND} references"
        );
        let id = format!("r{}", set.items.len() + 1);
        let name = format!("{id}.{}", kind.extension());
        write_file(&round_folder(task, &step, round), &name, &bytes)?;
        set.items.push(Reference {
            id: id.clone(),
            file: name,
            kind,
            note: note.trim().to_string(),
            verdict: None,
            comment: None,
        });
        Ok(id)
    })
}

/// Shows the drafted round to the user, and tells the leader.
pub fn submit(task: &Task, agent: &str) -> Result<(String, usize)> {
    let task_id = task.config()?.id;
    task.transaction(|tx| {
        let step = tx
            .state
            .open_order_of(agent)
            .map(|order| order.step.clone())
            .context("references are for the step of your open order")?;
        let set = tx
            .state
            .references
            .iter_mut()
            .filter(|set| set.step == step)
            .max_by_key(|set| set.round)
            .filter(|set| set.status == SetStatus::Drafting)
            .context("add your references first (references_add)")?;
        ensure!(!set.items.is_empty(), "add your references first (references_add)");
        set.status = SetStatus::Waiting;
        let (round, count) = (set.round, set.items.len());
        tx.state.say(
            LEADER,
            format!(
                "{agent} shows the user {count} design references for step {step} (round {round}). The user answers on the references page (the board's REFERENCES line, or `devagentics references url {}`); you cannot answer for them. Tell the user they are waiting.",
                task_id
            ),
        );
        tx.log(
            LogKind::Plan,
            Some(agent),
            Some(&step),
            &format!("{count} design references wait for the user (round {round})"),
        );
        Ok((step, count))
    })
}

/// The user's answer to one reference.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub verdict: Verdict,
    pub comment: Option<String>,
}

/// The user's answer to a waiting round: every reference approved or
/// rejected. One approved lets the designer plan; none, and it shows a new
/// round. The designer is told what the user said.
pub fn answer(
    task: &Task,
    step: &str,
    answers: &BTreeMap<String, Answer>,
    comment: Option<&str>,
) -> Result<SetStatus> {
    task.transaction(|tx| {
        let set = tx
            .state
            .references
            .iter_mut()
            .filter(|set| set.step == step)
            .max_by_key(|set| set.round)
            .filter(|set| set.status == SetStatus::Waiting)
            .with_context(|| format!("no references wait for an answer on {step}"))?;
        for id in answers.keys() {
            ensure!(
                set.items.iter().any(|item| &item.id == id),
                "{step} has no reference {id}"
            );
        }
        let missing: Vec<&str> = set
            .items
            .iter()
            .filter(|item| !answers.contains_key(&item.id))
            .map(|item| item.id.as_str())
            .collect();
        ensure!(
            missing.is_empty(),
            "approve or reject every reference: {} not answered",
            missing.join(", ")
        );
        for item in &mut set.items {
            let answer = &answers[&item.id];
            item.verdict = Some(answer.verdict);
            item.comment = answer
                .comment
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
        }
        set.comment = comment
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        let approved = set
            .items
            .iter()
            .any(|item| item.verdict == Some(Verdict::Approved));
        set.status = if approved {
            SetStatus::Approved
        } else {
            SetStatus::Rejected
        };
        let (status, agent, round) = (set.status, set.agent.clone(), set.round);
        let told = told_designer(set);
        tx.state.say(&agent, told);
        tx.state.say(
            LEADER,
            format!(
                "The user {} {agent}'s design references for step {step} (round {round}).",
                if approved {
                    "approved"
                } else {
                    "rejected all of"
                }
            ),
        );
        tx.log(
            LogKind::Plan,
            Some(&agent),
            Some(step),
            &format!(
                "the user answered the design references (round {round}): {}",
                if approved { "approved" } else { "rejected" }
            ),
        );
        Ok(status)
    })
}

fn verdict_word(verdict: Option<Verdict>) -> &'static str {
    match verdict {
        Some(Verdict::Approved) => "approved",
        Some(Verdict::Rejected) => "rejected",
        None => "not answered",
    }
}

fn item_line(item: &Reference) -> String {
    let comment = item
        .comment
        .as_ref()
        .map_or_else(String::new, |comment| format!(" — \"{comment}\""));
    format!(
        "- {} ({}): {}{comment}",
        item.id,
        verdict_word(item.verdict),
        item.note
    )
}

fn told_designer(set: &ReferenceSet) -> String {
    let mut lines = vec![format!(
        "The user answered your design references for step {} (round {}):",
        set.step, set.round
    )];
    lines.extend(set.items.iter().map(item_line));
    if let Some(comment) = &set.comment {
        lines.push(format!("Their comment: \"{comment}\""));
    }
    lines.push(if set.status == SetStatus::Approved {
        "Write your execution plan from the approved ones, following their comments.".to_string()
    } else {
        "None was approved: show a new round built from their comments (references_add, then references_submit).".to_string()
    });
    lines.join("\n")
}

/// The plan's References section: what the user approved, and what they
/// said, for the floor and the reviewers who read the plan.
pub fn plan_section(set: &ReferenceSet) -> String {
    let mut lines = vec![
        String::new(),
        "## References".to_string(),
        String::new(),
        format!(
            "Approved by the user (round {}). Build to these; the comments are the user's.",
            set.round
        ),
        String::new(),
    ];
    lines.extend(
        set.items
            .iter()
            .filter(|item| item.verdict == Some(Verdict::Approved))
            .map(item_line),
    );
    let rejected: Vec<String> = set
        .items
        .iter()
        .filter(|item| item.verdict == Some(Verdict::Rejected))
        .map(item_line)
        .collect();
    if !rejected.is_empty() {
        lines.push(String::new());
        lines.push("Rejected (do not build these):".to_string());
        lines.extend(rejected);
    }
    if let Some(comment) = &set.comment {
        lines.push(String::new());
        lines.push(format!("The user's comment: \"{comment}\""));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Keeps an image the user gives for the designers.
pub fn give(task: &Task, file: &Path, note: &str) -> Result<String> {
    let (bytes, kind) = read_reference(file)?;
    ensure!(
        kind.is_image(),
        "{} is an HTML page; give an image",
        file.display()
    );
    give_bytes(task, &bytes, kind, note)
}

/// Keeps an image the user gives, from its bytes (the page's upload).
pub fn give_bytes(task: &Task, bytes: &[u8], kind: FileKind, note: &str) -> Result<String> {
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "an image may be at most {} MB",
        MAX_BYTES / (1024 * 1024)
    );
    task.transaction(|tx| {
        // Never a number used before: a removed image's id stays its own.
        let next = tx
            .state
            .user_references
            .iter()
            .filter_map(|item| item.id.strip_prefix('u')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            .max(tx.state.user_references_given);
        tx.state.user_references_given = next + 1;
        let id = format!("u{}", next + 1);
        let name = format!("{id}.{}", kind.extension());
        write_file(&user_folder(task), &name, bytes)?;
        tx.state.user_references.push(Reference {
            id: id.clone(),
            file: name,
            kind,
            note: note.trim().to_string(),
            verdict: None,
            comment: None,
        });
        tx.log(
            LogKind::Plan,
            None,
            None,
            &format!("the user gave design reference {id}"),
        );
        let about = if note.trim().is_empty() {
            String::new()
        } else {
            format!(" (\"{}\")", note.trim())
        };
        tx.state.say(
            LEADER,
            format!("The user gave design reference {id}{about}: look at it with design_references view={id}. Every designer reads it before showing directions."),
        );
        Ok(id)
    })
}

/// Takes back an image the user gave.
pub fn remove_given(task: &Task, id: &str) -> Result<()> {
    task.transaction(|tx| {
        let place = tx
            .state
            .user_references
            .iter()
            .position(|item| item.id == id)
            .with_context(|| format!("you gave no image {id}"))?;
        let removed = tx.state.user_references.remove(place);
        let _ = std::fs::remove_file(user_folder(task).join(&removed.file));
        tx.log(
            LogKind::Plan,
            None,
            None,
            &format!("the user took back design reference {id}"),
        );
        Ok(())
    })
}

/// The most an image the leader looks at may weigh: what its model takes in.
const VIEW_BYTES: usize = 5 * 1024 * 1024;

/// What the leader sees of a reference: `u1` (one the user gave) or
/// `G9/1/r2` (a designer's). An image comes as itself; an SVG or a mockup as
/// its source.
pub enum View {
    Image {
        note: String,
        mime: &'static str,
        bytes: Vec<u8>,
    },
    Source {
        note: String,
        text: String,
    },
}

pub fn view(task: &Task, id: &str) -> Result<View> {
    let state = task.state()?;
    let (path, item) = if let Some(item) = state.user_references.iter().find(|item| item.id == id) {
        (user_folder(task).join(&item.file), item.clone())
    } else {
        let mut parts = id.split('/');
        let (Some(step), Some(round), Some(name), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            bail!(
                "{id} is not a reference: u1 for yours, G9/1/r2 for a designer's (design_references lists them)"
            );
        };
        let set = state
            .references
            .iter()
            .find(|set| set.step == step && set.round.to_string() == round)
            .with_context(|| format!("no round {round} of {step}"))?;
        let item = set
            .items
            .iter()
            .find(|item| item.id == name)
            .with_context(|| format!("round {round} of {step} has no {name}"))?;
        (set.folder(task).join(&item.file), item.clone())
    };
    let bytes = std::fs::read(&path).with_context(|| format!("reading {id}"))?;
    let note = item.note.clone();
    Ok(match item.kind {
        FileKind::Png | FileKind::Jpeg | FileKind::Webp => {
            ensure!(
                bytes.len() <= VIEW_BYTES,
                "{id} is {} MB, more than you can look at ({} MB): ask the user for a smaller one",
                bytes.len() / (1024 * 1024),
                VIEW_BYTES / (1024 * 1024)
            );
            View::Image {
                note,
                mime: item.kind.content_type(),
                bytes,
            }
        }
        FileKind::Svg | FileKind::Html => View::Source {
            note,
            text: String::from_utf8_lossy(&bytes)
                .chars()
                .take(40_000)
                .collect(),
        },
    })
}

/// Every reference, for the leader: the user's images and each round.
pub fn listing(state: &State) -> serde_json::Value {
    serde_json::json!({
        "yours": state.user_references.iter().map(|item| serde_json::json!({
            "id": item.id, "note": item.note, "kind": item.kind,
        })).collect::<Vec<_>>(),
        "rounds": state.references.iter().map(|set| serde_json::json!({
            "step": set.step, "agent": set.agent, "round": set.round, "status": set.status,
            "comment": set.comment,
            "items": set.items.iter().map(|item| serde_json::json!({
                "id": format!("{}/{}/{}", set.step, set.round, item.id),
                "note": item.note, "kind": item.kind, "verdict": item.verdict, "comment": item.comment,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "how": "design_references view=<id> shows one; the user answers rounds on the references page, never you",
    })
}

/// What a designer sees: the user's images, copied into its scratch folder
/// so it can open them, and its rounds with the user's answers.
pub fn worker_status(task: &Task, agent: &str) -> Result<serde_json::Value> {
    let state = task.state()?;
    let folder = scratch_folder(task, agent).join("references");
    let mut given = Vec::new();
    for reference in &state.user_references {
        std::fs::create_dir_all(&folder)?;
        std::fs::copy(
            user_folder(task).join(&reference.file),
            folder.join(&reference.file),
        )
        .with_context(|| format!("copying the user's image {}", reference.id))?;
        given.push(serde_json::json!({
            "id": reference.id,
            "file": format!("{SCRATCH_MOUNT}/references/{}", reference.file),
            // The same file where your own program can open it as an image.
            "on_this_machine": folder.join(&reference.file),
            "note": reference.note,
        }));
    }
    let step = state.open_order_of(agent).map(|order| order.step.clone());
    let rounds: Vec<&ReferenceSet> = state
        .references
        .iter()
        .filter(|set| Some(&set.step) == step.as_ref())
        .collect();
    Ok(serde_json::json!({
        "user_images": given,
        "step": step,
        "rounds": rounds,
        "next": step.as_deref().and_then(|step| plan_refusal(&state, agent, step))
            .unwrap_or_else(|| "write your plan from the approved references".to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Agent, Order, OrderStatus};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn designer(id: &str) -> Agent {
        Agent {
            id: id.into(),
            role: "designer".into(),
            agent: "codex".into(),
            repo: "app".into(),
            session: String::new(),
            pane: "%1".into(),
            writes: false,
            group: Some(crate::roles::Group::Senior),
            lane: None,
            seat: 0,
            live: true,
            unavailable: false,
            ready: true,
            lost: false,
            usage: None,
            shows_references: true,
        }
    }

    fn task_with_designer() -> (tempfile::TempDir, Task) {
        let dir = tempfile::TempDir::new().unwrap();
        let config: crate::task::TaskConfig =
            toml::from_str("id = \"t\"\nbranch = \"feat/x\"\nworkspace = \"dev-a\"\n").unwrap();
        let task = Task::create(&crate::home::Home::at(dir.path().to_path_buf()), &config).unwrap();
        task.transaction(|tx| {
            tx.state.agents.push(designer("designer-1"));
            tx.state.orders.push(Order {
                id: "o1".into(),
                step: "G9".into(),
                agent: "designer-1".into(),
                text: "plan G9".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: String::new(),
                status: OrderStatus::Open,
                delivered: true,
                stalled: false,
                plan: None,
                read: vec![],
                targets: vec![],
            });
            Ok(())
        })
        .unwrap();
        (dir, task)
    }

    fn scratch_png(task: &Task, name: &str) -> PathBuf {
        let folder = scratch_folder(task, "designer-1");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(name);
        std::fs::write(&file, PNG).unwrap();
        file
    }

    fn answers(pairs: &[(&str, Verdict)]) -> BTreeMap<String, Answer> {
        pairs
            .iter()
            .map(|(id, verdict)| {
                (
                    id.to_string(),
                    Answer {
                        verdict: *verdict,
                        comment: Some(format!("about {id}")),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_file_is_known_by_its_content_not_its_name() {
        assert_eq!(FileKind::of(PNG), Some(FileKind::Png));
        assert_eq!(
            FileKind::of(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(FileKind::Jpeg)
        );
        assert_eq!(FileKind::of(b"RIFF\0\0\0\0WEBPVP8 "), Some(FileKind::Webp));
        assert_eq!(FileKind::of(b"  <svg xmlns='x'/>"), Some(FileKind::Svg));
        assert_eq!(FileKind::of(b"<!DOCTYPE html><p>"), Some(FileKind::Html));
        assert_eq!(FileKind::of(b"#!/bin/sh\nrm -rf /"), None);
    }

    #[test]
    fn a_designer_plans_only_after_the_user_approves_references() {
        let (_dir, task) = task_with_designer();
        let refusal = plan_refusal(&task.state().unwrap(), "designer-1", "G9").unwrap();
        assert!(refusal.contains("references_add"), "{refusal}");
        let error = crate::plans::store_execution(&task, "designer-1", "# Plan")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("plans only from references the user approved"),
            "{error}"
        );

        add(
            &task,
            "designer-1",
            &scratch_png(&task, "a.png"),
            "dense table",
        )
        .unwrap();
        add(&task, "designer-1", &scratch_png(&task, "b.png"), "cards").unwrap();
        submit(&task, "designer-1").unwrap();
        let state = task.state().unwrap();
        assert!(waits_for_user(&state, "designer-1"));
        assert!(
            plan_refusal(&state, "designer-1", "G9")
                .unwrap()
                .contains("not answered")
        );
        assert!(
            state.queue.iter().any(|message| message.to == LEADER && message.text.contains("2 design references")),
            "the leader is told"
        );

        let status = answer(
            &task,
            "G9",
            &answers(&[("r1", Verdict::Rejected), ("r2", Verdict::Approved)]),
            Some("less shadow"),
        )
        .unwrap();
        assert_eq!(status, SetStatus::Approved);
        let state = task.state().unwrap();
        assert_eq!(plan_refusal(&state, "designer-1", "G9"), None);
        let told = state
            .queue
            .iter()
            .find(|message| message.to == "designer-1")
            .unwrap();
        assert!(
            told.text.contains("r2 (approved): cards — \"about r2\""),
            "{}",
            told.text
        );
        assert!(told.text.contains("less shadow"));
        let stored =
            crate::plans::store_execution(&task, "designer-1", "# Plan\n1. Build B.\n").unwrap();
        assert_eq!(stored, "G9");
        let plan = crate::plans::read(&task, "G9").unwrap();
        assert!(plan.starts_with("# Plan\n1. Build B.\n"), "{plan}");
        assert!(
            plan.contains("## References"),
            "the plan carries what the user approved: {plan}"
        );
        let section = plan_section(approved_for(&state, "G9").unwrap());
        assert!(section.contains("## References"));
        assert!(section.contains("r2 (approved)"));
        assert!(section.contains("Rejected (do not build these):\n- r1"));
    }

    #[test]
    fn a_round_rejected_whole_starts_a_new_one() {
        let (_dir, task) = task_with_designer();
        add(&task, "designer-1", &scratch_png(&task, "a.png"), "dense").unwrap();
        submit(&task, "designer-1").unwrap();
        let status = answer(&task, "G9", &answers(&[("r1", Verdict::Rejected)]), None).unwrap();
        assert_eq!(status, SetStatus::Rejected);
        assert!(
            plan_refusal(&task.state().unwrap(), "designer-1", "G9")
                .unwrap()
                .contains("rejected")
        );

        add(&task, "designer-1", &scratch_png(&task, "b.png"), "airy").unwrap();
        let state = task.state().unwrap();
        let latest = latest(&state, "G9").unwrap();
        assert_eq!((latest.round, latest.status), (2, SetStatus::Drafting));
        assert!(round_folder(&task, "G9", 2).join("r1.png").is_file());
    }

    #[test]
    fn every_reference_needs_an_answer() {
        let (_dir, task) = task_with_designer();
        add(&task, "designer-1", &scratch_png(&task, "a.png"), "a").unwrap();
        add(&task, "designer-1", &scratch_png(&task, "b.png"), "b").unwrap();
        submit(&task, "designer-1").unwrap();
        let error = answer(&task, "G9", &answers(&[("r1", Verdict::Approved)]), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("r2 not answered"), "{error}");
        let error = answer(
            &task,
            "G9",
            &answers(&[
                ("r1", Verdict::Approved),
                ("r2", Verdict::Approved),
                ("r9", Verdict::Approved),
            ]),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("no reference r9"), "{error}");
    }

    #[test]
    fn what_the_user_saw_cannot_change_after() {
        let (_dir, task) = task_with_designer();
        let source = scratch_png(&task, "a.png");
        add(&task, "designer-1", &source, "a").unwrap();
        std::fs::write(&source, b"<!doctype html>changed").unwrap();
        let stored = round_folder(&task, "G9", 1).join("r1.png");
        assert_eq!(std::fs::read(stored).unwrap(), PNG);
    }

    #[test]
    fn add_refuses_what_is_not_a_reference() {
        let (_dir, task) = task_with_designer();
        let script = scratch_folder(&task, "designer-1").join("x.png");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"#!/bin/sh").unwrap();
        let error = add(&task, "designer-1", &script, "x")
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a PNG"), "{error}");

        for number in 0..MAX_PER_ROUND {
            add(
                &task,
                "designer-1",
                &scratch_png(&task, &format!("{number}.png")),
                "n",
            )
            .unwrap();
        }
        let error = add(&task, "designer-1", &scratch_png(&task, "z.png"), "n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("at most 8"), "{error}");

        task.transaction(|tx| {
            tx.state.orders[0].status = OrderStatus::Closed;
            Ok(())
        })
        .unwrap();
        let error = add(&task, "designer-1", &scratch_png(&task, "y.png"), "n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("open order"), "{error}");
    }

    #[test]
    fn a_role_without_references_plans_as_before() {
        let (_dir, task) = task_with_designer();
        task.transaction(|tx| {
            tx.state.agents[0].shows_references = false;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            plan_refusal(&task.state().unwrap(), "designer-1", "G9"),
            None
        );
        let error = add(&task, "designer-1", &scratch_png(&task, "a.png"), "a")
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not show references"), "{error}");
    }

    #[test]
    fn a_scratch_path_never_leaves_the_scratch_folder() {
        let (_dir, task) = task_with_designer();
        scratch_png(&task, "a.png");
        assert!(scratch_file(&task, "designer-1", "/scratch/a.png").is_ok());
        std::fs::write(task.dir().join("secret.png"), PNG).unwrap();
        assert!(scratch_file(&task, "designer-1", "/scratch/../../../secret.png").is_err());
        assert!(scratch_file(&task, "designer-1", "/scratch/missing.png").is_err());
    }

    #[test]
    fn the_users_images_reach_the_designer() {
        let (dir, task) = task_with_designer();
        let image = dir.path().join("mine.png");
        std::fs::write(&image, PNG).unwrap();
        assert_eq!(give(&task, &image, "like Linear's density").unwrap(), "u1");
        let status = worker_status(&task, "designer-1").unwrap();
        assert_eq!(
            status["user_images"][0]["file"],
            "/scratch/references/u1.png"
        );
        assert!(
            scratch_folder(&task, "designer-1")
                .join("references/u1.png")
                .is_file()
        );

        let page = dir.path().join("page.html");
        std::fs::write(&page, b"<!doctype html>").unwrap();
        assert!(
            give(&task, &page, "x")
                .unwrap_err()
                .to_string()
                .contains("give an image")
        );
    }

    #[test]
    fn the_leader_hears_of_a_given_image_and_can_look_at_it() {
        let (dir, task) = task_with_designer();
        let image = dir.path().join("mine.png");
        std::fs::write(&image, PNG).unwrap();
        give(&task, &image, "this density").unwrap();
        let state = task.state().unwrap();
        let told = state
            .queue
            .iter()
            .find(|message| message.to == LEADER)
            .unwrap();
        assert!(
            told.text.contains("design_references view=u1"),
            "{}",
            told.text
        );
        assert!(told.text.contains("this density"));

        match view(&task, "u1").unwrap() {
            View::Image { note, mime, bytes } => {
                assert_eq!(
                    (note.as_str(), mime, bytes.as_slice()),
                    ("this density", "image/png", PNG)
                );
            }
            View::Source { .. } => panic!("a PNG is shown as an image"),
        }
        let mockup = scratch_folder(&task, "designer-1").join("m.html");
        std::fs::create_dir_all(mockup.parent().unwrap()).unwrap();
        std::fs::write(&mockup, b"<!doctype html><h1>Cards</h1>").unwrap();
        add(&task, "designer-1", &mockup, "cards").unwrap();
        match view(&task, "G9/1/r1").unwrap() {
            View::Source { text, .. } => assert!(text.contains("<h1>Cards</h1>")),
            View::Image { .. } => panic!("a mockup is shown as its source"),
        }
        assert!(view(&task, "G9/2/r1").is_err());
        assert!(view(&task, "../../key").is_err());
        let listing = listing(&task.state().unwrap());
        assert_eq!(listing["yours"][0]["id"], "u1");
        assert_eq!(listing["rounds"][0]["items"][0]["id"], "G9/1/r1");
    }

    #[test]
    fn a_removed_images_id_is_never_given_again() {
        let (dir, task) = task_with_designer();
        let image = dir.path().join("mine.png");
        std::fs::write(&image, PNG).unwrap();
        give(&task, &image, "a").unwrap();
        give(&task, &image, "b").unwrap();
        remove_given(&task, "u2").unwrap();
        assert!(!user_folder(&task).join("u2.png").exists());
        assert_eq!(give(&task, &image, "c").unwrap(), "u3");
        assert!(remove_given(&task, "u9").is_err());
    }
}
