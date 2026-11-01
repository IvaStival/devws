# Spec: Follow every change the agent makes, and mark its lines

- **Date:** 25-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Executed (not committed)

## Context

Follow mode sometimes failed to show the agent's changes in LazyVim. Diagnosed
with a scenario loop against a real Neovim (scratch harness `follow_loop.sh`):

1. **Fixed already** (`config/nvim/lua/devws/follow.lua`, regression test
   `test_a_change_is_shown_from_the_explorer`): while LazyVim's explorer had
   the focus — as right after the editor runner's `nvim .` — the file opened
   at line 1 in the dashboard's window, with the cursor, the flash and the
   focus left in the explorer.
2. **This spec:** a change made through a shell command (`sed -i`, a heredoc,
   a formatter, `git checkout`; Codex's `exec` / `shell`) never reaches the
   follow hook, which only reads Edit/Write/apply_patch payloads, so it is
   never shown.

The user also wants every line the agent changed marked, not only the place
the editor jumps to.

## Requirements

1. After each agent tool call that changed project files — whatever the tool
   (Claude: Bash, Edit, MultiEdit, Write, NotebookEdit; Codex: any tool) — the
   editor shows the last changed file at its first changed line, as follow
   mode does today (follow / focus / notify / off modes unchanged).
2. Every line the agent changed gets a mark in the sign column of its file, in
   every file it changed (also files not on screen: marked when opened).
3. The marks stay until the user's next prompt to that agent, then clear.
4. What changed is worked out from the project's own state, before and after
   the tool call, not from the tool's description: so shell commands count.
5. Unchanged: reads are followed only in "explore" mode; the user typing or
   holding unsaved changes in the file only gets a notification; hooks never
   fail or slow the agent noticeably.

## Constraints

- Git projects: snapshots go into a temporary index of devws's own
  (`GIT_INDEX_FILE`, seeded from a copy of the repo's index), so the user's
  index, staging and history are never touched; `.gitignore`d files are left
  out. Only loose objects are written to `.git`, which git collects itself.
- Outside a git repository: today's behavior (Edit/Write/apply_patch payloads
  shown at their line); shell edits there are out of scope.
- Snapshots are per agent pane, so a workspace's extra agents don't mix up
  their calls; two agents changing the same files at the same moment may see
  each other's lines (accepted).
- No new settings.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/agent_follow_hook.sh` | Modify — `pre` (snapshot) and `post` (diff, show, mark) phases; clear on a new prompt |
| `config/tmuxinator/agent_changes.sh` | Create — git snapshot and diff: `snapshot <dir> <name>`, `changes <dir> <name>` printing `file<TAB>first<TAB>last` per changed range |
| `config/tmuxinator/editor_open.sh` | Modify — pass the changed ranges to Neovim; a `--clear-marks` request |
| `config/nvim/lua/devws/follow.lua` | Modify — `mark(ranges)` (signs, also for files opened later), `clear_marks()` |
| `config/tmuxinator/agent_settings.json` | Modify — Claude PreToolUse/PostToolUse follow hooks for Bash and the edit tools; UserPromptSubmit clears |
| `config/tmuxinator/agent_runner.sh` | Modify — Codex PreToolUse/PostToolUse follow hooks on every tool; UserPromptSubmit clears |
| `scripts/test_agent_follow.sh` | Modify — shell edits followed and marked, several files, marks cleared on prompt, non-git fallback |
| `README.md` | Modify — follow mode section |

## Success Criteria

- [ ] The scenario loop's "Claude Bash edit" goes green; a Codex `exec`
      edit is followed too.
- [ ] A command changing three files marks the changed lines of all three and
      shows the last one at its first change.
- [ ] Marks clear on the next prompt; reads never mark.
- [ ] The user's git index is byte-for-byte unchanged after a hooked call.
- [ ] `make check` passes.
