# Plan: Agent alerts that name their workspace and jump there on click

- **Date:** 24-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Executed (awaiting a real click check; not committed)

## Context

Implements [spec.md](spec.md): the needs-you / done alert names the workspace,
its folder and branch, and the agent that changed state, and clicking it takes
you to that agent. Tests first in each step (TDD), as the project asks.

How the click reaches tmux: terminal-notifier runs the click with `/bin/sh`,
outside tmux and with a minimal `PATH`. The command sets `TMUX` to the
server's socket (`#{socket_path}`) — the variable tmux itself uses to find its
server — and puts tmux's own directory first on `PATH`, so `focus_agent.sh`
and everything it calls (`minimize_panes.sh`, `workspace_panes.sh`) talk to
the right server unchanged.

## Step 1 — `focus_agent.sh`: take a client to an agent pane

**Files:**
- `scripts/test_jump_agent.sh` (tests first)
- `config/tmuxinator/focus_agent.sh` (new, executable)

**Actions:**
- Tests: with an attached client and `TMUX` unset, `focus_agent.sh <session> <pane>`
  switches the client to the workspace and focuses the pane; restores a
  minimized agent group first; with a pane that has gone, still switches and
  focuses the main agent; with a workspace that has gone, exits 0 and changes
  nothing; run with a bare `PATH` and only `TMUX=<socket>,0,0` set, it reaches
  the test's server.
- `focus_agent.sh <session> [pane] [client]`: restore the group
  (`minimize_panes.sh … restore agents`), fall back to the main agent when the
  pane is not in the workspace, pick the most recently active client when
  none is given (`#{client_activity}`), `switch-client -c`, `select-window`,
  `select-pane`.

**Verification:**
- `./scripts/test_jump_agent.sh` — the new tests fail before, pass after.

## Step 2 — `jump_agent.sh` uses it

**Files:**
- `config/tmuxinator/jump_agent.sh`

**Actions:**
- Keep choosing the target workspace and the agent in the wanted state;
  replace its restore / switch / select block with a call to `focus_agent.sh`.
  Keep its "Could not switch" message.

**Verification:**
- `./scripts/test_jump_agent.sh` — the existing `C-b a` tests pass unchanged.

## Step 3 — The alert's content

**Files:**
- `scripts/test_agent_status.sh` (tests first)
- `config/tmuxinator/agent_status.sh`

**Actions:**
- The test hook records `title|subtitle|message|session|pane`. Update the
  existing expectations to `work ● needs you|project · <branch>|claude needs you|…`
  and `work ✓ done|…|claude is done|…`; the test project becomes a git repo
  on a known branch.
- New tests: an extra agent's pane (`extra-agent`, its own `@devws_agent`)
  reporting waiting gives `codex in a new panel needs you` with its pane id;
  a workspace whose root is not a repo gives the folder alone as subtitle.
- `notify`: title `<name> ✓ done` / `<name> ● needs you`; subtitle from
  `@devws_root` (basename) and `git -C <root> branch --show-current`; message
  from the reporting pane's role and `@devws_agent` (pane option for an extra
  agent, session option for the main one). `DEVWS_NOTIFY_COMMAND` gets the
  five fields.

**Verification:**
- `./scripts/test_agent_status.sh` — new and updated tests pass; the
  `DEVWS_NOTIFY` and on-screen tests still pass.

## Step 4 — Posting: terminal-notifier, else osascript

**Files:**
- `scripts/test_agent_status.sh` (tests first)
- `config/tmuxinator/agent_status.sh`

**Actions:**
- Tests, with `DEVWS_NOTIFY_COMMAND` unset and stubs on `PATH`: a
  `terminal-notifier` stub receives `-title`, `-subtitle`, `-message`,
  `-group devws-<session>`, `-activate com.googlecode.iterm2` and an `-execute`
  command; running that command with `env -i /bin/sh -c` focuses the agent
  (end to end, through Step 1). Without the stub, an `osascript` stub receives
  the title, subtitle and message as arguments.
- `notify`: build the click command (`TMUX=<socket>,0,0 PATH=<tmux dir>:/usr/bin:/bin
  <focus_agent.sh> <session> <pane>`, each part quoted with `printf %q`); post
  with terminal-notifier when found, else osascript with `subtitle`. Both stay
  in the background.

**Verification:**
- `./scripts/test_agent_status.sh` passes.
- By hand, once: a real alert from a workspace you are not on, clicked, lands
  on its agent.

## Step 5 — Install, checks and docs

**Files:**
- `Brewfile`
- `scripts/check.sh`
- `README.md`

**Actions:**
- `Brewfile`: `brew "terminal-notifier"` (in the mac block, as it is macOS only).
- `check.sh`: add `focus_agent.sh` to the executable list (it is already
  covered by the `config/tmuxinator/*.sh` glob; confirm) and nothing else new.
- README: replace the `devws · my-app — claude needs you` example with the new
  three-line alert, say that clicking it goes to the agent, and that it needs
  terminal-notifier (installed by `./install.sh --deps`), else it is not
  clickable.

**Verification:**
- `brew bundle check --file=Brewfile` lists terminal-notifier as missing or
  present, without a syntax error.

## Final Verification

1. `make check` passes (all suites, lint).
2. `C-b a` still jumps as before.
3. A real done alert from another workspace reads as in the spec and, clicked,
   lands on the agent that finished.

## Execution Notes

- Steps 1–5 done; `make check` passes (17 suites).
- Added `DEVWS_TERMINAL_NOTIFIER` (internal, like `DEVWS_NOTIFY_COMMAND`) so
  tests can stand in for terminal-notifier, including when it is installed.
- The alert names the agent whose own state matches the workspace's new
  state, so a workspace that turns done because another agent went idle names
  the agent that is done.
- Pending: Final Verification 3 (a real alert, clicked), then commit.
