# Plan: Team role settings in the menu, and Codex agents that start unattended

- **Date:** 09-11-2026
- **Branch:** feat/agentic
- **Status:** Executed 09-11-2026 — `make check` passed; new build installed;
  the live trust check waits for the leader's next Codex Floor agent

## Context

Implements [spec.md](spec.md): a `devagentics role set` command that edits one
role's model, effort or approvals in place; a **⚙ Team roles** menu item that
drives it; and devws trusting the task folders it creates, so Codex agents in
new lanes start without the "Trust this folder?" prompt.

Done by hand on 09-11-2026, outside this plan: `hammer-claude` uses
`claude-sonnet-5-5`, `ui-designer` uses `approvals = "never"`.

## Step 1 — Trust a task folder before Codex starts there

**Status:** Done

**Files:**
- `config/tmuxinator/agent_runner.sh`
- `scripts/test_agent_cli.sh`

**Actions:**
- Tests first, with the stub `codex` the suite already uses and a private
  `CODEX_HOME`: an agent started in `$DEVAGENTICS_HOME/tasks/t/lanes/ops` adds
  `[projects."<that folder, physical path>"]` / `trust_level = "trusted"` to its
  `config.toml`; starting it again adds nothing; an agent in a folder outside
  the tasks folder adds nothing; existing lines in the file are unchanged.
- In `agent_runner.sh`, for the codex kind only, after `CODEX_HOME` is set:
  resolve the folder with `pwd -P`, return unless it is under the tasks folder,
  and append the table when the file has no `[projects."<folder>"]` line.
  Create the config file (and folder) when missing. A failure to write is
  reported on stderr and does not stop the agent: Codex then asks, as today.

**Verification:**
- `bash scripts/test_agent_cli.sh` passes; the new cases failed before the change.

## Step 2 — `devagentics role set`

**Status:** Done

**Files:**
- `agentic/src/roles.rs`
- `agentic/src/main.rs`

**Actions:**
- Unit tests first, in `roles.rs`'s test module with `home_with`: changing a
  model rewrites only that line; a missing key is added before the closing
  `+++`; an empty value removes the key's line; instructions and other lines are
  byte-identical; invalid effort or approvals for the role's program (Claude or
  Codex, from the agent name's kind) is refused naming the allowed values; the
  leader and unknown roles are refused; a repo role is written in the repo's
  `.devws/roles`; the edited text is parsed (`parse`) before it is written.
- Add `pub fn set(home, repo, name, kind, changes) -> Result<PathBuf>` (the
  command resolves the kind through devws, as `claude_work` is a Claude account), with the
  per-program value lists as constants beside `AUTO`'s meaning.
- `RoleAction::Set { role, --model, --effort, --approvals, --repo }`, printing
  the file it changed and that running agents keep their settings.
- `role list` adds effort and approvals columns (`-` when unset).

**Verification:**
- `cargo test` in `agentic/` passes; `cargo clippy -- -D warnings` and
  `cargo fmt --check` are clean.

## Step 3 — **⚙ Team roles** in the menu

**Status:** Done

**Files:**
- `config/tmuxinator/devws_menu.sh`
- `config/tmuxinator/workspace_actions.sh`
- `scripts/test_devws_menu.sh`

**Actions:**
- Tests first, with the suite's fzf answer queue and a stub agentic command
  (as `switch-leader` does): the item is listed in the Agent tab of a normal
  workspace; choosing a role, **Model** and a model calls
  `role set <role> --model <model>`; **Default** passes an empty value;
  **Other…** passes the typed text; Esc at any step calls nothing.
- Add `agent|team-roles|⚙ Team roles||team_roles|` to the actions table.
- `team_roles` in `workspace_actions.sh`: `role list --repo <workspace root>`
  for the role picker (leader left out); then Model / Effort / Approvals; then
  the values for the role's program, from `role set`'s lists printed by the
  command rather than repeated in shell; show the command's message and pause.

**Verification:**
- `bash scripts/test_devws_menu.sh` passes.

## Step 4 — Documentation

**Status:** Done

**Files:**
- `agentic/docs/usage.md`
- `docs/guide/agentic.md`

**Actions:**
- Document `role set` and the menu item next to the role file format, and that
  devws trusts the task folders it creates for Codex (and no others).

**Verification:**
- `bash scripts/test_guide.sh` passes (it checks the guide's links).

## Final Verification

1. `make check` passes (Rust format, Clippy, tests, and every shell suite).
2. `git diff --check` is clean.
3. Live: `devagentics role set hammer-claude --effort low` reports the file and
   `role list` shows it; a new Codex Floor agent in a new task lane starts at
   its prompt with no trust question.
