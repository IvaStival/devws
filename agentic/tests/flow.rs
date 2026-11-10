//! A whole task, through the real binary: a stub `devws` (panes and states in
//! files), a real git repo with a real origin, a stub `gh`.

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

struct World {
    dir: TempDir,
    repo: TempDir,
    origin: TempDir,
}

fn git(dir: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?}");
}

fn script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl World {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let origin = TempDir::new().unwrap();
        for sub in ["bin", "home", "state", "sent", "home/roles"] {
            fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        for name in ["planner", "executor"] {
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("roles/samples/{name}.md")),
                dir.path().join(format!("home/roles/{name}.md")),
            )
            .unwrap();
        }
        let bin = dir.path().join("bin");
        script(
            &bin.join("devws"),
            r#"#!/bin/sh
DIR="$DEVAGENTICS_TEST_DIR"
echo "$*" >> "$DIR/devws.calls"
case "$1 $2" in
  "agent kind") echo claude ;;
  "agent spawn") n=$(cat "$DIR/pane.counter" 2>/dev/null || echo 6); n=$((n+1)); echo $n > "$DIR/pane.counter"; echo "%$n" ;;
  "agent state") cat "$DIR/state/$3" 2>/dev/null || echo idle ;;
  "agent send") cat >> "$DIR/sent/$3"; echo >> "$DIR/sent/$3" ;;
  "agent limits") echo "- -" ;;
  "agent usage") [ "$3" = "%7" ] && echo "1200 300 4000 0.5 0" || echo - ;;
  "agent mark"|"agent stop") ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#,
        );
        script(
            &bin.join("tmux"),
            "#!/bin/sh\ncase \"$1\" in list-panes) echo '%1 agent' ;; esac\n",
        );
        script(
            &bin.join("gh"),
            &format!(
                r#"#!/bin/sh
case "$1 $2" in
  "pr view") [ -f "{dir}/pr" ] && echo "https://github.com/acme/app/pull/1" || exit 1 ;;
  "pr create") touch "{dir}/pr"; echo "https://github.com/acme/app/pull/1" ;;
esac
"#,
                dir = dir.path().display()
            ),
        );

        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(repo.path(), &["init", "-q", "-b", "main"]);
        fs::write(repo.path().join("README.md"), "hello\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "first"]);
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                &origin.path().display().to_string(),
            ],
        );
        git(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        World { dir, repo, origin }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn command(&self, pane: Option<&str>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_devagentics"));
        command
            .env("DEVAGENTICS_HOME", self.path("home"))
            .env("DEVAGENTICS_DEVWS", self.path("bin/devws"))
            .env("DEVAGENTICS_TMUX", self.path("bin/tmux"))
            .env("DEVAGENTICS_GH", self.path("bin/gh"))
            .env("DEVAGENTICS_TEST_DIR", self.dir.path())
            .env_remove("TMUX_PANE");
        if let Some(pane) = pane {
            command.env("TMUX_PANE", pane);
        }
        command
    }

    /// Runs `devagentics`: whether it succeeded, its output, its error output.
    fn run(&self, pane: Option<&str>, arguments: &[&str], stdin: &str) -> (bool, String, String) {
        let mut child = self
            .command(pane)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn ok(&self, pane: Option<&str>, arguments: &[&str], stdin: &str) -> String {
        let (success, stdout, stderr) = self.run(pane, arguments, stdin);
        assert!(success, "devagentics {arguments:?} failed: {stderr}");
        stdout
    }

    /// The leader's side: tool calls over a session on standard input, as the
    /// leader's program makes them. Returns (is an error, text) for each.
    fn leader(&self, task: &str, calls: &[(&str, Value)]) -> Vec<(bool, String)> {
        let mut requests = vec![
            json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        ];
        for (number, (name, arguments)) in calls.iter().enumerate() {
            requests.push(json!({ "jsonrpc": "2.0", "id": number + 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } }));
        }
        let input = requests
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let stdout = self.ok(None, &["mcp", task], &input);
        let responses: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), calls.len() + 1, "{stdout}");
        responses[1..]
            .iter()
            .map(|response| {
                (
                    response["result"]["isError"].as_bool().unwrap(),
                    response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                )
            })
            .collect()
    }

    fn set_state(&self, pane: &str, state: &str) {
        fs::write(self.path("state").join(pane), state).unwrap();
    }

    /// An approved task whose senior, planner-1 in %7, has an accepted
    /// execution plan for step p1: the factory floor may be ordered.
    fn planned(&self) {
        self.ok(
            None,
            &["task", "start", &self.repo.path().display().to_string()],
            "",
        );
        self.leader("feat-x", &[("spawn_agent", json!({ "role": "planner" }))]);
        self.ok(Some("%7"), &["plan", "write"], "1. go\n");
        self.ok(None, &["task", "approve", "feat-x"], "");
        self.leader(
            "feat-x",
            &[(
                "send_order",
                json!({ "agent": "planner-1", "step": "p1", "title": "Plan", "text": "plan it", "check": "true" }),
            )],
        );
        self.ok(Some("%7"), &["plan", "write"], "1. do it\n");
        self.ok(Some("%7"), &["report", "done", "planned"], "");
        let accepted = self.leader("feat-x", &[("accept_step", json!({ "step": "p1" }))]);
        assert!(!accepted[0].0, "{accepted:?}");
    }

    fn sent(&self, pane: &str) -> String {
        fs::read_to_string(self.path("sent").join(pane)).unwrap_or_default()
    }

    fn devws_calls(&self) -> String {
        fs::read_to_string(self.path("devws.calls")).unwrap_or_default()
    }

    fn commit(&self, file: &str, text: &str) {
        let path = self.repo.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        git(self.repo.path(), &["add", "-A"]);
        git(self.repo.path(), &["commit", "-q", "-m", file]);
    }
}

#[test]
fn a_whole_task_from_the_plan_to_the_pull_request() {
    let world = World::new();

    // `devws agentic` starts the task and gets the leader's arguments.
    let started = world.ok(
        None,
        &["task", "start", &world.repo.path().display().to_string()],
        "",
    );
    let lines: Vec<_> = started.lines().collect();
    assert_eq!(&lines[..2], ["feat-x", "claude"], "{started}");
    let leader_arguments = fs::read(lines[2]).unwrap();
    assert!(String::from_utf8_lossy(&leader_arguments).contains("--mcp-config"));

    // The leader has a planner write the plan.
    let spawned = world.leader("feat-x", &[("spawn_agent", json!({ "role": "planner" }))]);
    let (failed, text) = &spawned[0];
    assert!(
        !failed && text.contains("Started planner-1") && text.contains("pane %7"),
        "{text}"
    );
    let stored = world.ok(Some("%7"), &["plan", "write"], "1. s1: add src/a.rs\n");
    assert!(stored.contains("the user, who approves it"), "{stored}");

    // Writers wait for the user.
    let refusal = world.leader("feat-x", &[("spawn_agent", json!({ "role": "executor" }))]);
    let (refused, reason) = &refusal[0];
    assert!(*refused && reason.contains("not approved"), "{reason}");
    let approval = world.leader("feat-x", &[("approve_plan", json!({}))]);
    assert!(
        !approval[0].0 && approval[0].1.contains("approved"),
        "{approval:?}"
    );

    // The senior plans the execution; the CTO reads and accepts that plan.
    let results = world.leader(
        "feat-x",
        &[(
            "send_order",
            json!({ "agent": "planner-1", "step": "p1", "title": "Plan a.rs", "text": "Plan the execution of s1", "check": "true" }),
        )],
    );
    assert!(!results[0].0, "{results:?}");
    world.set_state("%7", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    let order = world.sent("%7");
    assert!(order.contains("You change no files"), "{order}");
    let stored = world.ok(Some("%7"), &["plan", "write"], "1. create src/a.rs\n");
    assert!(stored.contains("Execution plan of step p1"), "{stored}");
    world.ok(Some("%7"), &["report", "done", "plan written"], "");
    let results = world.leader(
        "feat-x",
        &[
            ("read_plan", json!({ "step": "p1" })),
            ("spawn_agent", json!({ "role": "executor" })),
            ("send_order", json!({ "agent": "executor-1", "step": "s1", "title": "Add a.rs", "text": "Create src/a.rs", "allowed": ["src/**"], "check": "test -f src/a.rs", "plan": "p1" })),
            ("accept_step", json!({ "step": "p1" })),
            ("send_order", json!({ "agent": "executor-1", "step": "s1", "title": "Add a.rs", "text": "Create src/a.rs", "allowed": ["src/**"], "check": "test -f src/a.rs", "plan": "p1" })),
        ],
    );
    assert_eq!(results[0], (false, "1. create src/a.rs\n".to_string()));
    assert!(!results[1].0, "{results:?}");
    assert!(
        results[2].0 && results[2].1.contains("not accepted yet"),
        "the floor waits for the CTO to accept the plan: {results:?}"
    );
    assert!(!results[3].0 && !results[4].0, "{results:?}");

    // The order waits while the executor starts up, and goes in once it answers.
    world.ok(None, &["watch", "feat-x", "--once"], "");
    assert_eq!(
        world.sent("%8"),
        "",
        "an order was typed into an agent still starting"
    );
    world.set_state("%8", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    let order = world.sent("%8");
    assert!(
        order.contains("ORDER o")
            && order.contains("Create src/a.rs")
            && order.contains("devagentics report done"),
        "{order}"
    );

    // It reads the plan, asks the senior, and gets the answer typed in.
    assert_eq!(
        world.ok(Some("%8"), &["plan", "show", "p1"], ""),
        "1. create src/a.rs\n"
    );
    let asked = world.ok(Some("%8"), &["ask", "Public or private?"], "");
    assert!(asked.contains("Asked planner-1"), "{asked}");
    world.set_state("%7", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    let question = world.sent("%7");
    assert!(
        question.contains("QUESTION q") && question.contains("Public or private?"),
        "{question}"
    );
    let id = question
        .split_whitespace()
        .find(|word| word.starts_with('q') && word[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap()
        .to_string();
    world.ok(Some("%7"), &["answer", &id, "public"], "");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    assert!(
        world.sent("%8").contains("ANSWER to"),
        "{}",
        world.sent("%8")
    );

    // It does the work and reports; devAgentics checks.
    world.commit("src/a.rs", "pub fn a() {}\n");
    let answer = world.ok(Some("%8"), &["report", "done", "added src/a.rs"], "");
    assert!(answer.contains("review"), "{answer}");

    // The leader is told once it has finished its turn.
    world.set_state("%1", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    let told = world.sent("%1");
    for wanted in [
        "executor-1 reports step s1 done.",
        "Tree: clean",
        "rules kept; 1 changed",
        "Check: passed",
    ] {
        assert!(
            told.contains(wanted),
            "the leader is not told '{wanted}': {told}"
        );
    }

    // It accepts the step and opens the pull request.
    let results = world.leader(
        "feat-x",
        &[
            ("accept_step", json!({ "step": "s1" })),
            ("open_prs", json!({})),
        ],
    );
    assert!(results.iter().all(|(failed, _)| !failed), "{results:?}");
    assert!(
        results[1].1.contains("https://github.com/acme/app/pull/1"),
        "{}",
        results[1].1
    );
    let branches = Command::new("git")
        .arg("-C")
        .arg(world.origin.path())
        .args(["branch", "--list", "feat/x"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feat/x"),
        "the branch was not pushed"
    );

    // The status and the list agree.
    let status: Value =
        serde_json::from_str(&world.ok(None, &["task", "status", "feat-x"], "")).unwrap();
    assert_eq!(status["steps"][0]["status"], "accepted");
    assert_eq!(status["steps"][1]["status"], "accepted");
    assert_eq!(status["execution_plans"][0]["author"], "planner-1");
    assert_eq!(status["task"]["approved"], true);
    let list = world.ok(None, &["task", "list"], "");
    assert!(
        list.contains("feat-x") && list.contains("2 of 2 steps accepted"),
        "{list}"
    );

    let log = fs::read_to_string(world.path("home/tasks/feat-x/log.jsonl")).unwrap();
    for kind in [
        "start",
        "spawn",
        "plan",
        "approve",
        "order",
        "delivered",
        "report",
        "check",
        "review",
        "question",
        "answer",
        "pr",
    ] {
        assert!(
            log.contains(&format!("\"kind\":\"{kind}\"")),
            "the log has no {kind}: {log}"
        );
    }
}

#[test]
fn a_rule_broken_in_the_work_fails_the_step_and_the_leader_hears_it() {
    let world = World::new();
    world.planned();
    world.leader(
        "feat-x",
        &[
            ("spawn_agent", json!({ "role": "executor" })),
            ("send_order", json!({ "agent": "executor-1", "step": "s1", "title": "T", "text": "x", "forbidden": ["src/secret/"], "check": "true", "plan": "p1" })),
        ],
    );
    world.commit("src/secret/key.rs", "x\n");

    let answer = world.ok(Some("%8"), &["report", "done", "did it"], "");
    assert!(answer.contains("problems"), "{answer}");
    world.set_state("%1", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    assert!(
        world
            .sent("%1")
            .contains("FORBIDDEN changed: src/secret/key.rs"),
        "{}",
        world.sent("%1")
    );

    let results = world.leader("feat-x", &[("accept_step", json!({ "step": "s1" }))]);
    assert!(
        results[0].0 && results[0].1.contains("not waiting for a review"),
        "{results:?}"
    );
    let results = world.leader("feat-x", &[("open_prs", json!({}))]);
    assert!(
        results[0].0 && results[0].1.contains("not every step is accepted"),
        "{results:?}"
    );
}

#[test]
fn after_a_restart_the_leader_hears_of_the_lost_panel_and_resume_starts_it_again() {
    let world = World::new();
    world.ok(
        None,
        &["task", "start", &world.repo.path().display().to_string()],
        "",
    );
    world.leader("feat-x", &[("spawn_agent", json!({ "role": "planner" }))]);

    world.set_state("%7", "gone");
    world.set_state("%1", "done");
    world.ok(None, &["watch", "feat-x", "--once"], "");
    assert!(
        world.sent("%1").contains("planner-1's panel is gone"),
        "{}",
        world.sent("%1")
    );

    let resumed = world.ok(None, &["task", "resume", "feat-x"], "");
    assert!(resumed.contains("Started again: planner-1"), "{resumed}");
    assert_eq!(world.devws_calls().matches("agent spawn").count(), 2);
    let status: Value =
        serde_json::from_str(&world.ok(None, &["task", "status", "feat-x"], "")).unwrap();
    assert_eq!(status["agents"][0]["pane"], "%8");
    assert_eq!(status["agents"][0]["lost"], false);
}

#[test]
fn the_commands_say_what_is_wrong() {
    let world = World::new();
    let (ok, _, error) = world.run(None, &["report", "done", "x"], "");
    assert!(!ok && error.contains("no TMUX_PANE"), "{error}");
    let (ok, _, error) = world.run(Some("%99"), &["report", "done", "x"], "");
    assert!(!ok && error.contains("not the pane of an agent"), "{error}");
    let (ok, _, error) = world.run(None, &["task", "approve", "nothing"], "");
    assert!(!ok && error.contains("no task named nothing"), "{error}");

    world.ok(
        None,
        &["task", "start", &world.repo.path().display().to_string()],
        "",
    );
    let (ok, _, error) = world.run(None, &["task", "approve", "feat-x"], "");
    assert!(!ok && error.contains("no plan to approve"), "{error}");

    let roles = world.ok(None, &["role", "list"], "");
    for role in ["executor", "leader", "planner"] {
        assert!(roles.contains(role), "{roles}");
    }
    assert!(
        world
            .ok(None, &["task", "pause", "feat-x"], "")
            .contains("Paused")
    );
    let list = world.ok(None, &["task", "list"], "");
    assert!(list.contains("paused"), "{list}");
}

#[test]
fn board_views_prioritize_work_and_keep_usage_separate() {
    let world = World::new();
    world.ok(
        None,
        &["task", "start", &world.repo.path().display().to_string()],
        "",
    );
    let roles = world.path("home").join("roles");
    fs::create_dir_all(&roles).unwrap();
    fs::write(
        roles.join("tester.md"),
        "+++\nagent = \"claude\"\ndescription = \"Runs the tests\"\n+++\nTest it.\n",
    )
    .unwrap();
    world.leader("feat-x", &[("spawn_agent", json!({ "role": "planner" }))]);
    world.set_state("%7", "working");

    let board = world.ok(None, &["board", "feat-x", "--once", "--width", "90"], "");
    for expected in [
        "feat-x · feat/x",
        "[overview]",
        "claude · high",
        "CTO",
        "SENIORS · 1",
        "1  planner-1",
        "⠋ working",
        "PROGRESS",
    ] {
        assert!(
            board.contains(expected),
            "missing '{expected}' in:\n{board}"
        );
    }
    assert!(
        !board.contains("cache 4k"),
        "overview should not list token history"
    );
    assert!(
        !board.contains("tester"),
        "inactive roles belong in the secondary catalog"
    );
    let usage = world.ok(None, &["board", "feat-x", "--once", "--view", "usage"], "");
    assert!(usage.contains("planner  in 1k"), "{usage}");
    assert!(usage.contains("cache 4k"), "{usage}");
    for view in ["agents", "steps", "activity"] {
        let text = world.ok(
            None,
            &[
                "board", "feat-x", "--once", "--view", view, "--width", "38", "--height", "8",
            ],
            "",
        );
        assert!(text.contains(&format!("[{view}]")), "{text}");
    }
}
