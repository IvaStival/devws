# Plan: Workspace state persistence and a working Shift+Enter

- **Date:** 28-09-2026
- **Branch:** main
- **Status:** Executed

## Context

Implements `specs/plans/2026-09-28-101208_workspace-persistence-shift-enter/spec.md`.
Two independent halves: a devws-owned state file that replays
`tmuxinator start dev …` to rebuild the open workspaces after a reboot or a
crash, and an iTerm2 key mapping that finally makes Shift+Enter distinguishable
from Enter.

## Step 1 — State file and snapshot script

**Files:**
- `config/tmuxinator/save_state.sh` (new, executable — picked up automatically
  by `install.sh`'s `config/tmuxinator/*` symlink loop and by
  `scripts/check.sh`'s `config/tmuxinator/*.sh` syntax loop)

**Format** — `~/.devws-state/sessions.tsv`, line-based TSV, chosen over JSON
because `jq` is not a dependency and both existing files in `~/.devws-state/`
are already line-based. A `W` record per workspace, followed by its `P`
records in pane order:

```
#devws-state	1
W	<session>	<root>	<agent>	<editor>	<picker_open>	<layout>
P	<session>	<pane_index>	<role>	<cwd>
```

TAB rather than `|`, because a captured `#{window_layout}` contains `,`, `{`,
`}` and `x` but never a TAB. The W-then-P grouping lets restore work in one
pass with plain variables — no associative arrays, since
`#!/usr/bin/env bash` may resolve to macOS bash 3.2.

**Actions:**
- Exit 0 immediately when `~/.devws-state/restore.lock` exists (a restore
  replays tmuxinator, firing the same hooks that call this script) or when no
  tmux server is running.
- Enumerate with
  `tmux list-sessions -F '#{session_name}\t#{@devws_root}\t#{@devws_agent}\t#{@devws_editor}'`,
  keeping `dev-*` sessions with a non-empty root.
- Per session, take the layout from
  `tmux display-message -p -t "$session:" '#{window_layout}'` and the panes
  from `tmux list-panes -t "$session:" -F '#{pane_index}\t#{@devws_role}\t#{pane_title}\t#{pane_current_path}'`.
  Target the session's **active window**, never a window name — the name is not
  guaranteed to survive a non-attached start.
- Derive each pane's role: `@devws_role` first, then `pane_title` for
  `devws-picker`, then `extra`. When no pane claims the terminal role — a
  workspace created before the terminal pane started marking itself — the
  first unclaimed pane takes it, so restoring such a workspace does not add a
  duplicate terminal.
- Write to `mktemp` in the destination directory so the final `mv` is atomic;
  skip the `mv` entirely when `cmp -s` shows nothing changed.

## Step 2 — Save triggers

**Files:**
- `config/tmux/tmux.conf`, `shell/devws.zsh`, `config/tmuxinator/session_picker.sh`

**Actions:**
- Four backgrounded tmux hooks next to the existing `after-new-window` hook:
  `session-created`, `session-closed`, `client-detached`, `pane-exited`, each
  `run-shell -b "~/.tmuxinator/save_state.sh"`.
- Explicit saves where devws already mutates a workspace: after
  `tmuxinator start` and after the `case` in `devws-menu()`
  (`shell/devws.zsh`); in `new_workspace`, `new_terminal`, `switch_agent` and
  `switch_editor` via a local `save_state` helper, and inside
  `close_workspace`'s `tmux run-shell -b` chain (`session_picker.sh`).
- Not folded into `refresh_devws_pickers.sh`: that script is a pure redraw and
  is also called where a save is already queued.

## Step 3 — Make the terminal pane identifiable

**Files:**
- `config/tmuxinator/dev.yml`, `config/tmuxinator/session_picker.sh`

The `terminal` pane was identified by `pane_title == "terminal"`, but the shell
replaces that title as soon as it draws a prompt — so `new_terminal()` never
found it in a live session and silently did nothing. Pre-existing bug,
surfaced because save/restore depends on the same lookup.

**Actions:**
- `dev.yml`: the terminal pane runs
  `tmux set-option -p -t "$TMUX_PANE" @devws_role terminal; clear`, matching
  what `agent_runner.sh` and `editor_runner.sh` already do.
- `session_picker.sh`: extract a `terminal_pane()` helper that matches
  `@devws_role` first and falls back to the title for older workspaces; tag
  panes created by `new_terminal()` with `@devws_role extra` via
  `split-window -P -F '#{pane_id}'`.

## Step 4 — Restore script

**Files:**
- `config/tmuxinator/restore_state.sh` (new, executable)

**Actions:**
- Lock with `mkdir "$HOME/.devws-state/restore.lock"` — atomic, and its
  presence suppresses `save_state.sh`; released from an `EXIT` trap.
- Read the state file on **file descriptor 3** (`exec 3<` + `read -u 3`) and
  give `tmuxinator` `</dev/null`. Sharing stdin lets the generated tmuxinator
  script swallow the rest of the state file.
- Per `W` record: skip when `tmux has-session -t "=$session"` succeeds (the `=`
  forces an exact match, so `dev-app` does not match `dev-app2`); skip with a
  message when the root is gone; validate the agent and editor against
  `DEVWS_AGENTS`/`DEVWS_EDITORS` with a warning on fallback; then
  `tmuxinator start --no-attach dev project_root=… agent=… editor=…`.
- Wait for the workspace to identify itself — a pane with `@devws_role
  terminal` **and** a pane titled `devws-picker`. tmuxinator creates the panes
  at once but their commands run asynchronously, and it can take upwards of ten
  seconds; poll for up to 30.
- Flush each workspace in this order so the pane count matches the layout at
  the moment it is applied: kill the sidebar when it was saved closed, recreate
  the extra terminals with `split-window -c <saved cwd>` (tagging each
  `@devws_role extra`), apply the saved layout with `select-layout` when the
  pane count matches, and `send-keys "cd …"` only when the base terminal was
  not sitting in the workspace root.
- Print the restored session names on stdout; never attach.

## Step 5 — Shell wiring

**Files:**
- `shell/devws.zsh`, `config/tmuxinator/.env`

**Actions:**
- Dispatch `restore`, `save` and `fresh` beside the existing `md` and `menu`
  branches. `devws-restore [--no-attach]` runs the restore script, refreshes
  the sidebars, then `switch-client` inside tmux or `attach` outside it.
  `devws fresh` deletes the state file.
- Auto-restore inside `devws()`, immediately before `tmuxinator start`: only
  when `DEVWS_AUTO_RESTORE` is not 0, no tmux server is running, and the state
  file is non-empty. Nothing is added to the sourced top level of the file —
  it runs in every interactive shell, where a state probe would cost a `tmux`
  fork per shell.
- Document `DEVWS_AUTO_RESTORE` in `.env` next to `DEVWS_AGENTS`.

## Step 6 — iTerm2 key mapping

**Files:**
- `scripts/setup_iterm_keys.sh` (new, executable), `install.sh`, `delete.sh`,
  `Makefile`

**The sequence:** `ESC CR` (hex `0x1b 0x0d`, the bytes Option+Enter already
produces), written as iTerm2 action 11 ("Send Hex Code") under the global key
`0xd-0x20000` (Return plus the Shift modifier flag). Chosen over CSI-u
`\e[13;2u` because CSI-u needs the Kitty keyboard protocol to be negotiated:
an application that has not enabled it inserts the literal text `[13;2u` into
the composer, whereas `ESC CR` degrades to a harmless no-op. tmux also
forwards `M-Enter` untouched regardless of the extended-keys setting. Action
11 and action 10 ("Send Escape Sequence") were read back out of iTerm2's own
stored key maps rather than guessed.

**Actions, each an early exit:**
- Skip silently when not macOS or iTerm2 is absent, keeping Linux installs
  unaffected.
- Refuse while iTerm2 is running: it keeps preferences in memory and rewrites
  the whole file on quit, silently discarding an external write.
- No-op when the mapping is already present.
- **Seed** the merged map from the current `GlobalKeyMap`, or from
  `/Applications/iTerm.app/Contents/Resources/DefaultGlobalKeyMap.plist` when
  the preference is unset — iTerm2 falls back to that bundled map while the
  preference is absent, so a single-entry write would silently drop every
  built-in global binding.
- Back the preferences up into the existing `~/.devws-backups/<ts>/` (a copy,
  not a move: these files are edited in place) and record ownership in
  `~/.devws-state/iterm-keys` so `delete.sh` removes only what devws added.
- Write through `defaults write` so the change goes via `cfprefsd`, and — when
  `LoadPrefsFromCustomFolder` is set — patch the plist in that folder too.
  That file, not the one in `~/Library/Preferences`, is what iTerm2 reads at
  launch, so without this the mapping would not survive a restart.
- `install.sh`: a `--iterm-keys` flag plus an opt-in prompt reusing
  `ensure_fzf()`'s consent pattern, skipped when non-interactive or already
  configured. `Makefile`: an `install-iterm-keys` target.
- `delete.sh`: `remove_iterm_keys()` deletes just that key by the same
  temp-file round trip, and unsets `GlobalKeyMap` entirely when what is left
  equals the bundled default — restoring the exact "preference never set"
  state. Also removes `~/.devws-state/sessions.tsv` and any stale lock before
  the existing `rmdir "$state_root"`, which would otherwise silently fail.

## Step 7 — tmux terminal settings

**Files:**
- `config/tmux/tmux.conf`

**Actions:**
- `extended-keys always` → `on`. `always` forces modifyOtherKeys mode 1 on
  applications that never requested it, changing the byte sequences of other
  modified keys for readline, zsh, LunarVim and the agents — and per `man tmux`
  it cannot affect `Enter` at all, so it never delivered what `b8bdbf7`
  intended.
- `default-terminal` `screen-256color` → `tmux-256color`: a `screen-*` entry
  advertises none of tmux's capabilities, so applications never request
  extended keys. `infocmp tmux-256color` resolves on macOS.
- Add `set -as terminal-features '*:extkeys'` and
  `set -g extended-keys-format csi-u`. Both are inert for `ESC CR` but are the
  prerequisites for the CSI-u fallback, making that a one-constant change.
- `config/tmux/default-keys.conf`, sourced last, does not interfere: it sets
  only `prefix`/`prefix2`, `unbind -a` touches key tables rather than options,
  and `M-Enter` is bound in no table.

## Step 8 — Tests and documentation

**Files:**
- `scripts/test_workspace_state.sh` (new), `scripts/test_iterm_keys.sh` (new),
  `scripts/check.sh`, `README.md`

**Actions:**
- `test_workspace_state.sh`: fake `tmux` and `tmuxinator` on a stripped
  `PATH`. The `tmux` shim parses the `-F` format string and renders exactly the
  requested fields from a per-session pane fixture, so both scripts' different
  format strings are answered correctly. Cases: record shape, the legacy
  terminal-adoption fallback, save idempotence, the restore lock suppressing a
  save, replay order and arguments, extras recreated and tagged, the layout
  applied, an open session skipped, a missing folder reported, the agent
  fallback, and the sidebar killed when it was saved closed.
- `test_iterm_keys.sh`: fake `pgrep` and `defaults`, the latter mirroring what
  it is handed into a fake preferences file the way `cfprefsd` would. Asserts
  the written map holds the bundled bindings **plus one**, the action and text
  are right, ownership is recorded, a second run does nothing, a custom
  preferences folder is patched, and the script refuses while iTerm2 runs.
  Skips itself when iTerm2 is not installed.
- Register both in all three of `scripts/check.sh`'s lists, and
  `setup_iterm_keys.sh` in its syntax and executable-bit lists.
- `README.md`: a "Session persistence" section under Use (what is saved, where,
  when, the four commands, auto-restore semantics, the `DEVWS_AUTO_RESTORE`
  opt-out and three limitations) and a "Shift+Enter in iTerm2" subsection under
  Requirements (the command, the must-quit-iTerm2 requirement, the sequence and
  why, the documented alternates). Configuration and Uninstall lists updated.

## Follow-ups

- `config/tmux/default-keys.conf` is sourced last and wipes every binding
  `tmux.conf` sets above it, resetting the prefix to `C-b` — so the documented
  `C-a` prefix, the `|`/`_` splits and the `C-r` reload are all dead today.
- `shell/devws.zsh` and `session_picker.sh` duplicate the project-root
  resolution idiom; a shared helper would remove it.
