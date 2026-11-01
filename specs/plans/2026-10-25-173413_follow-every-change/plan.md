# Plan: Follow every change the agent makes, and mark its lines

- **Date:** 25-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Executed (not committed)
- **Spec:** [spec.md](spec.md)

## Context

Implements [spec.md](spec.md). The explorer-focus bug is already fixed and
tested. Tests first in each step.

## Step 1 — `agent_changes.sh`: snapshot and diff a git project

**Files:** `config/tmuxinator/agent_changes.sh` (new), `scripts/test_agent_follow.sh`

**Actions:**
- `snapshot <dir> <name>`: in the repo containing `<dir>`, copy `.git/index`
  to devws's temp folder (`$TMPDIR/devws-changes-$(id -u)/<name>.index`),
  `GIT_INDEX_FILE=… git add -A`, `git write-tree` → saved as `<name>.before`.
- `changes <dir> <name>`: same for "after", then `git diff -U0 --no-color
  <before> <after>` parsed into `file<TAB>first<TAB>last` lines (absolute
  paths; added files: all lines; deletions: the line where they were).
  Clears `<name>`'s files. Prints nothing outside a repo or without a snapshot.
- Tests: a sed edit, a new file and a multi-file change are reported with
  their exact ranges; the repo's own index and `git status` are unchanged.

**Verification:** `./scripts/test_agent_follow.sh`.

## Step 2 — Neovim: `mark()` and `clear_marks()`

**Files:** `config/nvim/lua/devws/follow.lua`, `scripts/test_agent_follow.sh`

**Actions:**
- `mark(ranges)`: a sign (`▍`, its own highlight group linked to `DiffChange`)
  on each line of each range, by extmarks in the follow namespace's own
  marks namespace; files not loaded yet keep their ranges and get marked on
  `BufReadPost`. Reloads (`checktime`) keep them: they are re-applied after
  the buffer reloads.
- `clear_marks()`: removes all of them.
- Tests: marks land on the given lines, survive a reload, appear on a file
  opened later, and clear.

## Step 3 — The hook's phases

**Files:** `config/tmuxinator/agent_follow_hook.sh`, `config/tmuxinator/editor_open.sh`, `scripts/test_agent_follow.sh`

**Actions:**
- `agent_follow_hook.sh <agent> pre`: snapshot named after the agent pane.
- `agent_follow_hook.sh <agent>` (post, as today): if `agent_changes.sh
  changes` reports ranges, mark them all and show the last file at its first
  range (through `editor_open.sh --follow`, which passes the ranges); else
  today's payload-based path (reads, non-git projects).
- `agent_follow_hook.sh <agent> prompt`: `editor_open.sh --clear-marks`.
- Tests: a Claude Bash payload after a `sed -i` is followed to the changed
  line and marked; a Codex `exec` payload likewise; a command changing three
  files marks all three and shows the last; a prompt clears; outside git the
  old path still works; follow `off` neither shows nor marks.

## Step 4 — Wire the hooks

**Files:** `config/tmuxinator/agent_settings.json`, `config/tmuxinator/agent_runner.sh`, `scripts/test_agent_status.sh` (Codex args check)

**Actions:**
- Claude: PreToolUse `Bash|Edit|MultiEdit|Write|NotebookEdit` → `pre`;
  PostToolUse matcher gains `Bash`; UserPromptSubmit → `prompt`.
- Codex: PreToolUse (all) → `pre`; PostToolUse (all) → post;
  UserPromptSubmit → `prompt`. The state hooks stay as they are.
- Tests: the settings and Codex's arguments carry the new hooks.

## Step 5 — Docs and timing

- README follow-mode section: shell edits and the marks.
- Time a hooked call on this repo (pre + post) and note it; it must stay
  well under a second.

## Final Verification

1. `make check` passes, run alone.
2. The scenario loop (`follow_loop.sh`) is all green.
3. By hand: the agent runs a `sed -i` on a file; LazyVim jumps there and the
   changed lines carry the mark until the next prompt.

## Execution Notes

- Steps 1–5 done; `make check` passes (17 suites).
- Neovim names buffers by their real path (macOS `/var` is `/private/var`):
  marks are keyed by `resolve()`d, normalized paths.
- "The file changed last" is the newest by modification time (`stat -f %Fm`),
  since git lists changed files alphabetically.
- Codex's follow hooks run on every tool (no matcher): its tools are `exec`,
  `shell`, `apply_patch` depending on the version.
- Timing: snapshot + diff on this repo (151 files) 0.12–0.18 s per tool call.
- Step 4 needed auto mode off: the classifier gives no verdict on edits to
  agent hook configuration.
- Not done: shell edits outside a git project (out of scope by the spec).
