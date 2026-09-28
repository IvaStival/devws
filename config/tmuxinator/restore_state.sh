#!/usr/bin/env bash
# Rebuild the workspaces recorded by save_state.sh.
#
# Never attaches: the caller decides whether to attach or switch-client. Safe to
# run repeatedly — workspaces that are already open are left untouched.

set -u

state_root="$HOME/.devws-state"
state_file="$state_root/sessions.tsv"
lock_dir="$state_root/restore.lock"
config="$HOME/.tmuxinator/.env"
tab="$(printf '\t')"

[ -r "$config" ] && . "$config"
[ -s "$state_file" ] || exit 0

mkdir -p "$state_root" || exit 1
# mkdir is the atomic lock primitive; its presence also suppresses save_state.sh.
if ! mkdir "$lock_dir" 2>/dev/null; then
  printf 'devws restore: another restore is already running\n' >&2
  exit 1
fi
trap 'rmdir "$lock_dir" 2>/dev/null' EXIT

restored=""

valid_choice() {
  local value="$1"
  local list="$2"
  local fallback="$3"
  local candidate

  for candidate in $list; do
    if [ "$candidate" = "$value" ]; then
      printf '%s\n' "$value"
      return 0
    fi
  done
  printf '%s\n' "$fallback"
}

terminal_pane() {
  # @devws_role is authoritative; the title only survives in workspaces created
  # before the terminal pane started marking itself.
  tmux list-panes -t "$1:" \
    -F "#{pane_id}${tab}#{@devws_role}${tab}#{pane_title}" 2>/dev/null |
    awk -F '\t' '
      $2 == "terminal" { print $1; exit }
      $3 == "terminal" { fallback = $1 }
      END { if (fallback != "") print fallback }'
}

picker_pane() {
  tmux list-panes -t "$1:" -F "#{pane_id}${tab}#{pane_title}" 2>/dev/null |
    awk -F '\t' '$2 == "devws-picker" { print $1; exit }'
}

wait_for_workspace() {
  local session="$1"
  local attempt=0

  # tmuxinator creates the panes immediately but their commands run
  # asynchronously, so wait until both the terminal pane and the sidebar have
  # identified themselves. Until then neither can be found reliably.
  # tmuxinator can take upwards of ten seconds to finish building a session.
  while [ "$attempt" -lt 300 ]; do
    if [ -n "$(terminal_pane "$session")" ] && [ -n "$(picker_pane "$session")" ]; then
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  [ -n "$(terminal_pane "$session")" ]
}

pending_session=""
pending_root=""
pending_picker=1
pending_layout=""
pending_panes=0
pending_terminal_cwd=""
pending_extras=""
pending_skip=1

start_workspace() {
  local agent editor

  pending_skip=1
  [ -n "$pending_session" ] || return 0

  if tmux has-session -t "=$pending_session" 2>/dev/null; then
    return 0
  fi
  if [ ! -d "$pending_root" ]; then
    printf 'devws restore: skipped %s: folder is gone: %s\n' \
      "$pending_session" "$pending_root" >&2
    return 0
  fi

  agent="$(valid_choice "$pending_agent" "${DEVWS_AGENTS:-claude codex}" \
    "${DEVWS_DEFAULT_AGENT:-claude}")"
  editor="$(valid_choice "$pending_editor" "${DEVWS_EDITORS:-lvim nvim vim code zed}" \
    "${DEVWS_DEFAULT_EDITOR:-lvim}")"
  [ "$agent" = "$pending_agent" ] ||
    printf 'devws restore: %s: unknown agent "%s"; using %s\n' \
      "$pending_session" "$pending_agent" "$agent" >&2
  [ "$editor" = "$pending_editor" ] ||
    printf 'devws restore: %s: unknown editor "%s"; using %s\n' \
      "$pending_session" "$pending_editor" "$editor" >&2

  # stdin is detached from the record loop's descriptor so the generated
  # tmuxinator script cannot swallow the rest of the state file.
  tmuxinator start --no-attach dev \
    project_root="$pending_root" agent="$agent" editor="$editor" \
    </dev/null >/dev/null 2>&1

  if ! wait_for_workspace "$pending_session"; then
    printf 'devws restore: %s did not finish starting\n' "$pending_session" >&2
    return 0
  fi
  restored="${restored}${pending_session}"$'\n'
  pending_skip=0
}

flush_workspace() {
  local pane cwd picker count added

  [ "$pending_skip" = 0 ] || return 0

  if [ "$pending_picker" = 0 ]; then
    picker="$(picker_pane "$pending_session")"
    [ -n "$picker" ] && tmux kill-pane -t "$picker" 2>/dev/null
  fi

  pane="$(terminal_pane "$pending_session")"
  if [ -n "$pane" ]; then
    printf '%s' "$pending_extras" | while IFS= read -r cwd; do
      [ -n "$cwd" ] || continue
      [ -d "$cwd" ] || cwd="$pending_root"
      added="$(
        tmux split-window -d -h -t "$pane" -c "$cwd" -P -F '#{pane_id}' 2>/dev/null
      )" || continue
      tmux set-option -p -t "$added" @devws_role extra 2>/dev/null
    done
  fi

  if [ -n "$pending_layout" ]; then
    count="$(tmux list-panes -t "$pending_session:" -F 1 2>/dev/null | grep -c .)"
    if [ "$count" = "$pending_panes" ]; then
      tmux select-layout -t "$pending_session:" "$pending_layout" 2>/dev/null
    fi
  fi

  # send-keys is the only way to move a live interactive shell, so it is used
  # only when the terminal pane was not sitting in the workspace root.
  if [ -n "$pane" ] && [ -n "$pending_terminal_cwd" ] &&
      [ "$pending_terminal_cwd" != "$pending_root" ] &&
      [ -d "$pending_terminal_cwd" ]; then
    tmux send-keys -t "$pane" "cd $(printf '%q' "$pending_terminal_cwd")" Enter 2>/dev/null
  fi
}

exec 3< "$state_file"
while IFS="$tab" read -r -u 3 record field2 field3 field4 field5 field6 field7; do
  case "$record" in
    W)
      flush_workspace
      pending_session="$field2"
      pending_root="$field3"
      pending_agent="$field4"
      pending_editor="$field5"
      pending_picker="$field6"
      pending_layout="$field7"
      pending_panes=0
      pending_terminal_cwd=""
      pending_extras=""
      start_workspace
      ;;
    P)
      [ "$field2" = "$pending_session" ] || continue
      pending_panes=$((pending_panes + 1))
      case "$field4" in
        extra) pending_extras="${pending_extras}${field5}"$'\n' ;;
        terminal) pending_terminal_cwd="$field5" ;;
      esac
      ;;
    *) continue ;;
  esac
done
exec 3<&-
flush_workspace

printf '%s' "$restored"
