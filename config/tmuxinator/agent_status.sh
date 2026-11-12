#!/usr/bin/env bash
# Records an agent's state on its pane, and the workspace's: a workspace can
# hold several agents (the main one, and extra ones from New panel).
#
# Each agent pane keeps its own state in the pane option @devws_pane_state
# (unset while idle), which its header shows (tmux.conf's @devws_pane_icon and
# @devws_pane_badge). The workspace's @devws_agent_state (unset while idle)
# sums its agents up —
# waiting, else working, else done — for the minimized strip, the status bar,
# the sidebar and the header's ghost. When that state started goes in
# @devws_agent_state_since (epoch seconds), which jump_agent.sh orders the
# waiting agents by.
#
# When an agent starts waiting or is done in a workspace no client shows, a
# macOS notification says so: the workspace and its state, its folder and
# branch, and the agent. Clicked, it takes you to that agent (focus_agent.sh);
# that needs terminal-notifier, else osascript shows it, not clickable
# (alert.sh). An agent team's agents (@devws_task) are done after every
# order, so they alert only when waiting; team_alert.sh tells when the whole
# team stops. DEVWS_NOTIFY in .env: 1 (default) for both, "waiting" for
# waiting only, 0 for none.
#
#   agent_status.sh <session> <working|waiting|done|idle> [pane]
#                                  the agent in <pane>; without one, the
#                                  workspace's main agent (role agent)
#
# The spinner shown while an agent works turns in header_ticker.sh, one clock
# for every workspace (agent_spinner.sh).

set -u

session="${1:-}"
state="${2:-}"
pane="${3:-}"
script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
# shellcheck source=alert.sh
. "$(dirname "$script")/alert.sh"

case "$state" in
  working|waiting|done|idle) ;;
  *)
    printf 'Unknown agent state: %s (working, waiting, done, idle)\n' "$state" >&2
    exit 2
    ;;
esac
if ! tmux has-session -t "=$session" 2>/dev/null; then
  printf 'No devws workspace named: %s\n' "$session" >&2
  exit 1
fi

# Whether a client shows the workspace, where the user sees the state already.
on_screen() {
  tmux list-clients -F '#{client_session}' 2>/dev/null | grep -qxF "$session"
}

# The agent pane the alert is about: the one that reported, when it is in the
# workspace's new state, else the first agent in it (the workspace can turn
# done because another agent went idle).
alerting_pane() {
  local agent
  if [ -n "$pane" ] && [ "$(tmux show-options -pqv -t "$pane" @devws_pane_state 2>/dev/null)" = "$state" ]; then
    printf '%s' "$pane"
    return
  fi
  for agent in $(agent_panes); do
    [ "$(tmux show-options -pqv -t "$agent" @devws_pane_state 2>/dev/null)" = "$state" ] || continue
    printf '%s' "$agent"
    return
  done
  printf '%s' "$pane"
}

# The workspace's project folder and branch: "orbit-back · feat/login".
project_line() {
  local root branch
  root="$(tmux show-option -qv -t "=$session:" @devws_root)"
  [ -n "$root" ] || return 0
  branch="$(git -C "$root" branch --show-current 2>/dev/null)"
  printf '%s%s' "$(basename "$root")" "${branch:+ · $branch}"
}

notify() {
  local title subtitle message agent agent_pane where=""
  case "$(notify_setting):$state" in
    0:*|waiting:done) return 0 ;;
  esac
  # An agent team's agents are done after every order: only needing you
  # alerts; team_alert.sh tells when the whole team stops.
  if [ "$state" = done ] && [ -n "$(tmux show-option -qv -t "=$session:" @devws_task 2>/dev/null)" ]; then
    return 0
  fi
  on_screen && return 0
  agent_pane="$(alerting_pane)"
  # An extra agent (New panel) carries its own name on its pane.
  if [ -n "$agent_pane" ] && [ "$(tmux show-options -pqv -t "$agent_pane" @devws_role 2>/dev/null)" = extra-agent ]; then
    agent="$(tmux show-options -pqv -t "$agent_pane" @devws_agent 2>/dev/null)"
    where=" in a new panel"
  else
    agent="$(tmux show-option -qv -t "=$session:" @devws_agent)"
  fi
  case "$state" in
    waiting)
      title="${session#dev-} ● needs you"
      message="${agent:-the agent}$where needs you"
      ;;
    done)
      title="${session#dev-} ✓ done"
      message="${agent:-the agent}$where is done"
      ;;
  esac
  subtitle="$(project_line)"
  post_alert "$title" "$subtitle" "$message" "$session" "$agent_pane"
}

# The workspace's agent panes, also those hidden while minimized.
agent_panes() {
  tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
    awk '$2 ~ /^(agent|extra-agent)$/ { print $1 }'
}

# The workspace's state from its agents' own ones: waiting, else working,
# else done, else idle.
summary() {
  local states
  states=" $(tmux list-panes -s -t "=$session" -F '#{@devws_role}|#{@devws_pane_state}' 2>/dev/null |
    awk -F '|' '$1 ~ /^(agent|extra-agent)$/ { print $2 }' | tr '\n' ' ') "
  for candidate in waiting working done; do
    case "$states" in *" $candidate "*) printf '%s' "$candidate"; return ;; esac
  done
  printf 'idle'
}

[ -n "$pane" ] || pane="$(tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
  awk '$2 == "agent" { print $1; exit }')"
if [ -n "$pane" ]; then
  previous_pane="$(tmux show-options -pqv -t "$pane" @devws_pane_state 2>/dev/null)"
  next_pane="$state"
  [ "$state" != idle ] || next_pane=""
  if [ "$previous_pane" != "$next_pane" ]; then
    if [ "$state" = idle ]; then
      tmux set-option -p -u -t "$pane" @devws_pane_state 2>/dev/null
    else
      tmux set-option -p -t "$pane" @devws_pane_state "$state" 2>/dev/null
    fi
    # shellcheck source=workspace_panes.sh
    . "$(dirname "$script")/workspace_panes.sh"
    mark_tab_alert "$pane"
  fi
  state="$(summary)"
fi

previous="$(tmux show-option -qv -t "=$session:" @devws_agent_state)"
if [ "$state" = idle ]; then
  state=""
fi
if [ "$previous" != "$state" ]; then
  if [ -z "$state" ]; then
    tmux set-option -u -t "=$session:" @devws_agent_state
    tmux set-option -u -t "=$session:" @devws_agent_state_since
  else
    tmux set-option -t "=$session:" @devws_agent_state "$state"
    tmux set-option -t "=$session:" @devws_agent_state_since "$(date +%s)"
  fi
fi
# The header's ticker draws these states. One that died (nothing left to say
# why) would leave the header frozen until tmux.conf is loaded again: an agent
# event starts a new one.
ticker="$(tmux show-option -gqv @devws_header_ticker 2>/dev/null)"
if [ -n "$ticker" ] && ! kill -0 "$ticker" 2>/dev/null; then
  "$(dirname "$script")/header_ticker.sh" --start >/dev/null 2>&1
fi
# The sidebar redraws its rows, and a notification goes out, only when the
# state changes.
if [ "$previous" != "$state" ]; then
  # The frame is global (agent_spinner.sh); one left on the session by an
  # older devws, with a ticker per workspace, would hide it.
  tmux set-option -qu -t "=$session:" @devws_agent_frame
  "$(dirname "$script")/refresh_devws_pickers.sh" --reload
  case "$state" in
    waiting|done) notify ;;
  esac
fi
exit 0
