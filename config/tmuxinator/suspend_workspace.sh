#!/usr/bin/env bash
# Freezes the workspaces where nothing is happening, and brings them back the
# moment you return, so they cost no CPU meanwhile (their memory stays used).
#
#   suspend_workspace.sh sweep              (header_ticker.sh, every minute)
#                                           freeze each workspace unseen for
#                                           DEVWS_SUSPEND_AFTER minutes (.env,
#                                           default 15; 0 turns it off) whose
#                                           agent is idle or done
#   suspend_workspace.sh resume <session>   (tmux.conf, on switch or attach)
#   suspend_workspace.sh resume --all       (reload_devws.sh)
#   suspend_workspace.sh resume --closed    (tmux.conf, a workspace closed)
#
# Frozen: the agents, the editors, the sidebar and the minimized placeholders,
# with every process they started (language servers, MCP servers), all below
# the devws script that runs them. Never the terminals (role terminal or
# extra), which may serve or watch something, nor a pane devws did not start
# (an extra Neovim, or an agent started before the runners stopped replacing
# themselves). A workspace counts as seen while a client shows
# it (@devws_seen, stamped by the sweep) and from when its agent last changed
# state (@devws_agent_state_since).

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${DEVWS_SUSPEND_AFTER+set}" ] && [ -r "$script_dir/.env" ]; then
  # shellcheck source=/dev/null
  . "$script_dir/.env"
fi
frozen_dir="${TMPDIR:-/tmp}"
frozen_dir="${frozen_dir%/}/devws-frozen-$(id -u)"

# <pane pid…>: in each pane, what runs below its devws script (agent_runner.sh,
# editor_runner.sh, session_picker.sh, a placeholder), each parent before its
# children. Never the script nor the shell above it: tmux continues a pane's
# own process when it stops, and the interactive shell that tmuxinator typed
# the runner into would see its program stop and take the pane back; the
# script, a bash without job control, waits on regardless. A pane without a
# devws script is left out.
below_devws_scripts() {
  ps -axo pid=,ppid=,command= | awk -v roots="$*" '
    {
      children[$2] = children[$2] " " $1
      # A bash running a devws script, not a program that only mentions one
      # (an agent given devws hooks on its command line).
      is_script[$1] = $3 ~ /(^|\/)bash$/ && $4 ~ /\/\.tmuxinator\//
    }
    # Breadth first from <pid>: the first devws script met.
    function script_in(pid,    queue, head, tail, count, list, i) {
      queue[1] = pid
      tail = 1
      for (head = 1; head <= tail; head++) {
        if (is_script[queue[head]]) return queue[head]
        count = split(children[queue[head]], list, " ")
        for (i = 1; i <= count; i++) queue[++tail] = list[i]
      }
      return ""
    }
    END {
      count = split(roots, pane_roots, " ")
      for (root = 1; root <= count; root++) {
        script = script_in(pane_roots[root])
        if (script == "") continue
        tail = split(children[script], queue, " ")
        for (head = 1; head <= tail; head++) {
          print queue[head]
          found = split(children[queue[head]], list, " ")
          for (i = 1; i <= found; i++) queue[++tail] = list[i]
        }
      }
    }'
}

# <session id>: the pids of the panes to freeze.
freezable_panes() {
  tmux list-panes -s -t "$1" -F "#{pane_pid}	#{@devws_role}	#{pane_title}" 2>/dev/null |
    awk -F '\t' '
      $2 ~ /^(extra-)?(agent|editor)$/ || $2 ~ /^minimized-/ || ($2 == "" && $3 == "devws-picker") { print $1 }'
}

is_attached() {
  [ "$(tmux display-message -p -t "$1" '#{session_attached}' 2>/dev/null || echo 1)" != 0 ]
}

# Lines "<pid> <start time>": the start time tells a pid that was reused
# after its process ended.
start_times() {
  ps -o pid=,lstart= -p "$1" 2>/dev/null | awk '{ pid = $1; $1 = ""; print pid $0 }'
}

# <pids, one a line>: their start time lines, in the same order.
with_start_times() {
  awk 'NR == FNR { started[$1] = $0; next } $1 in started { print started[$1] }' \
    <(start_times "$(paste -sd , - <<<"$1")") - <<<"$1"
}

# Continued children first: a shell only learns that its program stopped if
# it is still stopped when the shell looks (wait4), so it never takes the
# terminal back.
continue_recorded() {
  local file="$1" pids
  pids="$(cut -d ' ' -f1 "$file" | paste -sd , -)"
  [ -n "$pids" ] || return 0
  start_times "$pids" |
    awk 'NR == FNR { alive[$0] = 1; next } $0 in alive { print $1 }' - "$file" |
    tail -r | xargs kill -CONT 2>/dev/null
  return 0
}

# <pids, one a line>: whether one is a shell other than devws's own scripts —
# a job the agent left running (Claude's background commands run in one), or
# a terminal opened in the editor. An idle agent or editor runs none.
runs_a_job() {
  ps -o comm=,command= -p "$(paste -sd , - <<<"$1")" 2>/dev/null |
    awk '$1 ~ /(^|\/)-?(sh|bash|zsh|dash|fish)$/ && $3 !~ /\/\.tmuxinator\// { found = 1 }
      END { exit !found }'
}

suspend_session() {
  local session="$1" file pids
  is_attached "$session" && return 0
  pids="$(below_devws_scripts $(freezable_panes "$session"))"
  [ -n "$pids" ] || return 0
  # Something still runs an action: the workspace is not idle.
  runs_a_job "$pids" && return 0
  mkdir -p "$frozen_dir"
  file="$frozen_dir/${session#\$}"
  with_start_times "$pids" > "$file"
  tmux set-option -t "$session" @devws_frozen 1
  # Each parent stops before its children, so none sees a child stop.
  cut -d ' ' -f1 "$file" | xargs kill -STOP 2>/dev/null
  # You came back while it froze: the switch's resume may have run too early.
  if is_attached "$session"; then
    resume_session "$session"
  else
    # The other sidebars show it paused.
    "$script_dir/refresh_devws_pickers.sh" --reload
  fi
  return 0
}

resume_session() {
  local session="$1" file
  file="$frozen_dir/${session#\$}"
  tmux set-option -t "$session" @devws_seen "$(date +%s)" 2>/dev/null
  [ -f "$file" ] || return 0
  continue_recorded "$file"
  rm -f "$file"
  tmux set-option -u -t "$session" @devws_frozen 2>/dev/null
  # Its sidebar slept through the changes.
  "$script_dir/refresh_devws_pickers.sh" --reload
}

# <all|closed>: every frozen workspace, or the leftovers of closed ones.
resume_files() {
  local which="$1" file id sessions
  sessions="$(tmux list-sessions -F '#{session_id}' 2>/dev/null)"
  for file in "$frozen_dir"/*; do
    [ -f "$file" ] || continue
    id="\$${file##*/}"
    if grep -qxF "$id" <<<"$sessions"; then
      [ "$which" = all ] && resume_session "$id"
    else
      continue_recorded "$file"
      rm -f "$file"
    fi
  done
}

sweep() {
  local after="${DEVWS_SUSPEND_AFTER:-15}" now id name attached seen state since frozen last
  case "$after" in '' | *[!0-9]*) after=15 ;; esac
  [ "$after" -gt 0 ] || return 0
  now="$(date +%s)"
  while IFS='|' read -r id name attached seen state since frozen; do
    case "$name" in dev-*) ;; *) continue ;; esac
    if [ "$attached" != 0 ]; then
      tmux set-option -t "$id" @devws_seen "$now"
      continue
    fi
    [ -z "$frozen" ] || continue
    case "$state" in working | waiting) continue ;; esac
    last="$seen"
    [ "${since:-0}" -gt "$last" ] && last="$since"
    [ $((now - last)) -ge $((after * 60)) ] && suspend_session "$id"
  done < <(tmux list-sessions \
    -F '#{session_id}|#{session_name}|#{session_attached}|#{?@devws_seen,#{@devws_seen},#{session_created}}|#{@devws_agent_state}|#{@devws_agent_state_since}|#{@devws_frozen}' 2>/dev/null)
}

# A session name, or an id as the hooks give it.
session_id() {
  case "$1" in
    \$*) printf '%s\n' "$1" ;;
    *) tmux display-message -p -t "=$1:" '#{session_id}' 2>/dev/null ;;
  esac
}

case "${1:-} ${2:-}" in
  'sweep ') sweep ;;
  'resume --all') resume_files all ;;
  'resume --closed') resume_files closed ;;
  resume\ ?*)
    id="$(session_id "$2")"
    [ -n "$id" ] && resume_session "$id"
    ;;
  *)
    printf 'Usage: suspend_workspace.sh sweep | resume <session|--all|--closed>\n' >&2
    exit 2
    ;;
esac
exit 0
