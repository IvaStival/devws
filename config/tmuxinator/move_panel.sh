#!/usr/bin/env bash
# Moves the workspace's focused panel one place right, left, down or up: it
# trades places with its neighbour on that side (Move panel in the devws
# menu, C-b Shift+arrow). The layout stays as it is — each place keeps its
# size — and nothing restarts; the panes' roles go with them, and the focus
# stays on the moved panel, so moving again takes it further.
#
#   move_panel.sh <session> <right|left|below|above>
#
# The sidebar stays where it is. A minimized group is expanded first, as its
# panes live in a hidden window meanwhile. The workspace is saved afterwards,
# so a restore brings the new order back.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-}"
direction="${2:-}"

fail() {
  tmux display-message "Move panel: $*" 2>/dev/null
  printf 'Move panel: %s\n' "$*" >&2
  exit 1
}

[ -n "$session" ] || fail "no workspace"
case "$direction" in
  right) target='{right-of}' side="to the right" ;;
  left) target='{left-of}' side="to the left" ;;
  below) target='{down-of}' side="below" ;;
  above) target='{up-of}' side="above" ;;
  *) fail "unknown direction: $direction (right, left, below, above)" ;;
esac

"$script_dir/minimize_panes.sh" "$session" restore all >/dev/null ||
  fail "could not expand the minimized panes"
focused="$(tmux display-message -p -t "=$session:" '#{pane_id}' 2>/dev/null)"
[ -n "$focused" ] || fail "workspace $session not found"
neighbour="$(tmux display-message -p -t "=$session:.$target" '#{pane_id}' 2>/dev/null)"

# tmux's neighbours wrap around the window (left of the leftmost pane is the
# rightmost one) and are the pane itself when alone on that axis: only one
# really on that side counts.
read -r focused_left focused_top <<<"$(tmux display-message -p -t "$focused" '#{pane_left} #{pane_top}')"
read -r neighbour_left neighbour_top <<<"$(tmux display-message -p -t "${neighbour:-$focused}" '#{pane_left} #{pane_top}')"
case "$direction" in
  right) [ "$neighbour_left" -gt "$focused_left" ] ;;
  left) [ "$neighbour_left" -lt "$focused_left" ] ;;
  below) [ "$neighbour_top" -gt "$focused_top" ] ;;
  above) [ "$neighbour_top" -lt "$focused_top" ] ;;
esac || fail "no panel $side"

for pane in "$focused" "$neighbour"; do
  [ "$(tmux display-message -p -t "$pane" '#{pane_title}')" != devws-picker ] ||
    fail "the sidebar stays where it is"
done

# swap-pane leaves the focus on the place, not on the pane: it follows the
# moved one back.
tmux swap-pane -s "$focused" -t "$neighbour" || fail "could not move the panel"
tmux select-pane -t "$focused"

"$script_dir/save_state.sh"
