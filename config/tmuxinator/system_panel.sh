#!/usr/bin/env bash
# The system workspace's panels (system.yml) and the log choice behind them
# (System monitor in the devws menu):
#
#   system_panel.sh top             btop: the machine's processes, CPU, memory
#   system_panel.sh docker          lazydocker: containers, images, logs
#   system_panel.sh log [--choose]  runs the saved log command, a tail -F.
#                                   With no command yet, or --choose (the
#                                   menu's System monitor), it first asks for
#                                   the log in the pane, in a box inset in it:
#                                   the other panels go on meanwhile. Ctrl-C
#                                   stops the command and asks again: another
#                                   log, or the command to edit (-n, a |
#                                   grep…). Esc follows the same one again
#   system_panel.sh docker-group    the group lazydocker joins (sg) to reach
#                                   Docker, when the panes lack it
#   system_panel.sh command-for <file>   the tail command for that file
#   system_panel.sh use <file>      makes that file the log to follow, and
#                                   the first of the recent ones
#   system_panel.sh choices [<folder>…]  the log list's rows,
#                                   "<key><TAB><label>": search, type, the
#                                   recent logs that still exist, then the
#                                   *.log files under /var/log and the
#                                   folders given
#   system_panel.sh files           every file under ~ and /var/log, for the
#                                   list's search
#
# A panel never closes by itself: a tool that quits, or is missing, waits for
# Enter. The command and the recent logs are kept in ~/.devws-state, so a
# restored workspace follows the log it had.

set -u

state_dir="$HOME/.devws-state"
command_file="$state_dir/system-log-command"
recent_file="$state_dir/system-logs"
recent_limit=10

fail() {
  printf 'devws system: %s\n' "$1" >&2
  exit 1
}

# The pane's role and the name its header shows (tmux.conf).
mark_pane() {
  [ -n "${TMUX_PANE:-}" ] || return 0
  tmux set-option -p -t "$TMUX_PANE" @devws_role "$1"
  tmux set-option -p -t "$TMUX_PANE" @devws_label "$2"
}

# The Docker socket's group, when this process lacks it though the user is
# in it: a tmux server started before the user joined the docker group runs
# every pane without it until it restarts. Fails when Docker is reachable,
# not a local socket, or not the user's to reach.
docker_group() {
  local socket="${DOCKER_HOST:-unix:///var/run/docker.sock}" group
  case "$socket" in unix://*) socket="${socket#unix://}" ;; *) return 1 ;; esac
  [ -S "$socket" ] && [ ! -w "$socket" ] || return 1
  group="$(stat -c %G "$socket" 2>/dev/null || stat -f %Sg "$socket" 2>/dev/null)" || return 1
  id -Gn "$(id -un)" | tr ' ' '\n' | grep -qxF "$group" || return 1
  printf '%s\n' "$group"
}

run_tool() {
  local tool="$1" reply group
  local -a command
  while :; do
    command=("$tool")
    if [ "$tool" = lazydocker ] && group="$(docker_group)" && command -v sg >/dev/null 2>&1; then
      command=(sg "$group" -c "exec $tool")
    fi
    if command -v "$tool" >/dev/null 2>&1; then
      "${command[@]}" || :
      printf '\n%s closed. Enter starts it again.' "$tool"
    else
      printf '%s is missing: run ./install.sh --deps in your devws folder (or brew install %s).\nEnter checks again.' "$tool" "$tool"
    fi
    # No terminal to read from: wait instead of spinning.
    IFS= read -r reply || sleep 2
  done
}

command_for() {
  printf 'tail -n 200 -F %q\n' "$1"
}

saved_command() {
  [ -r "$command_file" ] && cat "$command_file"
}

save_command() {
  mkdir -p "$state_dir"
  printf '%s\n' "$1" > "$command_file"
}

# The header shows the command, cut to fit.
log_label() {
  local command="$1"
  [ "${#command}" -le 60 ] || command="${command:0:59}…"
  printf 'Log · %s' "$command"
}

# The question runs in the log pane, never in a popup: a tmux popup takes the
# whole client's keys, and the other panels must go on while it waits. fzf
# (devws-ui draws no box) frames it in a box inset in the pane.
pick() {
  DEVWS_UI=/nonexistent "$HOME/.tmuxinator/picker.sh" "$@" \
    --height=100% --margin=12%,10% --border=rounded --border-label=" $1 "
}

edit_command() {
  local command reply
  command="$(saved_command)"
  printf '\033c\033[2mEdit the command and press Enter; Ctrl-C keeps it.\033[0m\n'
  if IFS= read -e -r -i "$command" -p 'log $ ' reply && [ -n "$reply" ]; then
    save_command "$reply"
  fi
}

# The open workspaces' folders, where their own logs are looked for.
workspace_roots() {
  tmux list-sessions -F '#{@devws_root}' 2>/dev/null | awk 'NF && !seen[$0]++'
}

# Asks for the log to follow: from the list, by searching every file, or
# typed; with a command already chosen, editing it comes first. Fails on Esc,
# or on a file it cannot read, keeping the command as it was.
choose_log() {
  local choice path
  local -a roots
  mapfile -t roots < <(workspace_roots)
  choice="$(
    {
      [ -z "$(saved_command)" ] || printf 'edit\t✎ Edit the command\n'
      choices "${roots[@]}"
    } | pick 'Log file' 'The log this pane follows' --delimiter=$'\t' --with-nth=2
  )" || return 1
  path="${choice%%$'\t'*}"
  case "$path" in
    edit) edit_command; return 0 ;;
    search) path="$(files | pick Search 'Every file under ~ and /var/log')" || return 1 ;;
    type)
      printf '\033c'
      IFS= read -e -r -p 'Log file (Tab completes, Ctrl-C cancels): ' path || return 1
      case "$path" in
        "~") path="$HOME" ;;
        "~/"*) path="$HOME/${path#\~/}" ;;
      esac
      ;;
  esac
  [ -n "$path" ] || return 1
  # use_log stops with a message on a file it cannot read: not this pane.
  ( use_log "$path" ) || { printf 'Enter goes back to the list.'; IFS= read -r _ || sleep 2; return 1; }
}

run_log() {
  local command ask_first="$1"
  # Ctrl-C ends the command, not this loop.
  trap ':' INT
  while :; do
    if [ "$ask_first" = 1 ] || [ -z "$(saved_command)" ]; then
      mark_pane system-log 'Log · choose a file'
      if ! choose_log && [ -z "$(saved_command)" ]; then
        printf '\033cNo log chosen yet. Enter shows the list.'
        IFS= read -r _ || sleep 2
        continue
      fi
    fi
    ask_first=1
    command="$(saved_command)"
    [ -n "$command" ] || continue
    printf '\033c'
    mark_pane system-log "$(log_label "$command")"
    printf '\033[2m$ %s   (Ctrl-C: another log or edit)\033[0m\n' "$command"
    bash -c "$command"
  done
}

absolute_path() {
  local folder
  folder="$(cd "$(dirname "$1")" 2>/dev/null && pwd)" || return 1
  printf '%s/%s\n' "$folder" "$(basename "$1")"
}

use_log() {
  local file
  [ -f "$1" ] && [ -r "$1" ] || fail "cannot read $1"
  file="$(absolute_path "$1")" || fail "cannot read $1"
  save_command "$(command_for "$file")"
  {
    printf '%s\n' "$file"
    grep -Fvx -- "$file" "$recent_file" 2>/dev/null
  } | head -n "$recent_limit" > "$recent_file.next"
  mv "$recent_file.next" "$recent_file"
}

# *.log files near the system's logs and in the folders given, without
# dependency or git folders. Home itself is too large to walk here: the
# search option covers it.
found_logs() {
  local folder
  {
    find /var/log -maxdepth 2 -type f \( -name '*.log' -o -name syslog -o -name messages \) 2>/dev/null
    for folder in "$@"; do
      [ -d "$folder" ] && [ "$folder" != "$HOME" ] || continue
      find "$folder" -maxdepth 4 \( -name node_modules -o -name .git \) -prune -o -type f -name '*.log' -print 2>/dev/null
    done
  } | sort
}

choices() {
  local file
  printf 'search\t⌕ Search files…\n'
  printf 'type\t✎ Type a path…\n'
  {
    [ -r "$recent_file" ] && cat "$recent_file"
    found_logs "$@"
  } | awk '!seen[$0]++' | while IFS= read -r file; do
    [ -f "$file" ] && [ -r "$file" ] && printf '%s\t%s\n' "$file" "$file"
  done
  return 0
}

files() {
  if command -v fd >/dev/null 2>&1; then
    fd --type f --hidden --exclude .git --exclude node_modules . "$HOME" /var/log 2>/dev/null
  else
    find "$HOME" /var/log \( -name node_modules -o -name .git \) -prune -o -type f -print 2>/dev/null
  fi
  return 0
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  top) mark_pane system-top 'System · btop'; run_tool btop ;;
  docker) mark_pane system-docker 'Docker · lazydocker'; run_tool lazydocker ;;
  log) run_log "$([ "${1:-}" = --choose ] && echo 1 || echo 0)" ;;
  docker-group) docker_group ;;
  command-for) [ -n "${1:-}" ] || fail "usage: system_panel.sh command-for <file>"; command_for "$1" ;;
  use) [ -n "${1:-}" ] || fail "usage: system_panel.sh use <file>"; use_log "$1" ;;
  choices) choices "$@" ;;
  files) files ;;
  *) fail "usage: system_panel.sh <top|docker|log|docker-group|command-for|use|choices|files>" ;;
esac
