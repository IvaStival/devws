# Plan: devAgentics v1, agent teams across repos

- **Date:** 03-11-2026
- **Branch:** feat/v1-teams
- **Status:** Pending

## Context

Implements `spec.md`. It depends on devws's `feat/agent-commands`
(`specs/plans/2026-11-03-193644_devagentics-interface/` in devws), which is
done first. Steps 1–8 only need the fake devws; step 9 needs the real one.
Every module is test-first.

CLI facts checked on 03-11-2026:
- **claude:** `--model`, `--effort`, `--mcp-config`, `--strict-mcp-config`,
  `--permission-mode`, `--allowedTools`, and the initial prompt as the last
  argument.
- **codex 0.164:** `-m`, `-s`, `-a`, `-c key=value` (`model_reasoning_effort`,
  `mcp_servers.<name>.command|args`), and the initial prompt.

## Step 1 — Skeleton, task folder and state

**Files:** `Cargo.toml`, `src/main.rs`, `src/task.rs`, `src/log.rs`

**Actions:**
- Dependencies (spec's list). A clap CLI with the subcommands stubbed.
- `TaskHome` (`DEVAGENTICS_HOME`, default `~/.devagentics`). `task.toml`
  types: repos (spec, worktree, check), branch and limits.
- `state.json` types: agents (id, role, repo, pane, account, lock), orders
  (id, step, agent, text, globs, check, base commit, status), steps (id,
  title, status: open, ordered, reported, checked, accepted, rejected) and
  message queues per agent.
- `with_state(|s| …)`: an exclusive `fs2` lock on `state.lock`, then read,
  change, and write to a temp file plus rename.
- `log.rs`: append a JSON line per entry kind (spec 18) under the same lock.

**Verification:**
- Unit tests: a round trip; two threads updating state concurrently lose
  nothing; log lines parse back.

## Step 2 — Roles

**Files:** `src/roles.rs`, `roles/planner.md`, `roles/leader.md`, `roles/executor.md`

**Actions:**
- Parse the `+++` TOML front matter and the body; validate the fields.
- Precedence: the repo's `.devws/roles/`, then `~/.devagentics/roles/`, then
  the built-ins (`include_str!`).
- `role list` prints each role's name, agent, model, `writes`, and where it
  came from.
- Write the three default prompts:
  - **planner:** writes `plan.md` with numbered steps per repo and their
    checks;
  - **leader:** the tools; `task_status` first; one order at a time; review
    each diff; ai-memory handoffs and decisions (spec 19); escalate only what
    it can't decide;
  - **executor:** only the order; only the allowed files; commit; then
    `devagentics report`.

**Verification:**
- Unit: an override wins by name, a broken front matter names the file, and
  the built-ins all parse.

## Step 3 — Role → CLI arguments

**Files:** `src/agent_args.rs`

**Actions:**
- `args_for(role, kind, context) -> Vec<String>`, where context carries the
  first message, and the MCP config for the leader.
  - **claude:** `--model`, `--effort`, `--permission-mode` (the role's
    `approvals`, default `acceptEdits`), `--allowedTools
    "Bash(devagentics report:*)"`, `--mcp-config <file>
    --strict-mcp-config` for the leader, then the first message.
  - **codex:** `-m`, `-c model_reasoning_effort=…`, `-s` (default
    `workspace-write`), `-a` (default `on-request`), `-c
    mcp_servers.devagentics.command=…` for the leader, then the first message.
- The account kind comes from the agent name through `devws` (claude, codex,
  or a `DEVWS_AGENT_<name>` entry).
- Write the args file under the task folder.

**Verification:**
- Unit: snapshot tests of both kinds for each default role.

## Step 4 — The devws seam

**Files:** `src/devws.rs`, `tests/fake_devws/` (a script)

**Actions:**
- `trait Devws`: `ensure_worktree`, `open`, `spawn`, `run`, `state`, `send`
  (Delivered, Busy or Gone), `mark`, `stop`, `limits`.
- `CommandDevws` runs `devws …` (`DEVAGENTICS_DEVWS` overrides the path) and
  maps exit codes 3 and 4. `set_pane_option` is the one direct tmux call
  (spec constraint).
- A fake devws for tests: a script that records calls in a file and answers
  states from a file the test writes.

**Verification:**
- Integration: each method against the fake gives the expected argv and
  parsed result.

## Step 5 — Orders, reports and the file rules

**Files:** `src/orders.rs`, `src/checks.rs`

**Actions:**
- `send_order`:
  - refuses an agent with an open order;
  - records `git rev-parse HEAD` as the base commit;
  - queues the message text (order, files, check, how to report).
- `report(pane, done|blocked, summary)`: finds the agent by pane. For `done`:
  - clean tree, or fail with "uncommitted changes";
  - `git diff --name-only base..HEAD` against the `globset` allowed and
    forbidden lists;
  - the check command (`sh -c`, a 15-minute timeout, output in
    `checks/<step>-<n>.log`).
  It then queues the result for the leader and logs it.
- `accept_step` / `reject_step` change the step and log the review.

**Verification:**
- Integration on temp git repos: a pass; a forbidden file touched; a dirty
  tree; a failing check; a blocked report reaching the leader's queue.

## Step 6 — Spawning, locks and limits

**Files:** `src/team.rs`

**Actions:**
- `spawn_agent(role, repo)`:
  - enforce the writer lock, `max_agents` and 85% of the 5-hour limit, the
    last through `limits` (`-` means unknown and is allowed, with a log
    entry);
  - in the task's first repo: `devws agent spawn … --place team --label
    <role>` into the leader's workspace; in another repo (`add_repo`):
    `devws open --no-attach --task`, the agent as that workspace's main one;
  - mark the pane `@devws_team 1` and `@devagentics_agent <id>`, then queue
    the role prompt plus the task context as its first message, through the
    args file's initial prompt.
- `stop_agent` releases the lock.
- `task start <folder>` (the contract with `devws agentic`: three lines on
  stdout), `task approve`, `pause`, `resume` (respawn gone team agents,
  re-queue their open orders), `cancel`, `list`, `status`, `plan write`.
- The first repo's agents are panels of the leader's workspace; another
  repo's agent opens that repo's workspace (`add_repo`, then `spawn_agent`).
- Worktrees come from `devws worktree ensure <repo> <branch>`, a seam method
  (`ensure_worktree`).

**Verification:**
- Integration with the fake devws: a second writer refused, the fourth agent
  refused, an account at 90% refused; approve swaps the pane; resume
  re-spawns a gone agent with its order.

## Step 7 — Dispatcher and log view (`watch`)

**Files:** `src/watch.rs`

**Actions:**
- A single-instance lock. A loop every second:
  - for each agent with a queued message, `state`: done or idle → `send`,
    then pop and log `delivered`; busy → keep it;
  - `gone` → log it and tell the leader;
  - done with an open order and no report for 2 minutes → tell the leader;
  - paused → deliver nothing.
- Print new log lines as one readable line each.

**Verification:**
- Integration: with the fake's states scripted (working, then done), the
  message is delivered exactly once, after the change; a gone pane is
  reported; nothing goes out while paused.

## Step 8 — MCP server and PRs

**Files:** `src/mcp.rs`, `src/prs.rs`

**Actions:**
- An `rmcp` stdio server, `devagentics mcp <id>`, with the spec's tools
  (8–13), each a thin call into team, orders and log.
- `escalate`: log it, `mark` the leader pane `waiting`.
- `open_prs`: refuse unless every step is accepted. Then, per repo:
  - `git push -u origin <branch>`;
  - `gh pr create --draft --fill` (the body says which task it belongs to);
  - then `gh pr edit --body` on each to add the links to the others.

**Verification:**
- Integration: an MCP client test (rmcp client over a child process) lists
  the tools and calls `task_status`. `open_prs` with stub `git` and `gh` on
  PATH makes 2 creates and 2 edits containing each other's URLs.

## Step 9 — End to end, docs

**Files:** `README.md`, `docs/usage.md`

**Actions:**
- Install with `cargo install --path .` and run the spec's end-to-end
  criterion:
  - two local test repos (a tiny API and web, each with a `make test`);
  - task new → plan → approve → the leader spawns a Claude and a Codex
    executor → one step each → accept → `open_prs` against two throwaway
    private GitHub repos (created and deleted with the user's OK);
  - kill the leader pane, `task resume`, and check it continues.
- Write the usage doc: commands, role files, the task folder.

**Verification:**
- Two linked draft PRs exist; the log shows the whole flow; the resumed
  leader's first call is `task_status`.

## Final Verification

1. `cargo test` passes; `cargo clippy --all-targets -- -D warnings` and
   `cargo fmt --check` are clean.
2. The end-to-end run in step 9 succeeds.
3. devws's `scripts/check.sh` still passes with the devAgentics usage.
