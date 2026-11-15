# Plan: Remote team panel and clipboard

- **Date:** 13-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed

## Context

Implements [spec.md](spec.md). agentic.sh already adds ~/.cargo/bin to its own PATH, but passes the bare executable name into a new pane. That pane can have a different PATH. Resolve the executable before crossing that boundary rather than editing the user's Bash profile.

The clipboard helper writes a tmux-wrapped escape sequence to SSH_TTY, which can be stale after reattachment and may lead directly to a terminal rather than another tmux. Prefer tmux's native clipboard forwarding to the attached client instead of enabling arbitrary passthrough globally.

## Step 1 — Make board executable lookup independent of pane PATH

**Files:** config/tmuxinator/agentic.sh, config/tmuxinator/agentic.yml, scripts/test_agentic.sh, scripts/check.sh (template expectations if needed).

**Actions:**
- Resolve the installed or overridden agentic executable to an absolute path after dependency checks.
- Pass that path into the board template with shell quoting that preserves spaces and apostrophes.
- Add regression coverage where the launcher can find the executable but the board shell cannot find its bare name; cover a path containing shell-sensitive characters.

**Verification:** Run scripts/test_agentic.sh and template validation.

## Step 2 — Forward remote clipboard data through the attached tmux client

**Files:** config/tmux/yank.sh, config/tmux/tmux.conf, scripts/test_clipboard.sh (new).

**Actions:**
- Preserve existing native clipboard backends and the explicit fallback-off setting.
- Replace manual SSH_TTY writes and tmux escape wrapping with native tmux clipboard forwarding (load-buffer -w).
- Configure clipboard forwarding explicitly and verify terminal capability handling for supported clients.
- Add behavioral checks for exact payload bytes, whitespace/newlines, fallback disabled, local backend priority, and copying after reattachment without depending on an inherited SSH_TTY.
- Verify forwarding in an isolated tmux server with a controlled terminal client where feasible.

**Verification:** Run the new clipboard suite and Bash syntax checks; inspect emitted OSC 52 data through a controlled client. A real terminal clipboard paste remains a manual server check.

## Step 3 — Validate and provide pull instructions

**Actions:**
- Run relevant suites and scripts/check.sh --quick (includes repository syntax checks); report failures caused by pre-existing work separately.
- Review the scoped diff and mark spec/plan executed when complete.
- Provide pull/reload commands and a safe way to reopen the failed board without restarting active agents.
- Do not commit unless requested; warn that a server pull requires these local changes to be committed and pushed first.

**Verification:** Only intended files changed by this task; relevant tests pass; recovery commands correspond to actual project behavior.


## Verification Results

- Passed: scripts/test_agentic.sh (real isolated tmux server, including quoted executable paths and Cargo-only lookup).
- Passed: scripts/test_clipboard.sh (payload bytes, trailing newlines, stale SSH_TTY, disabled fallback, local backend priority).
- Passed: scripts/test_rust_tools.sh, Bash syntax checks, ERB/YAML template validation, scoped git diff --check.
- Passed: controlled PTY/tmux client verification emitted the expected OSC 52 payload before and after client reattachment. This checks terminal output, not the user's actual desktop clipboard permissions.
- Blocked: scripts/check.sh --quick stops at pre-existing untracked scripts/test_usage_history.sh, which lacks its executable bit. That unrelated file was left untouched.
- No commit or push performed.

## Applying on the Server

After committing and pushing these changes, run in the server's devws checkout:

```bash
git pull
devws reload
```

Existing failed Team panes still contain the old command. In the failed Team pane's shell, start its board without restarting the agents:

```bash
"$HOME/.cargo/bin/devagentics" board "$(tmux show-option -qv @devws_task)"
```

New teams use the resolved executable automatically. If SSH is run inside a local tmux, reload the local tmux configuration too so the outer tmux accepts clipboard forwarding. Verify by dragging a selection and pasting into a local application; the terminal must allow OSC 52 clipboard writes.
