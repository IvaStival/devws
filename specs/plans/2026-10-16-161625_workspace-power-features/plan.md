# Plan: devws workspace power features

- **Date:** 16-10-2026
- **Branch:** one per feature (below), each off `main` after `feat/agent-registry` is merged
- **Status:** Pending

## Context

Implements [spec.md](spec.md): F1 jump to agent, F2 limit warnings, F6 custom layouts, F5 server
pane, F4 review changes, F3 worktree workspaces, in that order. Each feature is its own branch
and PR and ends with tests, README and `scripts/check.sh`. Tests follow the existing scripts:
a private tmux server (`tmux -L`), a temporary `HOME`, stubs in `$TEST_ROOT/bin`, and
`DEVWS_ENV_FILE` for `.env`.

Throughout: anything that deletes asks first with "keep" as the default (spec constraint).

---

## Step 0 — Merge the registry

**Actions:**
- Open the PR for `feat/agent-registry`, merge it, branch F1 from the updated `main`.

**Verification:**
- `main` contains `agents.sh` and `register_agent.sh`.

---

## F1 — Jump to the agent that needs you (`feat/jump-to-agent`)

### Step 1.1 — Record when a state started

**Files:** `config/tmuxinator/agent_status.sh`

**Actions:**
- When the state changes, set `@devws_agent_state_since` to `date +%s`; unset it on `idle`.
- Unchanged state (repeated `working` events) leaves the timestamp alone.

**Verification:**
- `scripts/test_agent_status.sh`: new cases for the timestamp being set, kept, and unset.

### Step 1.2 — Notifications

**Files:** `config/tmuxinator/agent_status.sh`, `config/tmuxinator/.env`

**Actions:**
- On a change to `waiting` or `done`, unless `DEVWS_NOTIFY=0` (or `=waiting` and the state is
  `done`): skip when the session is the active session of an attached client; otherwise run
  `osascript -e 'display notification …'` in the background, titled with the workspace name
  (without `dev-`), body "<agent> needs you" / "<agent> is done".
- `DEVWS_NOTIFY_COMMAND` replaces `osascript` for tests (same pattern as `DEVWS_CLOCK_TIME`).
- Document `DEVWS_NOTIFY` in `.env`.

**Verification:**
- Tests with a stub notifier: notified when not on screen, silent when on screen, silent with
  `DEVWS_NOTIFY=0`, `waiting` only with `DEVWS_NOTIFY=waiting`.
- By hand: an agent in another workspace asking for permission shows a macOS notification.

### Step 1.3 — `jump_agent.sh`

**Files:** `config/tmuxinator/jump_agent.sh` (new)

**Actions:**
- `jump_agent.sh [client]`: list `dev-*` sessions with
  `#{@devws_agent_state}|#{@devws_agent_state_since}`; candidates are `waiting` sorted by
  oldest `since`, else `done` the same way.
- Cycling: start after the client's current session in that order, wrapping round.
- `switch-client -c <client> -t <session>`, then `select-pane` on the `agent` role pane (via
  `workspace_panes.sh`), restoring the agent group first when it is minimized
  (`minimize_panes.sh restore agents`).
- None: `display-message "No agent needs you"`.

**Verification:**
- New `scripts/test_jump_agent.sh`: three sessions (waiting old, waiting new, done) → order is
  old, new, then wraps; only done → goes to done; none → message; minimized agent is restored.

### Step 1.4 — Key and menu

**Files:** `config/tmux/tmux.conf`, `config/tmuxinator/devws_menu.sh`,
`config/tmuxinator/workspace_actions.sh`, `README.md`, `scripts/check.sh`

**Actions:**
- `bind a run-shell -b "~/.tmuxinator/jump_agent.sh '#{client_name}'"` (`!` and `D` are taken:
  break-pane and tmux.conf's detach; `a`, `G` and `S` are free).
- Menu: `agent|jump|! Go to waiting agent|C-b a|jump_agent`.
- README: the shortcut table, the Agent tab, and a "Notifications" paragraph under Agent status.
- Add the new test to `check.sh`.

**Verification:**
- `scripts/test_devws_menu.sh` lists the new item; `scripts/check.sh`.

---

## F2 — Limit warnings and account switch (`feat/limit-warnings`)

### Step 2.1 — Stress color at the threshold

**Files:** `config/tmuxinator/header_agents.sh`

**Actions:**
- `limit()` colors the percentage and bar with `DEVWS_COLOR_STRESS` when used ≥
  `DEVWS_LIMIT_WARN` (default 90), and keeps its current colors below.
- Export `DEVWS_LIMIT_WARN` from `header_ticker.sh`'s `load_settings`.

**Verification:**
- `scripts/test_header.sh`: 89% normal, 90% stress, threshold read from the env file.

### Step 2.2 — Warn once per crossing

**Files:** `config/tmuxinator/header_ticker.sh`

**Actions:**
- After `update_tokens` refreshes the agents, for each agent and window (5h, wk): above the
  threshold and not yet warned → `display-message -d 5000` on every attached client:
  `<agent>: 5h at 92% — C-b Space › Agent › Switch account (starts a new conversation)`; record
  `@devws_limit_warned_<agent>_<window>`. Below the threshold → clear it, so the next crossing
  warns again.
- The limits come from the same sources as the header (tmux option for Claude,
  `header_tokens.sh limits` for Codex); factor `limits_of` out of `header_agents.sh` into
  `agents.sh` rather than copy it.

**Verification:**
- Tests: crossing shows one message; staying above shows none; dropping and crossing again
  shows another.

### Step 2.3 — Switch account

**Files:** `config/tmuxinator/workspace_actions.sh`, `config/tmuxinator/devws_menu.sh`

**Actions:**
- `switch_account()`: kind of the workspace's agent (`agent_kind`); list registered agents of
  that kind except the current one, each as `<name>  5h 42%  wk 63%`, sorted by the larger of
  the two ascending (unknown sorts last); fzf; restart through the existing `switch_agent`
  path with the chosen name.
- With no other account of that kind: message pointing to **Register agent**.
- Menu: `agent|switch-account|⇄ Switch account||switch_account`.

**Verification:**
- `scripts/test_devws_menu.sh`: item present; a function test with stubbed limits checks the
  order and that the agent pane is respawned with the chosen name.
- README: limits paragraph in The header and the Agent tab row.

---

## F6 — Custom layouts (`feat/custom-layouts`)

### Step 6.1 — `layouts.sh`

**Files:** `config/tmuxinator/layouts.sh` (new)

**Actions:**
- Mirrors `agents.sh`: `layout_valid_name`, `layout_slots <name>` (prints the two slots, one
  per line; `default` → `agent`, `terminal`), `layout_slot_valid` (`agent`, `terminal`,
  `run:<non-empty>`), `layout_names` (`default` plus `DEVWS_LAYOUTS`, deduplicated).
- Slots are split on ` | `; exactly two, else invalid.

**Verification:**
- New `scripts/test_layouts.sh`: default, a valid preset, wrong slot count, bad names.

### Step 6.2 — Run pane

**Files:** `config/tmuxinator/run_pane.sh` (new)

**Actions:**
- `run_pane.sh <command>`: sets `@devws_role run` and `@devws_run_command`, runs the command
  with `bash -lc` in the pane's directory, and on exit prints `■ exited <status> — Enter to
  restart, q to close`; Enter loops, `q` exits the pane.
- Records `@devws_run_status` (`running` / exit code) as a pane option for the status line.

**Verification:**
- Test in a private tmux server: role and command set; `false` leaves status `1`; Enter
  restarts it.

### Step 6.3 — Slot runner and dev.yml

**Files:** `config/tmuxinator/slot_runner.sh` (new), `config/tmuxinator/dev.yml`

**Actions:**
- `slot_runner.sh <layout> <1|2> <agent>`: reads `.env` and `layouts.sh`, then `exec`s
  `agent_runner.sh <agent>`, the terminal setup (`@devws_role terminal`, then `exec $SHELL`),
  or `run_pane.sh <command>`. Unknown layout → falls back to `default` with a message.
- `dev.yml`: the agent and terminal panes run `slot_runner.sh <layout> 1|2 <agent>`;
  `pre_window` sets `@devws_layout`. The default layout produces the exact panes of today.

**Verification:**
- `scripts/test_workspace_layout.sh`: default layout gives agent + terminal roles; a `logs`
  layout gives run + terminal.

### Step 6.4 — Run panes in the pane helpers

**Files:** `config/tmuxinator/workspace_panes.sh`, `toggle_layout.sh`, `minimize_panes.sh`,
`config/tmux/tmux.conf`

**Actions:**
- `agent_slot_pane`: after agent, take a `run` pane in slot 1 before the terminal. The follower
  is whatever holds slot 2. Record the slot on the pane (`@devws_slot 1|2`) in
  `slot_runner.sh` so the helpers don't have to guess.
- `minimize_panes.sh`: the agents group includes run panes in slots; the placeholder label is
  `Run`.
- `pane-border-format`: `▾ ▶ <command>` for `run`, and `▸ Run` minimized; header click
  minimizes the agents group as it does for the agent.

**Verification:**
- Existing layout/minimize tests still pass; new cases toggle and minimize a `logs` workspace.

### Step 6.5 — Save and restore

**Files:** `config/tmuxinator/save_state.sh`, `config/tmuxinator/restore_state.sh`

**Actions:**
- `W` line gains the layout (bump the state version to 2; read version 1 as `default`).
- `restore_state.sh` passes `layout=` to tmuxinator. Slot panes come back from the layout, so
  no per-pane command is needed for them.

**Verification:**
- `scripts/test_workspace_state.sh`: a `logs` workspace saves and restores as `logs`; an old
  version-1 file still restores.

### Step 6.6 — Choosing and managing layouts

**Files:** `config/tmuxinator/register_layout.sh` (new), `workspace_actions.sh`,
`devws_menu.sh`, `shell/devws.zsh`, `config/tmuxinator/.env`

**Actions:**
- `register_layout.sh add <name> <slot1> <slot2>` / `remove <name>`: same `.env` rewriting as
  `register_agent.sh` (resolve symlink, awk rewrite, swap in). Refuse `default`.
- `new_layout()`: name (validated, not taken), then for each slot fzf over agent / terminal /
  command…, then a typed command for `command…`. Shows the result and asks to save.
- `remove_layout()`: fzf over layouts except default, shows the two slots, asks
  `Remove layout "logs"? [y/N]` (default keep).
- `new_workspace()`: after the editor, `choose_layout` only when `layout_names` has more than
  one; passes `layout=`.
- `devws <path> [agent] [editor] [layout]`, validated against `layout_names`.
- Menu: `workspace|new-layout|＋ New layout||new_layout`, `workspace|remove-layout|− Remove
  layout||remove_layout`. Document `DEVWS_LAYOUTS` in `.env`.

**Verification:**
- New `scripts/test_register_layout.sh`: add, add duplicate, remove, remove default refused,
  `.env` symlink kept.
- Menu test lists the items; remove with Enter keeps the layout.
- README: a "Layouts" section with the logs example.

---

## F5 — Server pane (`feat/server-pane`)

### Step 5.1 — Project server command

**Files:** `config/tmuxinator/workspace_actions.sh`

**Actions:**
- Remember the command per project in `~/.devws-state/projects/<hash of root>/server`, the
  way the database folder is stored (reuse that helper).
- Suggestion: `composer.json` with a `dev` script → `composer run dev`; `artisan` → `php
  artisan serve`; `package.json` with `dev` → `npm run dev` (`pnpm`/`yarn` by lockfile);
  `Makefile` with `run:` → `make run`.
- `server_command()` (menu **▶ Server command**): asks, prefilled with the remembered or the
  suggested command.

**Verification:**
- Tests per project kind for the suggestion; remembered command wins.

### Step 5.2 — Open or focus the server

**Files:** `config/tmuxinator/open_run_pane.sh` (new), `config/tmux/tmux.conf`,
`devws_menu.sh`

**Actions:**
- `open_run_pane.sh <session> server`: focus an open run pane named `server`
  (`@devws_run_name server`); else ask for the command when none is remembered, then split
  where the database pane goes (same placement rules as `open_database.sh`) and run
  `run_pane.sh`.
- `bind S`, menu `workspace|server|▶ Server|C-b S|open_server`, `workspace|server-command|▶
  Server command||server_command`.

**Verification:**
- Test: opens once, focuses on the second call, placement in both arrangements.

### Step 5.3 — Status on tmux's line

**Files:** `config/tmuxinator/header_ticker.sh`, `config/tmux/tmux.conf`

**Actions:**
- Every 5 s with the system sources: for each session's run panes, `running` → find a listening
  port among the pane's process tree (`pgrep -P` recursively, `lsof -nP -iTCP -sTCP:LISTEN -a
  -p`), write `@devws_run_summary` (`▶ server :8000`, `■ server exit 1`).
- `wg_runs` on tmux's own line, next to the conversation.

**Verification:**
- Test with a stub `lsof`: running with a port, running without, exited.

### Step 5.4 — Save and restore

**Files:** `save_state.sh`, `restore_state.sh`, `toggle_layout.sh`, `minimize_panes.sh`

**Actions:**
- A `P` line for a `run` pane outside the slots stores its name and command; restore reopens it
  through `open_run_pane.sh --no-focus`, like the database pane.
- The server pane follows the database pane's rules in toggle and minimize (its own group).

**Verification:**
- State test: a workspace with a server pane restores it with its command. README section.

---

## F4 — Review the agent's changes (`feat/review-changes`)

### Step 4.1 — Turn snapshot

**Files:** `config/tmuxinator/turn_snapshot.sh` (new), `config/tmuxinator/agent_state.sh`

**Actions:**
- `turn_snapshot.sh <session> <dir>`: inside a git work tree, with a temporary
  `GIT_INDEX_FILE` seeded from the real index: `git add -A`, `git write-tree`,
  `git commit-tree -p HEAD -m "devws turn"` (no parent on an unborn branch); store the hash in
  `@devws_turn_base`. Nothing else is written; the temporary index is removed.
- `agent_state.sh` starts it in the background on `UserPromptSubmit`, from the hook's `cwd`.

**Verification:**
- New `scripts/test_review.sh` in a temp repo: after the snapshot, `git status --porcelain`,
  `.git/index`, `git stash list` and `git for-each-ref` are unchanged; the snapshot tree contains
  an untracked file; `git diff <base>` shows a later edit only.

### Step 4.2 — diffview in Neovim

**Files:** `config/nvim/lua/plugins/diffview.lua` (new), `config/nvim/lua/devws/review.lua`
(new), `config/nvim/lazy-lock.json`

**Actions:**
- Add `sindrets/diffview.nvim` (lazy-loaded on its commands).
- `require'devws.review'.open(base)`: `DiffviewOpen <base>` (or plain `DiffviewOpen` for HEAD)
  in a new tab; returns `"opened"`.

**Verification:**
- Headless Neovim test (`nvim --headless`) loads the module and opens a view on a temp repo.

### Step 4.3 — Action and key

**Files:** `workspace_actions.sh`, `devws_menu.sh`, `config/tmux/tmux.conf`

**Actions:**
- `review_changes()`: not a git repo → message; editor not Neovim / no `@devws_editor_socket` →
  message; fzf **Since the agent's last prompt** (hidden when no `@devws_turn_base`) / **All
  uncommitted**; call `review.open` through `nvim --server … --remote-expr`, as
  `editor_open.sh` does; focus the editor pane.
- Menu in Agent and Editor tabs (`C-b G`); `bind G`.

**Verification:**
- Menu test; by hand: prompt the agent to edit two files, review shows exactly those two.
  README section.

---

## F3 — Worktree workspaces (`feat/worktree-workspaces`)

### Step 3.1 — `worktree.sh`

**Files:** `config/tmuxinator/worktree.sh` (new)

**Actions:**
- `worktree.sh path <repo> <branch>` → `<parent of repo>/<repo name>-<branch with / → ->`.
- `worktree.sh ensure <repo> <branch>`: an existing worktree for the branch (`git worktree list
  --porcelain`) → its path; an existing local branch → `git worktree add <path> <branch>`; a new
  name → `git worktree add -b <branch> <path> HEAD`. Refuses when the path exists and is not
  that worktree.
- `worktree.sh status <path>`: `clean`, or the `git status --short` lines.
- `worktree.sh remove <path>`: `git worktree remove <path>` (never `--force`); never touches the
  branch.

**Verification:**
- New `scripts/test_worktree.sh` in temp repos: path naming, create new, reuse existing,
  existing branch, refuse a foreign folder, remove clean, git refuses dirty, branch still
  exists after remove.

### Step 3.2 — Opening one

**Files:** `workspace_actions.sh`, `devws_menu.sh`, `shell/devws.zsh`

**Actions:**
- `new_worktree_workspace()`: needs the current workspace's root in a git repo; fzf over local
  branches plus a "new branch…" entry (typed name, validated with `git check-ref-format
  --branch`); then agent, editor, layout as New workspace; `worktree.sh ensure`; tmuxinator
  start; mark the session `@devws_worktree 1`.
- `devws worktree <branch> [agent] [editor] [layout]` from the current directory's repo.
- Menu: `workspace|worktree|⑂ New worktree workspace||new_worktree_workspace`.

**Verification:**
- Menu test; by hand: a new branch opens in `../devws-<branch>` with its own agent.

### Step 3.3 — Closing one

**Files:** `workspace_actions.sh`

**Actions:**
- `close_workspace()`: after closing a worktree workspace, `worktree.sh status`:
  - dirty → print "Kept <path>: uncommitted changes" and the list; no question.
  - clean → `Remove the worktree folder <path> (branch <branch> is kept)? [y/N]`; only `y`
    removes; Enter/Esc keep.
- Asked before the workspace closes (the pane that asks may be in it); the folder is removed
  after `kill-session`. A worktree workspace is recognised by its folder (`worktree.sh linked`),
  so save/restore needs no change.

**Verification:**
- Tests: clean + Enter keeps, clean + `y` removes and the branch remains, dirty never asks.
  README section.

---

## Final Verification

- [ ] Every feature branch: `scripts/check.sh` passes apart from the known
      `test_workspace_minimize.sh` failure.
- [ ] By hand, in a real devws: each spec success criterion, one feature at a time.
- [ ] README updated per feature; spec and plan status set to `Executed` at the end.
