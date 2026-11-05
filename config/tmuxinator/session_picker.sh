#!/usr/bin/env bash
# Persistent devws sidebar: workspace switcher, plus the entry that opens the
# devws menu of workspace actions (devws_menu.sh).

set -u

# Absolute: fzf runs it again (--rows) from wherever it is.
picker_script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
tab="$(printf '\t')"

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
#   session_picker.sh --position <workspace>
#                              (internal) its place among the rows, which the
#                              task headings count in
#
# A workspace with a task (@devws_task, set by `devws open --task`) is listed
# under its task's heading, after the workspaces without one. An agent team's
# agents (@devagentics_agent) are listed under their workspace; Enter shows
# one (team_groups.sh go).
print_padded_row() {
  local style="$1"
  local width="$2"
  local text="$3"
  local padding=$((width - ${#text}))

  (( padding < 0 )) && padding=0
  printf '%b%s%*s\033[0m' "$style" "$text" "$padding" ""
}

# The rows in order, as "workspace<TAB><session>", "task<TAB><id>",
# "group<TAB><session><TAB><place>" and "agent<TAB><pane><TAB><place>": the
# workspaces without a task, then each task's heading and its workspaces. An
# agent team's workspace is followed by its peeked agents (place "peeked"),
# then a tree of its groups' workspaces (team_groups.sh), each with its
# agents; the last group's rows have place "last", the others "mid". The
# order the rows are drawn in and fzf's cursor is placed by.
row_order() {
  local kind name groups group place index
  local -a team_groups
  groups="$(group_order)"
  workspace_order | while IFS="$tab" read -r kind name; do
    printf '%s\t%s\n' "$kind" "$name"
    [ "$kind" = workspace ] || continue
    agents_in "$name" | sed "s/^/agent${tab}/; s/\$/${tab}peeked/"
    team_groups=()
    while IFS="$tab" read -r _ group; do
      [ -n "$group" ] && team_groups+=("$group")
    done < <(awk -F '\t' -v team="$name" '$1 == team' <<<"$groups")
    for index in "${!team_groups[@]}"; do
      place=mid
      [ "$index" -eq $((${#team_groups[@]} - 1)) ] && place=last
      printf 'group\t%s\t%s\n' "${team_groups[$index]}" "$place"
      agents_in "${team_groups[$index]}" | sed "s/^/agent${tab}/; s/\$/${tab}${place}/"
    done
  done
}

# A workspace's team agents (@devagentics_agent), one pane per line.
agents_in() {
  tmux list-panes -s -t "=$1" -F "#{pane_id}${tab}#{@devagentics_agent}" 2>/dev/null |
    awk -F '\t' '$2 != "" { print $1 }'
}

# "<team><TAB><group workspace>" for the groups of teams that are open:
# Seniors first, then the Floor's (Floor, Floor 2…), then any other.
group_order() {
  tmux list-sessions -F "#{session_name}${tab}#{@devws_team_of}${tab}#{@devws_group}" 2>/dev/null |
    awk -F '\t' '
      { open[$1] = 1; team[NR] = $2; group[NR] = $3; name[NR] = $1 }
      END {
        for (row = 1; row <= NR; row++) {
          if (team[row] == "" || !(team[row] in open)) continue
          rank = tolower(group[row]) == "seniors" ? 1 : (tolower(group[row]) == "floor" ? 2 : 3)
          printf "%s\t%s\t%s\t%s\n", team[row], rank, group[row], name[row]
        }
      }' | LC_ALL=C sort -t "$tab" -k1,1 -k2,2n -k3,3 -k4,4 | cut -f1,4
}

# The workspaces and task headings; a group's workspace is not one of them
# while its team's is open (row_order lists it under the team).
workspace_order() {
  tmux list-sessions -F "#{session_name}${tab}#{@devws_task}${tab}#{@devws_team_of}" 2>/dev/null |
    awk -F '\t' '
      { open[$1] = 1; rows[NR] = $0 }
      END {
        for (row = 1; row <= NR; row++) {
          split(rows[row], field, "\t")
          if (field[3] != "" && (field[3] in open)) continue
          print field[1] "\t" field[2]
        }
      }' |
    awk -F '\t' '
      $1 !~ /^dev-/ { next }
      $2 == "" { plain[++plains] = $1; next }
      {
        if (!($2 in members)) order[++tasks] = $2
        members[$2] = members[$2] $1 "\t"
      }
      END {
        for (i = 1; i <= plains; i++) print "workspace\t" plain[i]
        for (t = 1; t <= tasks; t++) {
          print "task\t" order[t]
          n = split(members[order[t]], list, "\t")
          for (i = 1; i < n; i++) print "workspace\t" list[i]
        }
      }'
}

# An agent of a team: its id and state, in the tree of its team's groups, or
# under the team's workspace when peeked there.
agent_row() {
  local pane="$1" place="$2" id agent_state mark
  IFS='|' read -r id agent_state <<< "$(tmux display-message -p -t "$pane" \
    '#{@devagentics_agent}|#{@devws_pane_state}' 2>/dev/null)"
  case "$agent_state" in
    working) mark=" ⠋" ;;
    waiting) mark=" \033[33m● needs you\033[0;2m" ;;
    done) mark=" \033[32m✓ done\033[0;2m" ;;
    *) mark="" ;;
  esac
  case "$place" in
    peeked) printf 'agent:%s\t    \033[2m◦ %s%b · peeked\033[0m\0' "$pane" "$id" "$mark" ;;
    last) printf 'agent:%s\t       \033[2m◦ %s%b\033[0m\0' "$pane" "$id" "$mark" ;;
    *) printf 'agent:%s\t    \033[2m│  ◦ %s%b\033[0m\0' "$pane" "$id" "$mark" ;;
  esac
}

# A group's workspace in its team's tree: ├ Seniors, └ Floor 2 for the last;
# ● while one of its agents waits; highlighted when you are in it.
group_row() {
  local session="$1" place="$2" current="$3" row_width="$4" team group alert number branch="├" mark="" mark_plain=""
  IFS='|' read -r team group alert <<< "$(tmux display-message -p -t "=$session:" \
    '#{@devws_team_of}|#{@devws_group}|#{@devws_tab_alert}' 2>/dev/null)"
  # dev-app-floor-2 is Floor 2 (team_groups.sh names them so).
  number="${session#"$team-$(printf '%s' "$group" | tr '[:upper:] ' '[:lower:]-')"}"
  case "$number" in -[0-9]*) group="$group ${number#-}" ;; esac
  [ "$place" = last ] && branch="└"
  [ -n "$alert" ] && mark=" \033[33m●\033[0m" mark_plain=" ●"
  printf 'group:%s\t' "$session"
  if [ "$session" = "$current" ]; then
    print_padded_row '\033[1;37;48;5;238m' "$row_width" "    $branch $group$mark_plain"
  else
    printf '    \033[2m%s\033[0m %s%b' "$branch" "$group" "$mark"
  fi
  printf '\0'
}

workspace_rows() {
  local current="$1"
  local kind session agent editor root agent_state branch state state_plain row_width
  local activity activity_plain pr_file pr_state

  row_width="$(
    tmux display-message -p -t "$TMUX_PANE" '#{pane_width}' 2>/dev/null
  )"
  case "$row_width" in
    ''|*[!0-9]*) row_width=80 ;;
  esac
  (( row_width > 2 )) && row_width=$((row_width - 2))

  while IFS="$tab" read -r kind session place; do
    if [ "$kind" = task ]; then
      printf 'task:%s\t\033[36m▸ %s\033[0m\0' "$session" "$session"
      continue
    fi
    if [ "$kind" = agent ]; then
      agent_row "$session" "$place"
      continue
    fi
    if [ "$kind" = group ]; then
      group_row "$session" "$place" "$current" "$row_width"
      continue
    fi
    IFS='|' read -r agent editor root agent_state frozen needs_reopen <<< "$(tmux display-message -p -t "=$session:" \
      '#{@devws_agent}|#{@devws_editor}|#{@devws_root}|#{@devws_agent_state}|#{@devws_frozen}|#{@devws_needs_reopen}' 2>/dev/null)"
    agent="${agent:-unknown}"
    editor="${editor:-unknown}"

    # The agent's state (agent_status.sh); animate_spinner turns the ⠋.
    case "$agent_state" in
      working) activity=" ⠋"; activity_plain=" ⠋" ;;
      waiting) activity=" \033[33m● needs you\033[0;2m"; activity_plain=" ● needs you" ;;
      done) activity=" \033[32m✓ done\033[0;2m"; activity_plain=" ✓ done" ;;
      *) activity=""; activity_plain="" ;;
    esac
    # Frozen while idle (suspend_workspace.sh) until you switch to it.
    # A mark to reopen it: its editor predates the runners and has unsaved
    # changes, or can't be asked, so only closing and reopening the workspace
    # lets the editor pause too.
    pause_mark=""
    if [ -n "$frozen" ] && [ -n "$needs_reopen" ]; then
      pause_mark=" · partly paused"
    elif [ -n "$frozen" ]; then
      pause_mark=" · paused"
    elif [ -n "$needs_reopen" ]; then
      pause_mark=" · reopen to pause"
    fi
    activity="$activity$pause_mark"
    activity_plain="$activity_plain$pause_mark"

    branch="—"
    state="  "
    state_plain="  "
    if [ -n "$root" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      branch="$(git -C "$root" branch --show-current 2>/dev/null)"
      [ -n "$branch" ] || branch="detached"
      # The PR state github_repo.sh keeps in the worktree's git folder, read
      # here directly: the sidebar redraws often and never waits on gh.
      pr_file="$(git -C "$root" rev-parse --path-format=absolute --git-dir 2>/dev/null)/devws-pr"
      if [ -r "$pr_file" ]; then
        read -r _ pr_state < "$pr_file"
        case "${pr_state:-none}" in
          none) ;;
          *) branch="$branch · PR $pr_state" ;;
        esac
      fi
      # The jungleforge apps run from this folder (refresh_states).
      grep -qxF "$root" "$running_file" 2>/dev/null && branch="$branch · ▶ running"
      pr_roots+=("$root")
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
  done < <(row_order)
}

# The folders jungleforge's apps run from (jungleforge.sh running-folders),
# one per line, shared by every sidebar.
tmp_dir="${TMPDIR:-/tmp}"
running_file="${tmp_dir%/}/devws-running-$(id -u)"

# A background refresh that fails says so in the header (an error note), at
# most once an hour: it runs every 30 s, and the same error would flood it.
report_refresh_errors() {
  local errors="$1" stamp="$1.reported" reported
  [ -s "$errors" ] || return 0
  reported="$(cat "$stamp" 2>/dev/null)"
  case "$reported" in ''|*[!0-9]*) reported=0 ;; esac
  [ $(($(date +%s) - reported)) -ge 3600 ] || return 0
  date +%s > "$stamp"
  "$HOME/.tmuxinator/job.sh" note "Sidebar refresh failed: $(head -n 1 "$errors")" --error >/dev/null 2>&1
}

# The workspaces' PR states and running apps are asked again in the
# background, at most every 30 seconds across all sidebars (github_repo.sh
# keeps each PR state for five minutes); a change redraws the sidebars. Run
# and Stop clear @devws_states_checked to have it asked at once.
refresh_states() {
  local checked now
  [ "${#pr_roots[@]}" -gt 0 ] || return 0
  checked="$(tmux show-option -gqv @devws_states_checked 2>/dev/null)"
  now="$(date +%s)"
  case "$checked" in ''|*[!0-9]*) checked=0 ;; esac
  [ $((now - checked)) -ge 30 ] || return 0
  tmux set-option -g @devws_states_checked "$now" 2>/dev/null
  (
    changed=1
    "$HOME/.tmuxinator/github_repo.sh" pr-refresh "${pr_roots[@]}" && changed=0
    running="$("$HOME/.tmuxinator/jungleforge.sh" running-folders "${pr_roots[@]}" 2>"$running_file.errors")"
    report_refresh_errors "$running_file.errors"
    if [ "$running" != "$(cat "$running_file" 2>/dev/null)" ]; then
      printf '%s\n' "$running" > "$running_file"
      changed=0
    fi
    [ "$changed" = 1 ] || "$HOME/.tmuxinator/refresh_devws_pickers.sh" --reload
  ) </dev/null >/dev/null 2>&1 &
}

# Swapped in whole, so a spinner frame never reads half of the rows.
write_rows() {
  pr_roots=()
  # Not a subshell: the rows collect pr_roots.
  workspace_rows "$(tmux display-message -p '#S' 2>/dev/null)" > "$rows_file.new"
  mv "$rows_file.new" "$rows_file"
  refresh_states
}

if [ "${1:-}" = --position ]; then
  row_order | awk -F '\t' -v workspace="${2:-}" '($1 == "workspace" || $1 == "group") && $2 == workspace { print NR; exit }'
  exit 0
fi

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
  current_position="$("$picker_script" --position "$current")"
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
    # A group of an agent team: its workspace.
    group:*)
      workspace="${selected%%$'\t'*}"
      tmux switch-client -t "${workspace#group:}" 2>/dev/null
      ;;
    # An agent of a team: its group's workspace, the agent.
    agent:*)
      agent="${selected%%$'\t'*}"
      tmux switch-client -t "${agent#agent:}" 2>/dev/null
      "$HOME/.tmuxinator/team_groups.sh" go "${agent#agent:}" 2>/dev/null
      ;;
    # A task's heading takes you to its first workspace.
    task:*)
      task="${selected%%$'\t'*}"
      workspace="$(row_order | awk -F '\t' -v task="${task#task:}" '
        $1 == "task" && $2 == task { found = 1; next }
        found && $1 == "workspace" { print $2; exit }
        found { exit }')"
      [ -z "$workspace" ] || tmux switch-client -t "$workspace" 2>/dev/null
      ;;
  esac
done
