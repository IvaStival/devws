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
  while IFS="$tab" read -r session root agent editor; do
    case "$session" in dev-*) ;; *) continue ;; esac
    [ -n "$root" ] || continue

    # Target the session's active window rather than a window name: the name is
    # not guaranteed to survive tmux hooks or a non-attached start.
    layout="$(
      tmux display-message -p -t "$session:" '#{window_layout}' 2>/dev/null
    )"
    panes="$(
      tmux list-panes -t "$session:" \
        -F "#{pane_index}${tab}#{@devws_role}${tab}#{pane_title}${tab}#{pane_current_path}" \
        2>/dev/null
    )"
    [ -n "$panes" ] || continue

    picker_open=0
    if printf '%s\n' "$panes" | awk -F '\t' '$3 == "devws-picker"' | grep -q .; then
      picker_open=1
    fi

    printf 'W\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$session" "$root" "$agent" "$editor" "$picker_open" "$layout"

    # Extra terminals inherit neither a title nor @devws_role, so anything that
    # is not one of the four template panes is recorded as "extra". Workspaces
    # started before the terminal pane began marking itself have no terminal
    # role at all; there the first unclaimed pane takes it, so restoring them
    # does not add a duplicate terminal.
    printf '%s\n' "$panes" | awk -F '\t' -v session="$session" '
      NF < 4 { next }
      {
        index_of[NR] = $1
        path_of[NR] = $4
        role = $2
        if (role == "") {
          if ($3 == "devws-picker") { role = "picker" }
          else if ($3 == "terminal") { role = "terminal" }
          else { role = "extra" }
        }
        role_of[NR] = role
        if (role == "terminal") { has_terminal = 1 }
        rows = NR
      }
      END {
        for (row = 1; row <= rows; row++) {
          if (!has_terminal && role_of[row] == "extra") {
            role_of[row] = "terminal"
            has_terminal = 1
          }
          printf "P\t%s\t%s\t%s\t%s\n", session, index_of[row], role_of[row], path_of[row]
        }
      }'
  done < <(
    tmux list-sessions \
      -F "#{session_name}${tab}#{@devws_root}${tab}#{@devws_agent}${tab}#{@devws_editor}" \
      2>/dev/null
  )
} > "$temporary"

# Unchanged state leaves the file and its timestamp alone.
if cmp -s "$temporary" "$state_file" 2>/dev/null; then
  exit 0
fi
mv "$temporary" "$state_file"
trap - EXIT
