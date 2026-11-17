Status: Executed (17-11-2026); not committed

# OpenCode (DeepSeek Flash) as a team worker provider

## Context
Team workers run only on Claude or Codex today (`Kind { Claude, Codex }`, `agentic/src/agent_args.rs:9`). The user has an **OpenCode Zen** account with prepaid credit and wants cheap models (DeepSeek Flash first, others later) to do floor work, and optionally senior work, so Claude and Codex tokens are saved. They also want:
- a way to register the account,
- a way to choose models,
- usage, credit-used and credit-left statistics,
- menu switches: one **mandatory global** switch plus one switch per group (floor, senior). When the global switch is off, the group switches are ignored and OpenCode is never used.

Decisions already made by the user:
- **Account type:** Zen (balance).
- **Registering:** devws runs `opencode auth login`, and devws keeps no secret itself.

## Design

### 1. New program kind `opencode`
- **`agentic/src/agent_args.rs`:** add `Kind::Opencode` and `parse("opencode")`, and change the test at :532 that currently refuses it.
  - Add `opencode()`, which builds `--model <provider/model>` and `--agent build` for floor or `--agent plan` for senior. Seniors never write, and opencode's built-in `plan` agent denies edits.
- **`config/tmuxinator/agents.sh`:**
  - `agent_default_home`: opencode → `~/.local/share/opencode`.
  - `agent_kind`, `agent_home`: accept the new kind.
- **`config/tmuxinator/agent_runner.sh`:** add a `opencode)` case to the launch block (:131-160). If the binary is missing, the existing `provider_unavailable` path handles it.
- **`agent_cli.sh:291`:** allow interrupting opencode panes.
- **`register_agent.sh:128` and the picker at `workspace_actions.sh:207`:**
  - Accept the `opencode` kind.
  - After registering, run `opencode auth login` (Zen provider) in a popup.
  - A **Login again** action reruns it.
- **Verify early (spike):**
  1. Install opencode.
  2. Confirm the exact Zen DeepSeek Flash id with `opencode models opencode`.
  3. Confirm which env var gives a non-default account folder its own data dir (`XDG_DATA_HOME` or similar).
  4. Confirm how devws detects the pane is ready and how it fills `@devws_transcript` and `@devws_startup_error` for a TUI that isn't Claude or Codex.
  5. Adapt those hooks before building the rest.

### 2. OpenCode settings (switches and models)
New file `~/.devagentics/opencode.toml`, holding preferences only and no credentials:
```toml
enabled = false              # mandatory global switch
account = "opencode"         # DEVWS_AGENTS entry to use
[floor]
enabled = true
model = "opencode/deepseek-…-flash"
[senior]
enabled = false
model = "opencode/deepseek-…-flash"
```
- **New module `agentic/src/opencode.rs`:**
  - `struct OpencodeSettings`, with `load`/`save`. A missing file means everything is off.
  - `fn model_for(group) -> Option<&str>`. It returns `None` when the global switch is off, the group is off, or the group is Cto (the leader never uses opencode).
- **CLI in `main.rs`:** `devagentics opencode show | set <global|floor|senior> <on|off> | model <floor|senior> <id> | models`.
  - `models` lists `opencode models <provider>` live, so the user can test other models later.

### 3. Worker selection
- **`Role::worker_candidates` (`roles.rs:162`):** when `model_for(role.group)` returns a model, put an opencode candidate **first**. The role's existing Claude/Codex candidates stay behind it as fallback.
- **`floor_model` (`roles.rs:119`):** leave opencode models alone; only Claude and Codex are forced.
- **`worker.rs::next_available` (:66):** skip the opencode account when its credit left is ≤ 0. This uses the same reason path as the 85% limit check, so the worker falls back to Claude or Codex.
- **`failure_from_record` (`worker.rs:115`):** map opencode's insufficient-balance and rate-limit codes to `Account` and model-not-found to `Model`. This is still structured records only.
- **`roles.rs::Setting::choices`:** add the opencode model list, so **⚙ Team roles** can also pin opencode per role.

### 4. Usage and credit
- **Used:** opencode stores a `cost` and token counts on each message in its data dir.
  - Add an opencode reader to `config/tmuxinator/header_tokens.sh`, with output in the same `usage`/`messages` format, so that `usage_history.sh`, the header and the board's `usage_card` (`agentic/src/board_view.rs:442`) pick it up unchanged.
  - Use the stored cost when present, otherwise `agent_prices.tsv`. Cache it with `token_cache.pl` like the others.
- **Credit left:**
  - In the spike, check whether Zen exposes a balance endpoint.
  - If it does, add `opencode_credit.sh`, modelled on `claude_limits.sh` (it reads the token from opencode's `auth.json` and sends it on stdin).
  - If it doesn't, the fallback is a **Set balance** menu action that saves the balance and the date. Left = saved balance − cost since that date, labelled "estimated".
- **Shown in:**
  - The board's account lines (`board_view.rs:863`): used / left.
  - The header agent bar.
  - The **OpenCode** menu screen.

### 5. Menu (`config/tmuxinator/devws_menu.sh`, `workspace_actions.sh`)
A new row in the agent tab, `◆ OpenCode`, opens a submenu:
- Global: on/off. The label shows **mandatory**, and when it is off the group rows display "(ignored)".
- Floor: on/off · Floor model…
- Senior: on/off · Senior model…
- Register / login account · Set balance (only when there is no API)
- Usage & credit, in the same popup style as `show_usage_history`

### Docs
- Update `agentic/docs/usage.md:331` and `design.md:75`.
- Save the spec and plan under `specs/plan/2026-11-17-…_opencode-workers/` with status kept current.

## Tests (TDD)
- **Rust:**
  - `Kind::parse("opencode")` and `opencode()` args (floor build / senior plan).
  - `OpencodeSettings`: missing file means off; the global switch overrides the groups; Cto is never chosen.
  - `worker_candidates` puts opencode first only when enabled.
  - `next_available` skips the account when it has no credit.
  - The opencode failure-code mapping.
  - `flow.rs`: a floor worker launches through opencode with a stub `devws`, and falls back to codex when credit is 0.
- **Shell:**
  - `scripts/test_opencode_usage.sh`, which runs the opencode usage reader against fixture message files.
  - Additions to the register_agent tests for the new kind.

## Verification
1. `make check` (fmt, clippy, cargo test, shell suites).
2. Manual run:
   1. Register the opencode account from the menu and log in.
   2. Turn on global and floor.
   3. Start a team task and check that the floor worker pane runs `opencode --model …` and edits files.
   4. Check that seniors stay on Claude/Codex while the senior switch is off.
   5. Turn global off: new floor workers go back to Codex even though floor is still on.
3. The board's USAGE view and `devws usage` show opencode rows, and credit used/left matches the Zen console.

## Spike findings (17-11-2026, opencode 1.19.6)
- Installed with `npm i -g --prefix ~/.local opencode-ai`. Without login, `opencode models opencode` lists only the free models, such as `opencode/deepseek-v4.1-flash-free`. Paid ids appear after `opencode auth login`.
- `XDG_DATA_HOME=<dir>` moves the data dir (`<dir>/opencode/opencode.db`, `auth.json`), so a registered folder isolates its account.
- `OPENCODE_CONFIG_CONTENT` injects config (plugins, permission) without touching the user's own opencode config.
- A plugin's `event` hook receives `session.created`, `session.status {status:{type:busy|idle}}`, `session.idle`, `permission.asked` and `session.error`. These map onto agent_state.sh: `SessionStart`, `UserPromptSubmit`, `Stop`, `PermissionRequest`.
- Usage is in SQLite (`opencode.db`): each assistant row in `message.data` has `cost`, `tokens{input,output,reasoning,cache{read,write}}`, `modelID`, `providerID`, `time.created`, `path.cwd`. `sqlite3` is available. The transcript is recorded as `<db>#<sessionID>`.
- Zen has **no balance API** (docs and third-party reports agree). Credit left = balance the user enters − cost since it was entered, shown as an estimate. A balance error at runtime still blocks the account (`Account` scope).

## Execution notes
- **Shipped:**
  - `Kind::Opencode`
  - `agentic/src/opencode.rs` (settings and switches)
  - `devagentics opencode show|switch|model|account`
  - opencode first in worker selection, skipped when switched off or out of credit
  - the leader refuses opencode
  - `Devws::credit` and `devws agent credit`
  - board credit display
  - `opencode_usage.sh`, `opencode_credit.sh`, `opencode_menu.sh` (◆ OpenCode in the Agent tab)
  - `opencode_devws_plugin.js`
  - runner, interrupt and register support
- **Changes from the plan:**
  - Model listing lives in the menu (`opencode models` on the account), not in a `devagentics opencode models` command.
  - No `flow.rs` test. Selection is covered by unit tests in `worker.rs`, and the real launch was checked end to end: an isolated tmux server, the free DeepSeek Flash model, an edit, the `done` state, the conversation recorded, usage read back, `devws agent send`, interrupt, stop.
  - opencode needs Escape **twice** to interrupt; `agent_cli.sh` sends both presses.
  - `header_tokens.sh costs` gained a GNU `date` fallback for the month start (it was macOS-only).
- **Rebased onto `origin/main` (afd6cc8, PR #26)** after the old local `main` caused frozen workspaces on Linux. Merged with main's account switches (`disabled_accounts`), usage-limit approval and `prefer_codex`. The opencode checks sit after the account switch. A reviewed task the user put on Codex keeps Codex first.
- **Reviewed tasks:** `reviewed_adapter` (team.rs) refuses opencode workers (Model scope, so they fall back). opencode works in legacy tasks. A reviewed-task adapter for opencode would be a separate change.
- **Pre-existing failures on origin/main:** `test_workspace_template.sh` (restore), and `test_guide.sh` for `DEVWS_PROC` and `DEVWS_SYS`.
