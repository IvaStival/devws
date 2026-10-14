# Spec: Link the agent with the editor

- **Date:** 14-10-2026
- **Branch:** feat/agent-editor-link
- **Status:** Executed

## Context

The agent (Claude Code or Codex) and Neovim sit side by side in a devws workspace but know
nothing of each other: the editor only reloads changed files. The user wants to follow the
agent's changes in the editor, and to have the agent open files in the editor when asked.

## Requirements

1. When the agent changes a file, the editor shows it at the changed line, without taking the
   tmux focus from the agent pane.
2. The agent can open a file (optionally at a line) in the editor on request.
3. Following is on by default; `C-b F` and a menu entry turn it off and on; the agent pane
   header shows when it is on.
4. Works for Claude Code and Codex, without changing the user's own agent settings.
5. Never interrupts the user's own typing: in insert/command mode or with unsaved changes the
   editor only notifies.

## Constraints

- Hooks never fail the agent: no editor, no socket or following off → exit 0 silently.
- Only Neovim gets the link; other editors keep working as today.

## Success Criteria

- [x] An agent edit moves the editor to the change (tested with real headless Neovim).
- [x] `editor_open.sh path:line` opens it; `--follow` respects the toggle.
- [x] `C-b F` flips `@devws_follow` and the header.
- [x] `./scripts/check.sh` passes.
