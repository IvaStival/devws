#!/usr/bin/env bash
# Snapshot every open devws workspace so restore_state.sh can rebuild it after
# a reboot or a tmux/iTerm2 crash.
#
# Records are tab-separated: a "W" line per workspace followed by a "P" line per
# pane, in pane order. Tabs are safe separators because a captured window layout
# contains only digits, "x", ",", "{" and "}".

set -u

state_root="$HOME/.devws-state"
state_file="$state_root/sessions.tsv"
lock_dir="$state_root/restore.lock"
tab="$(printf '\t')"

# A restore replays tmuxinator, which fires the same tmux hooks that call this
# script; saving mid-restore would persist a half-built workspace.
[ -d "$lock_dir" ] && exit 0
tmux list-sessions >/dev/null 2>&1 || exit 0
mkdir -p "$state_root" || exit 0

# Same directory as the destination so the final move is atomic.
temporary="$(mktemp "$state_root/sessions.XXXXXX")" || exit 0
trap 'rm -f "$temporary"' EXIT

{
  printf '#devws-state\t1\n'
  while IFS="$tab" read -r session root agent editor task; do
    case "$session" in dev-*) ;; *) continue ;; esac
    [ -n "$root" ] || continue

    # Target the session's active window rather than a window name: the name is
    # not guaranteed to survive tmux hooks or a non-attached start.
    layout="$(
      tmux display-message -p -t "$session:" '#{window_layout}' 2>/dev/null
    )"
    panes="$(
      tmux list-panes -t "$session:" \
        -F "#{pane_index}${tab}#{@devws_role}${tab}#{pane_title}${tab}#{pane_current_path}${tab}#{@devws_agent}${tab}#{@devws_team}" \
        2>/dev/null
    )"
    [ -n "$panes" ] || continue

    picker_open=0
    if printf '%s\n' "$panes" | awk -F '\t' '$3 == "devws-picker"' | grep -q .; then
      picker_open=1
    fi

    # The task (a program's grouping, `devws open --task`) comes last, so a
    # workspace without one keeps its line as it was.
    printf 'W\t%s\t%s\t%s\t%s\t%s\t%s%s\n' \
      "$session" "$root" "$agent" "$editor" "$picker_open" "$layout" "${task:+$tab$task}"

    # Extra terminals inherit neither a title nor @devws_role, so anything that
    # is not one of the four template panes is recorded as "extra". Workspaces
    # started before the terminal pane began marking itself have no terminal
    # role at all; there the first unclaimed pane takes it, so restoring them
    # does not add a duplicate terminal.
    # A job's log panel (job_panel.sh) is left out: its job is long gone by
    # the time a workspace is restored.
    printf '%s\n' "$panes" | awk -F '\t' -v session="$session" '
      NF < 4 || $2 == "job-log" { next }
      {
        rows++
        index_of[rows] = $1
        path_of[rows] = $4
        agent_of[rows] = $5
        team_of[rows] = $6
        role = $2
        # A minimized pane is recorded as the pane its placeholder stands in for.
        sub(/^minimized-/, "", role)
        if (role == "") {
          if ($3 == "devws-picker") { role = "picker" }
          else if ($3 == "terminal") { role = "terminal" }
          else { role = "extra" }
        }
        role_of[rows] = role
        if (role == "terminal") { has_terminal = 1 }
      }
      END {
        for (row = 1; row <= rows; row++) {
          if (!has_terminal && role_of[row] == "extra") {
            role_of[row] = "terminal"
            has_terminal = 1
          }
          # An extra agent (New panel) also records its agent, and "team" when a
          # program started it (@devws_team): that program restarts it.
          if (role_of[row] == "extra-agent") {
            printf "P\t%s\t%s\t%s\t%s\t%s%s\n", session, index_of[row], role_of[row], path_of[row], agent_of[row], (team_of[row] == 1 ? "\tteam" : "")
          } else {
            printf "P\t%s\t%s\t%s\t%s\n", session, index_of[row], role_of[row], path_of[row]
          }
        }
      }'
  done < <(
    tmux list-sessions \
      -F "#{session_name}${tab}#{@devws_root}${tab}#{@devws_agent}${tab}#{@devws_editor}${tab}#{@devws_task}" \
      2>/dev/null
  )
} > "$temporary"

# Unchanged state leaves the file and its timestamp alone.
if cmp -s "$temporary" "$state_file" 2>/dev/null; then
  exit 0
fi
mv "$temporary" "$state_file"
trap - EXIT
