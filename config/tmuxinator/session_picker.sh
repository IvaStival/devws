#!/usr/bin/env bash
# Persistent devws sidebar: workspace switcher, plus the entry that opens the
# devws menu of workspace actions (devws_menu.sh).

set -u

# Absolute: fzf runs it again (--rows) from wherever it is.
picker_script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

if ! command -v fzf >/dev/null 2>&1; then
  printf 'devws picker: fzf is required; run the devws installer and restart tmux.\n' >&2
  exit 127
fi

# The rows are drawn into rows_file with the spinner on its first frame (⠋).
# While fzf runs, nothing in this script polls: others talk to fzf through its
# socket (@devws_picker_socket). refresh_devws_pickers.sh --reload has it run
# "session_picker.sh --rows" when an agent's state changes, which redraws the
# rows in place, and agent_spinner.sh swaps the next spinner frame into them
# while the sidebar is on screen; so the git lookups run once per redraw and
# the cursor stays. A reload that came before fzf listened leaves
# rows_file.stale, which fzf checks once it starts.
#
#   session_picker.sh          the sidebar
#   session_picker.sh --rows   (fzf, internal) redraws the rows and prints them
print_padded_row() {
  local style="$1"
  local width="$2"
  local text="$3"
  local padding=$((width - ${#text}))

  (( padding < 0 )) && padding=0
  printf '%b%s%*s\033[0m' "$style" "$text" "$padding" ""
}

workspace_rows() {
  local current="$1"
  local session agent editor root agent_state branch state state_plain row_width
  local activity activity_plain

  row_width="$(
    tmux display-message -p -t "$TMUX_PANE" '#{pane_width}' 2>/dev/null
  )"
  case "$row_width" in
    ''|*[!0-9]*) row_width=80 ;;
  esac
  (( row_width > 2 )) && row_width=$((row_width - 2))

  while IFS='|' read -r session agent editor root agent_state; do
    case "$session" in dev-*) ;; *) continue ;; esac
    agent="${agent:-unknown}"
    editor="${editor:-unknown}"

    # The agent's state (agent_status.sh); animate_spinner turns the ⠋.
    case "$agent_state" in
      working) activity=" ⠋"; activity_plain=" ⠋" ;;
      waiting) activity=" \033[33m● needs you\033[0;2m"; activity_plain=" ● needs you" ;;
      done) activity=" \033[32m✓ done\033[0;2m"; activity_plain=" ✓ done" ;;
      *) activity=""; activity_plain="" ;;
    esac

    branch="—"
    state="  "
    state_plain="  "
    if [ -n "$root" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      branch="$(git -C "$root" branch --show-current 2>/dev/null)"
      [ -n "$branch" ] || branch="detached"
      if [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ]; then
        state="\033[33m●\033[0m "
        state_plain="● "
      else
        state="\033[32m✓\033[0m "
        state_plain="✓ "
      fi
    fi

    if [ "$session" = "$current" ]; then
      printf 'workspace:%s\t' "$session"
      print_padded_row '\033[1;37;48;5;238m' \
        "$row_width" "$state_plain  $session"
      printf '\n'
      print_padded_row '\033[2;37;48;5;238m' "$row_width" "    $branch"
      printf '\n'
      print_padded_row '\033[2;37;48;5;238m' \
        "$row_width" "    $agent$activity_plain · $editor"
      printf '\0'
    else
      printf 'workspace:%s\t%b  %s\033[0m' "$session" "$state" "$session"
      printf '\n    \033[2m%s\033[0m' "$branch"
      printf '\n    \033[2m%s%b · %s\033[0m\0' "$agent" "$activity" "$editor"
    fi
  done < <(tmux list-sessions \
    -F '#{session_name}|#{@devws_agent}|#{@devws_editor}|#{@devws_root}|#{@devws_agent_state}' 2>/dev/null)
}

# Swapped in whole, so a spinner frame never reads half of the rows.
write_rows() {
  workspace_rows "$(tmux display-message -p '#S' 2>/dev/null)" > "$rows_file.new"
  mv "$rows_file.new" "$rows_file"
}

if [ "${1:-}" = --rows ]; then
  rows_file="${DEVWS_PICKER_ROWS:?}"
  rm -f "$rows_file.stale"
  write_rows
  cat "$rows_file"
  exit 0
fi

# Always title this script's own pane. Without an explicit target tmux titles
# the client's active pane, which may be the editor or terminal.
tmux select-pane -t "$TMUX_PANE" -T devws-picker 2>/dev/null

tmp_dir="${TMPDIR:-/tmp}"
rows_file="$(mktemp "${tmp_dir%/}/devws-picker.XXXXXX")"
# Where refresh_devws_pickers.sh --reload marks the rows stale.
tmux set-option -p -t "$TMUX_PANE" @devws_picker_rows "$rows_file" 2>/dev/null
# Short and in TMPDIR: a Unix socket path is limited to about 100 bytes.
fzf_socket="${tmp_dir%/}/devws-picker-$$.sock"
tmux set-option -p -t "$TMUX_PANE" @devws_picker_socket "$fzf_socket" 2>/dev/null
# fzf runs "--rows" with its environment.
export DEVWS_PICKER_ROWS="$rows_file"
trap 'rm -f "$rows_file" "$rows_file.stale" "$fzf_socket"' EXIT

while tmux list-sessions >/dev/null 2>&1; do
  current="$(tmux display-message -p '#S' 2>/dev/null)"
  current_position="$(
    tmux list-sessions -F '#{session_name}' 2>/dev/null |
      awk -v current="$current" '
        /^dev-/ { position++ }
        $0 == current { print position; exit }
      '
  )"
  [ -n "$current_position" ] || current_position=1
  rm -f "$rows_file.stale"
  write_rows
  selected="$(
    fzf --height=100% --layout=reverse --ansi --read0 --gap=1 \
        --highlight-line \
        --listen="$fzf_socket" \
        --color='bg+:244,fg+:255' \
        --delimiter=$'\t' --with-nth=2.. \
        --bind="load:pos($current_position)+unbind(load)" \
        --bind="start:transform:[ -e '$rows_file.stale' ] && printf 'reload-sync(%q --rows)' '$picker_script'" \
        --bind='click-header:transform:
          [ "$FZF_CLICK_HEADER_LINE" = 1 ] && printf "print(menu)+accept\n"' \
        --header=$'\033[36m☰ Menu\033[0m  \033[2mC-b Space\033[0m\n\n\033[2mOPEN WORKSPACES\033[0m' \
        --prompt='> ' \
        --info=inline --no-separator --border=none --margin=0 --padding=0 \
        < "$rows_file"
  )"
  # Header actions add their command before fzf's current workspace output.
  selected="${selected%%$'\n'*}"

  case "${selected%%$'\t'*}" in
    # Every workspace action is in the tabbed menu (devws_menu.sh).
    menu) "$HOME/.tmuxinator/devws_menu.sh" --popup ;;
    '') ;;
    workspace:*)
      workspace="${selected%%$'\t'*}"
      tmux switch-client -t "${workspace#workspace:}" 2>/dev/null
      ;;
  esac
done
