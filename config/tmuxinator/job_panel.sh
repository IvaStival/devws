#!/usr/bin/env bash
# The log panel: a background job's live log (job.sh follow), docked below the
# terminal of the workspace that started it — at the window's bottom while the
# terminal is minimized. It never takes the focus; one per workspace, on its
# newest job. Its header shows the job (the pane option @devws_job_title,
# which tmux.conf's pane-border-format shows for the role job-log).
#
#   job_panel.sh open <session> <id>   opens it on job <id> (job.sh, once the
#                                      job has run a few seconds; C-b j)
#   job_panel.sh close <session>       closes it (layout toggle, minimize)
#
# It is never saved (save_state.sh skips it): its job is long gone by then.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=workspace_panes.sh
. "$script_dir/workspace_panes.sh"
lines=12

panel_of() {
  tmux list-panes -s -t "=$1:" -F '#{pane_id}|#{@devws_role}' 2>/dev/null |
    awk -F '|' '$2 == "job-log" { print $1; exit }'
}

open() {
  local session="$1" id="$2" command pane terminal
  tmux has-session -t "=$session" 2>/dev/null || return 0
  # The jobs folder goes along: the pane starts with the server's environment.
  command="$(printf 'DEVWS_JOBS_DIR=%q %q follow %q' \
    "${DEVWS_JOBS_DIR:-$HOME/.devws-state/jobs}" "$script_dir/job.sh" "$id")"
  pane="$(panel_of "$session")"
  if [ -n "$pane" ]; then
    tmux respawn-pane -k -t "$pane" "$command"
    return
  fi
  load_workspace_panes "$session"
  terminal="$(pane_with_role terminal)"
  if [ -n "$terminal" ]; then
    pane="$(tmux split-window -d -v -l "$lines" -t "$terminal" -P -F '#{pane_id}' "$command")" || return
  else
    pane="$(tmux split-window -d -f -v -l "$lines" -t "=$session:" -P -F '#{pane_id}' "$command")" || return
  fi
  tmux set-option -p -t "$pane" @devws_role job-log
}

close() {
  local pane
  pane="$(panel_of "$1")"
  [ -z "$pane" ] || tmux kill-pane -t "$pane"
}

case "${1:-}" in
  open) [ "$#" -eq 3 ] || { printf 'usage: job_panel.sh open <session> <id>\n' >&2; exit 2; }; open "$2" "$3" ;;
  close) [ "$#" -eq 2 ] || { printf 'usage: job_panel.sh close <session>\n' >&2; exit 2; }; close "$2" ;;
  *) printf 'usage: job_panel.sh <open|close> …\n' >&2; exit 2 ;;
esac
