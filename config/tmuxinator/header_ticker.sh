#!/usr/bin/env bash
# Keeps the header current: the one background loop behind tmux.conf's three
# rows of groups — the clock, the system, the agents, the network, the
# weather — and the ghost beside them, which only show what it writes.
#
#   header_ticker.sh --start   (tmux.conf) replaces the running ticker
#   header_ticker.sh --run     (internal) the loop itself
#
# Sources, each kept in a file of the state folder: the system usage and the
# network every 5 s (header_system.sh, header_network.sh), the agents' limits
# every minute (header_agents.sh) with their month's cost worked out every 10
# min (header_tokens.sh costs), the weather every 30 min (header_weather.sh).
# A source that fails keeps its last rows. Every minute too, each workspace's
# conversation (header_tokens.sh) goes to its @devws_agent_session_usage,
# which tmux's own line shows, and the idle workspaces are frozen
# (suspend_workspace.sh sweep).
#
# Two speeds:
#   every second   the clock is drawn (header_clock.sh; left out when the
#                  other groups need its room) and the rows are laid out
#                  (header_layout.sh) at the width of
#                  each attached client, into the session's
#                  @devws_header_row_<1-3>; the global rows, at the first
#                  client's width, stand in for a session until then
#   20 times a s.  the ghost (header_pet.sh) moves, into each attached
#                  workspace's @devws_ghost_<1-3>, in one mood for all the
#                  agents (overall_mood).
#                  Its cycles are drawn once at start and played back, so a
#                  frame is a single tmux call.
# Both send tmux only what changed: each change makes tmux redraw the status
# lines and the terminal draw them, so a frame the same as the last one, or
# rows whose data has not moved, are skipped.
# The ticker stops once the tmux server is gone or another ticker has replaced
# it.

set -u

script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
scripts_dir="$(dirname "$script")"
state_dir="${TMPDIR:-/tmp}"
state_dir="${state_dir%/}/devws-header-$(id -u)"
# Present while a devws popup (the menu) is open (devws_menu.sh): a redraw
# then shows the panes behind it for an instant, so the header holds still.
popup_flag="${TMPDIR:-/tmp}"
popup_flag="${popup_flag%/}/devws-popup-$(id -u)"
moods="idle working waiting done"
# The ghost's lane, and the columns under which it is left out.
ghost_width=12
narrow_width=110
# Frames a second: the loop's pace, which every other interval counts in.
fps=20

# Restarting on every tmux.conf load is how an update reaches a running server.
start() {
  local running
  running="$(tmux show-option -gqv @devws_header_ticker)"
  [ -n "$running" ] && kill "$running" 2>/dev/null
  # Started by the tmux server, so it outlives the shell that asked for it.
  tmux run-shell -b "exec '$script' --run >/dev/null 2>&1"
}

# The settings and the colors tmux.conf defines. The layout counts characters,
# which needs a UTF-8 locale; the tmux server may not have passed one on.
load_settings() {
  # Exported (set -a): the agent sources read each agent's DEVWS_AGENT_<name>.
  # DEVWS_ENV_FILE replaces .env, for tests.
  local env_file="${DEVWS_ENV_FILE:-$scripts_dir/.env}"
  set -a
  # shellcheck source=/dev/null
  [ -r "$env_file" ] && . "$env_file"
  set +a
  export DEVWS_AGENTS="${DEVWS_AGENTS-claude codex}" DEVWS_WEATHER_LOCATION="${DEVWS_WEATHER_LOCATION:-}"
  export DEVWS_COLOR_OK DEVWS_COLOR_WARN DEVWS_COLOR_STRESS DEVWS_COLOR_DIM DEVWS_COLOR_RULE DEVWS_COLOR_GHOST
  DEVWS_COLOR_OK="$(tmux show-option -gqv @devws_color_ok)"
  DEVWS_COLOR_WARN="$(tmux show-option -gqv @devws_color_warn)"
  DEVWS_COLOR_STRESS="$(tmux show-option -gqv @devws_color_stress)"
  DEVWS_COLOR_DIM="$(tmux show-option -gqv @devws_color_dim)"
  DEVWS_COLOR_RULE="$(tmux show-option -gqv @devws_color_rule)"
  DEVWS_COLOR_GHOST="$(tmux show-option -gqv @devws_color_ghost)"
  show_pet="${DEVWS_HEADER_PET:-1}"
  case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
    *UTF-8* | *utf8*) ;;
    *) export LC_ALL=en_US.UTF-8 ;;
  esac
}

# Writes a source's rows only when it printed some, and whole.
keep() {
  local name="$1" rows
  shift
  rows="$("$@")"
  [ -n "$rows" ] || return 0
  printf '%s\n' "$rows" > "$state_dir/$name.new" && mv "$state_dir/$name.new" "$state_dir/$name"
}

update_tokens() {
  local session transcript usage
  keep agents nice "$scripts_dir/header_agents.sh" "$state_dir/costs"
  while IFS='|' read -r session transcript; do
    [ -n "$transcript" ] || continue
    usage="$(nice "$scripts_dir/header_tokens.sh" session "$transcript")"
    [ -n "$usage" ] && tmux set-option -t "$session" @devws_agent_session_usage "$usage"
  done < <(tmux list-sessions -F '#{session_id}|#{@devws_agent_transcript}')
}

# Slow work runs beside the loop so the ghost keeps moving; work still busy
# from its last turn is skipped.
in_background() {
  local name="$1" pid_variable="${1}_pid"
  shift
  [ -n "${!pid_variable:-}" ] && kill -0 "${!pid_variable}" 2>/dev/null && return
  "$@" &
  printf -v "$pid_variable" '%s' "$!"
}

# Sets <variable> to what the command prints, every line kept: $(…) alone
# drops trailing empty rows, which would move the next group's rows up.
#   capture_rows <variable> <command…>
capture_rows() {
  local variable="$1" rows
  shift
  rows="$("$@"; printf .)"
  printf -v "$variable" '%s' "${rows%.}"
}

# Prints a source's first <count> rows, padded with empty ones.
source_rows() {
  local name="$1" count="$2"
  { head -n "$count" "$state_dir/$name" 2>/dev/null; yes '' | head -n "$count"; } | head -n "$count"
}

# The width the groups get: all of it when the ghost is left out.
groups_width() {
  local width="$1"
  if [ "$show_pet" = 1 ] && [ "$width" -ge "$narrow_width" ]; then
    printf '%s' $((width - ghost_width - 1))
  else
    printf '%s' "$width"
  fi
}

# Adds the set-option arguments for one target's rows to the frame, chained
# with ";" so all of it is one tmux call.
add_rows() {
  local target_flag="$1" target="$2" width="$3" row index=1
  while IFS= read -r row; do
    if [ -n "$target" ]; then
      frame+=(set-option "$target_flag" "$target" "@devws_header_row_$index" "$row" \;)
    else
      frame+=(set-option "$target_flag" "@devws_header_row_$index" "$row" \;)
    fi
    index=$((index + 1))
  done < <(printf '%s' "$clock_rows" "$system_rows" "$agent_rows" "$network_rows" "$weather_rows" |
    "$scripts_dir/header_layout.sh" "$(groups_width "$width")" 5 --optional-first)
}

# The mood of all the workspaces' agents together: the first of waiting (one
# needs you), working, done (one has finished, for you to look at) that any
# agent is in, else idle, when the ghost sleeps.
overall_mood() {
  local states mood
  states=" $(tmux list-sessions -F '#{@devws_agent_state}' 2>/dev/null | tr '\n' ' ') "
  for mood in waiting working done; do
    case "$states" in *" $mood "*) printf '%s' "$mood"; return ;; esac
  done
  printf 'idle'
}

# Every second, in the background: the rows of every attached workspace, and
# the list of them with the agents' overall mood, which the ghost reads.
lay_out() {
  local session width mood first_width="" drawn=" " clients="" checksum
  capture_rows clock_rows "$scripts_dir/header_clock.sh"
  capture_rows system_rows source_rows system 3
  capture_rows agent_rows source_rows agents 3
  capture_rows network_rows source_rows network 3
  capture_rows weather_rows source_rows weather 3
  mood="$(overall_mood)"
  frame=()
  while IFS='|' read -r session width; do
    [ -n "$first_width" ] || first_width="$width"
    # A session with two clients is laid out once, at the first one's width.
    case "$drawn" in *" $session "*) continue ;; esac
    drawn+="$session "
    clients+="$session|$mood|$width"$'\n'
    add_rows -t "$session" "$width"
  done < <(tmux list-clients -F '#{session_id}|#{client_width}')
  printf '%s' "$clients" > "$state_dir/clients.new" && mv "$state_dir/clients.new" "$state_dir/clients"
  [ -n "$first_width" ] || return 0
  add_rows -g "" "$first_width"
  # Rows the same as those last sent are not sent again: tmux would redraw the
  # status lines for nothing. The data changes every few seconds at most.
  checksum="$(printf '%s\n' "${frame[@]}" | cksum)"
  [ "$checksum" = "$(cat "$state_dir/rows.sent" 2>/dev/null)" ] && return 0
  # Without the last ";", which would start an empty command.
  tmux "${frame[@]:0:${#frame[@]}-1}" && printf '%s\n' "$checksum" > "$state_dir/rows.sent"
}

# The ghost's cycles (colored by header_pet.sh), one after the other in
# ghost_rows; each mood's first row and frame count in ghost_start_<mood> and
# ghost_frames_<mood>.
load_ghost() {
  local mood row count
  ghost_rows=()
  for mood in $moods; do
    printf -v "ghost_start_$mood" '%s' "${#ghost_rows[@]}"
    count=0
    while IFS= read -r row; do
      ghost_rows+=("$row")
      count=$((count + 1))
    done < <("$scripts_dir/header_pet.sh" "$mood")
    printf -v "ghost_frames_$mood" '%s' $((count / 3))
  done
}

# The attached workspaces, as the last layout found them.
read_clients() {
  local session state width previous="${ghost_sessions[*]:-}"
  ghost_sessions=()
  ghost_moods=()
  while IFS='|' read -r session state width; do
    [ -n "$session" ] || continue
    [ "$width" -ge "$narrow_width" ] || continue
    case " $moods " in *" $state "*) ;; *) state=idle ;; esac
    ghost_sessions+=("$session")
    ghost_moods+=("$state")
  done < "$state_dir/clients" 2>/dev/null
  # Other workspaces attached: what each one shows is not known any more.
  [ "${ghost_sessions[*]:-}" = "$previous" ] || ghost_shown=()
}

# Every tick looks, but only a frame that differs from the one a workspace
# shows goes to tmux, which redraws the status lines on each change: a slow
# mood repeats its frame for many ticks.
draw_ghost() {
  local tick="$1" index start_variable frames_variable first line rows
  [ "${#ghost_sessions[@]}" -gt 0 ] || return 0
  frame=()
  for index in "${!ghost_sessions[@]}"; do
    start_variable="ghost_start_${ghost_moods[index]}"
    frames_variable="ghost_frames_${ghost_moods[index]}"
    first=$((${!start_variable} + (tick % ${!frames_variable}) * 3))
    rows="${ghost_rows[first]}"$'\n'"${ghost_rows[first + 1]}"$'\n'"${ghost_rows[first + 2]}"
    [ "$rows" = "${ghost_shown[index]:-}" ] && continue
    ghost_shown[index]="$rows"
    for line in 0 1 2; do
      frame+=(set-option -t "${ghost_sessions[index]}" "@devws_ghost_$((line + 1))" "${ghost_rows[first + line]}" \;)
    done
  done
  [ "${#frame[@]}" -gt 0 ] || return 0
  tmux "${frame[@]:0:${#frame[@]}-1}"
}

run() {
  local tick=0
  mkdir -p "$state_dir"
  tmux set-option -g @devws_header_ticker "$$" || exit 0
  load_settings
  [ "$show_pet" = 1 ] && load_ghost
  # A new ticker sends everything once: a restarted tmux server has no rows.
  rm -f "$state_dir/rows.sent"
  ghost_sessions=()
  ghost_shown=()
  while true; do
    if [ $((tick % fps)) -eq 0 ]; then
      header_hidden="$(tmux show-option -gqv @devws_header_hidden 2>/dev/null)"
      if [ -z "$header_hidden" ] && [ ! -e "$popup_flag" ]; then
        read_clients
        in_background layout lay_out
      fi
    fi
    # Every 5 s: still the ticker of a running server?
    if [ $((tick % (5 * fps))) -eq 0 ]; then
      [ "$(tmux show-option -gqv @devws_header_ticker 2>/dev/null)" = "$$" ] || exit 0
      in_background system keep system nice "$scripts_dir/header_system.sh" "$state_dir/cpu.history"
      in_background network keep network nice "$scripts_dir/header_network.sh" "$state_dir"
    fi
    [ $((tick % (600 * fps))) -eq 0 ] && in_background costs keep costs nice "$scripts_dir/header_tokens.sh" costs
    [ $((tick % (60 * fps))) -eq 0 ] && in_background tokens update_tokens
    [ $((tick % (60 * fps))) -eq 0 ] && in_background suspend "$scripts_dir/suspend_workspace.sh" sweep
    [ $((tick % (1800 * fps))) -eq 0 ] && in_background weather keep weather "$scripts_dir/header_weather.sh"
    # While the header is hidden (header_toggle.sh) nothing is drawn: one turn
    # a second keeps the sources current for when it comes back.
    if [ -n "$header_hidden" ]; then
      tick=$((tick + fps))
      sleep 1
      continue
    fi
    if [ "$show_pet" = 1 ] && [ ! -e "$popup_flag" ]; then
      draw_ghost "$tick" 2>/dev/null || exit 0
    fi
    tick=$((tick + 1))
    sleep 0.05
  done
}

case "${1:-}" in
  --start) start ;;
  --run) run ;;
  *)
    printf 'Usage: header_ticker.sh --start\n' >&2
    exit 2
    ;;
esac
