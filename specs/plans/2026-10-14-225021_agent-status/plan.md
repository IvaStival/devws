# Plan: Show the agent's state (working / needs you / done) in devws

- **Date:** 14-10-2026
- **Branch:** feat/agent-status
- **Status:** Executed

## Context

The user can't tell at a glance whether the agent is still working, finished, or is waiting for
them (a permission prompt), especially when the agent pane is minimized or they are in another
workspace. They want an icon/animation for it. Decisions: show it in the agent pane header, the
minimized agent strip, the tmux status bar and the sidebar's workspace rows; no alerts or sounds.

devws already starts the agents with hooks on their command line (`agent_runner.sh`,
`agent_settings.json` for Claude Code, `-c hooks.…` for Codex), so the same mechanism reports the
state without touching the user's settings.

## States and where they come from

| State | Shown as | Claude Code hooks | Codex hooks |
|---|---|---|---|
| working | animated `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` + "working" | UserPromptSubmit, PreToolUse, PostToolUse | UserPromptSubmit, PreToolUse, PostToolUse |
| waiting | `●` "needs you" (bold yellow) | Notification with matcher `permission_prompt\|elicitation_dialog` | PermissionRequest |
| done | `✓` "done" (green) | Stop | Stop |
| idle | nothing | SessionStart, SessionEnd | SessionStart, SessionEnd |

"done" stays until the next prompt. A tool resuming after approval sets working again.

## Approach

1. **`config/tmuxinator/agent_state.sh <claude|codex>`** (new hook script, stdin = hook JSON):
   maps `hook_event_name` (+ Claude's `notification_type`) to a state and calls
   `agent_status.sh`. Always exits 0, never prints.

2. **`config/tmuxinator/agent_status.sh <session> <state>`** (new): stores `@devws_agent_state`
   on the session; when the state changes, refreshes the sidebars
   (`refresh_devws_pickers.sh`, existing). For `working` it starts the **ticker** unless one
   runs (pid in `@devws_agent_ticker`): a background loop that advances `@devws_agent_frame`
   every 0.15 s while the state is still `working` and the session exists, then exits. Setting
   the option redraws headers and the status bar (verified in a real client; if not, the ticker
   also runs `tmux refresh-client -S`).

3. **One badge format, reused** — global option `@devws_agent_badge` in `config/tmux/tmux.conf`
   (frame chosen from `@devws_agent_frame` with `#{e|%:…}`-style lookup, or a 10-way
   conditional), used via `#{E:@devws_agent_badge}` in:
   - the agent part of `pane-border-format` (after `Agent · #{@devws_agent}`, before the follow
     mode);
   - the minimized-agent strip (`▸ Agent` + just the icon, to stay narrow);
   - the session `status-left` set in `config/tmuxinator/dev.yml` (`[name] ⠋`).

4. **Sidebar** (`session_picker.sh` `workspace_rows`): the "agent · editor" line becomes
   "claude ⠋ working · nvim" / "claude ● needs you · nvim" / "claude ✓ done · nvim", read from
   each session's `@devws_agent_state` (static icon; redrawn on state changes only, since the
   sidebar redraws by restarting fzf).

5. **Wiring** (`agent_runner.sh`, `agent_settings.json`): add the state hooks next to the follow
   hook — Claude: UserPromptSubmit, PreToolUse, PostToolUse (all tools), Notification (matcher
   above), Stop, SessionStart, SessionEnd → `agent_state.sh claude`. Codex: the same events via
   `-c hooks.<Event>=[…]` → `agent_state.sh codex` (Codex asks once to review new hooks).
   `restart_agent`/`switch_agent` (`workspace_actions.sh`) reset the state to idle.

6. **Docs**: README section "Agent status" (what each icon means, where it shows); Configuration
   map lists the new scripts.

## Reuse

- `refresh_devws_pickers.sh` (sidebar redraw), `workspace_panes.sh`, the hook wiring and test
  harness of `scripts/test_agent_follow.sh` (isolated tmux server, stub agents, `wait_until`).

## Tests (new `scripts/test_agent_status.sh`, registered in `scripts/check.sh`)

- `agent_state.sh` maps real-shaped Claude and Codex payloads to states (prompt → working,
  permission notification → waiting, idle notification → unchanged, Stop → done, SessionEnd →
  idle); bad input / no tmux → exit 0, silent.
- Ticker: frames advance while working; the ticker exits on done and when the session is killed;
  only one ticker per session.
- The header, minimized strip and status-left formats render each state's icon/text (expanded
  with `display-message -p`).
- Sidebar rows show each workspace's state.
- `agent_runner.sh` gives claude settings with the state hooks and codex the `-c hooks.*`
  arguments; the follow hook is still there.

## Verification

1. `./scripts/check.sh`.
2. Real tmux client (pty + pyte, as for the menu): the header spinner changes frames while the
   state is working and stops on done.
3. Real Claude Code (`claude -p` in an isolated session): working during the run, done after;
   a permission prompt case → needs you.
4. Real Codex with `--dangerously-bypass-hook-trust` in a test run only: working → done.
5. Commit with the commit skill (no AI credits); `./install.sh` to link the new scripts.

## Execution notes (14-10-2026)

- Done: `agent_state.sh`, `agent_status.sh`, the `@devws_agent_icon` / `@devws_agent_badge`
  formats in the header, minimized strip and `status-left`, the sidebar rows, the Codex wiring,
  the idle reset on restart/switch, `scripts/test_agent_status.sh` (in `check.sh`), README.
- `agent_settings.json` has the Claude Code state hooks next to the follow hook.
- Deviations: the ticker stores the frame glyph itself in `@devws_agent_frame` (no lookup in
  the format); it runs under `tmux run-shell -b` keyed by session id, so it outlives the hook
  and survives renames. `agent_state.sh` takes no argument: both agents send `hook_event_name`.
  "needs you" is yellow without bold, since a bold attribute would leak into the rest of an
  inactive pane's header. Session targets use `=name:` (tmux 3.7 rejects `=name` for options).
- Verified: real tmux client redraws the header spinner and stops on done (in the test); real
  Codex (`codex exec --dangerously-bypass-hook-trust`) goes working → done; real Claude Code
  (interactive, in tmux) goes working → needs you (permission prompt) → working → done.
- Known gap: an interrupted Claude turn (Esc) may not send Stop, so "working" can stay until the
  next turn ends.

## Commits

- `2b93906` feat: show the agent's state (working / needs you / done) in devws
- `485f96d` feat: spin the agent icon and the sidebar without redrawing it — follow-up: the
  spinner replaces the header's agent icon (no "working" word); the sidebar spinner turns via
  fzf reloads over its `--listen` socket, and a state change marks the rows stale
  (`refresh_devws_pickers.sh --reload`) instead of restarting fzf
