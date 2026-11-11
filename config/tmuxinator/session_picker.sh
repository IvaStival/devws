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
# rows in place. Between redraws, agent_spinner.sh turns the ⠋ of a sidebar on
# screen by reloading the drawn rows with each frame. fzf's info line is
# hidden: it animates a spinner of its own, ten frames a second, on reloads.
# So the git lookups run once per redraw and the cursor stays. A reload that came before fzf listened leaves
# rows_file.stale, which fzf checks once it starts.
#
#   session_picker.sh          the sidebar
#   session_picker.sh --rows   (fzf, internal) redraws the rows and prints them
#   session_picker.sh --position <workspace>
#                              (internal) its place among the rows, which the
#                              task headings count in
#   session_picker.sh --task-workspace <task>
#                              (internal) resolve a task after a live reload
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

# Expansion belongs to this picker pane; fzf reloads run as fresh processes.
picker_pane="${TMUX_PANE:-$(tmux display-message -p '#{pane_id}' 2>/dev/null)}"
# DEVWS_PICKER_CURRENT overrides the active workspace for tests.
picker_current="${DEVWS_PICKER_CURRENT:-$(tmux display-message -p '#S' 2>/dev/null)}"
picker_option() {
  tmux show-option -pqv -t "$picker_pane" "$1" 2>/dev/null
}
set_picker_option() {
  tmux set-option -p -t "$picker_pane" "$1" "$2" 2>/dev/null
}
set_expanded() {
  local key="$1" value="$2" entries
  entries="$(picker_option @devws_picker_expansion | awk -F '\t' -v key="$key" '$1 != key')"
  set_picker_option @devws_picker_expansion "$(printf '%s\n%s\t%s\n' "$entries" "$key" "$value")"
}
# Teams and groups start expanded, so every agent shows wherever you enter a
# team from; only a manual collapse hides them.
is_expanded() {
  local value
  [ -z "${picker_query:-}" ] || return 0
  value="$(printf '%s\n' "$picker_expansion" | awk -F '\t' -v key="$1" '$1 == key { print $2 }')"
  [ "$value" != closed ]
}
prepare_expansion() {
  local previous team
  session_snapshot="$(tmux list-sessions -F "#{session_name}${tab}#{@devws_task}${tab}#{@devws_team_of}${tab}#{@devws_group}${tab}#{@devws_root}${tab}#{@devws_template}${tab}#{@devws_editor}" 2>/dev/null)"
  group_snapshot="$(group_order)"
  pane_snapshot="$(tmux list-panes -a -F "#{session_name}${tab}#{pane_id}${tab}#{@devagentics_agent}${tab}#{@devws_pane_state}${tab}#{@devws_role}${tab}#{@devws_model}${tab}#{@devws_model_configured}" 2>/dev/null)"
  team_snapshot="$(team_sessions)"
  previous="$(picker_option @devws_picker_current)"
  if [ "$previous" != "$picker_current" ]; then
    set_picker_option @devws_picker_current "$picker_current"
    set_expanded "$picker_current" open
    team="$(tmux display-message -p -t "=$picker_current:" '#{@devws_team_of}' 2>/dev/null)"
    [ -z "$team" ] || set_expanded "$team" open
  fi
  picker_query="$(picker_option @devws_picker_query)"
  picker_expansion="$(picker_option @devws_picker_expansion)"
}

# Hidden search text lets fzf retain parent paths when a descendant matches.
search_text() {
  printf '%s ' "$1"
  printf '%s\n' "$session_snapshot" | awk -F '\t' -v session="$1" '$1 == session || $3 == session { printf "%s ", $4 }'
  team_panes "$1" | awk -F '\t' '{ printf "%s %s %s %s %s ", $1, $3, $4, $6, $7 }'
}
# Both the summary and search index use one pane snapshot per redraw, rather
# than adding tmux requests for each workspace, group, and descendant.
team_panes() {
  printf '%s\n' "$pane_snapshot" | group_rows="$group_snapshot" awk -F '\t' -v session="$1" '
    BEGIN {
      members[session] = 1
      count = split(ENVIRON["group_rows"], rows, "\n")
      for (row_index = 1; row_index <= count; row_index++) {
        split(rows[row_index], fields, "\t")
        if (fields[1] == session) members[fields[2]] = 1
      }
    }
    $1 in members'
}
activity_counts() {
  team_panes "$1" | awk -F '\t' '
    $4 == "working" { working++ }
    $4 == "waiting" { waiting++ }
    END {
      if (working) printf " ⠋%d", working
      if (waiting) printf " ●%d", waiting
    }'
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
  groups="$group_snapshot"
  workspace_order | while IFS="$tab" read -r kind name place; do
    printf '%s\t%s\t%s\n' "$kind" "$name" "$place"
    [ "$kind" = workspace ] || continue
    is_expanded "$name" || continue
    agents_in "$name" | sed "s/^/agent${tab}/; s/\$/${tab}peeked/"
    team_groups=()
    while IFS="$tab" read -r _ group; do
      [ -n "$group" ] && team_groups+=("$group")
    done < <(awk -F '\t' -v team="$name" '$1 == team' <<<"$groups")
    for index in "${!team_groups[@]}"; do
      place=mid
      [ "$index" -eq $((${#team_groups[@]} - 1)) ] && place=last
      printf 'group\t%s\t%s\n' "${team_groups[$index]}" "$place"
      is_expanded "${team_groups[$index]}" || continue
      agents_in "${team_groups[$index]}" | sed "s/^/agent${tab}/; s/\$/${tab}${place}/"
    done
  done
}

# A workspace's team agents (@devagentics_agent), one pane per line.
agents_in() {
  printf '%s\n' "$pane_snapshot" | awk -F '\t' -v session="$1" '$1 == session && $3 != "" { print $2 }'
}

# "<team><TAB><group workspace>" for the groups of teams that are open:
# Seniors first, then the Floor's (Floor, Floor 2…), then any other.
group_order() {
  printf '%s\n' "$session_snapshot" |
    awk -F '\t' '
      { open[$1] = 1; team[NR] = $3; group[NR] = $4; name[NR] = $1 }
      END {
        for (row = 1; row <= NR; row++) {
          if (team[row] == "" || !(team[row] in open)) continue
          rank = tolower(group[row]) == "seniors" ? 1 : (tolower(group[row]) == "floor" ? 2 : 3)
          printf "%s\t%s\t%s\t%s\n", team[row], rank, group[row], name[row]
        }
      }' | LC_ALL=C sort -t "$tab" -k1,1 -k2,2n -k3,3 -k4,4 | cut -f1,4
}

# The workspaces and task headings; a group's workspace is not one of them
# while its team's is open (row_order lists it under the team). A task with an
# agent team has no heading of its own: the team's workspace leads it (place
# "heading"), so the project is not listed twice.
workspace_order() {
  printf '%s\n' "$session_snapshot" |
    awk -F '\t' '
      { open[$1] = 1; rows[NR] = $0 }
      END {
        for (row = 1; row <= NR; row++) {
          split(rows[row], field, "\t")
          if (field[3] != "" && (field[3] in open)) continue
          print field[1] "\t" field[2]
        }
      }' |
    teams="$team_snapshot" awk -F '\t' '
      BEGIN {
        count = split(ENVIRON["teams"], team_list, "\n")
        for (i = 1; i <= count; i++) team[team_list[i]] = 1
      }
      $1 !~ /^dev-/ { next }
      $2 == "" { plain[++plains] = $1; next }
      {
        if (!seen[$2]++) order[++tasks] = $2
        if (($1 in team) && !($2 in leader)) leader[$2] = $1
        else members[$2] = members[$2] $1 "\t"
      }
      END {
        for (i = 1; i <= plains; i++) print "workspace\t" plain[i]
        for (t = 1; t <= tasks; t++) {
          if (order[t] in leader) print "workspace\t" leader[order[t]] "\theading"
          else print "task\t" order[t]
          n = split(members[order[t]], list, "\t")
          for (i = 1; i < n; i++) print "workspace\t" list[i]
        }
      }'
}

# The agent teams' workspaces, one per line: started from the agentic
# template, holding the team's log, or with groups of their own.
team_sessions() {
  {
    printf '%s\n' "$session_snapshot" | awk -F '\t' '$6 == "agentic" || $7 == "agent team" { print $1 } $3 != "" { print $3 }'
    printf '%s\n' "$pane_snapshot" | awk -F '\t' '$5 == "team-log" { print $1 }'
  } | sort -u
}

# The project a task's workspaces are in: their folders' names.
task_project() {
  printf '%s\n' "$session_snapshot" | awk -F '\t' -v task="$1" '
    $2 == task && $5 != "" && !seen[$5]++ { sub(/\/+$/, "", $5); count = split($5, parts, "/"); names = names sep parts[count]; sep = ", " }
    END { print names }'
}

truncate_label() {
  local text="$1" room="$2"
  if [ "$room" -le 0 ]; then return; fi
  if [ "${#text}" -gt "$room" ]; then
    printf '%s…' "${text:0:$((room - 1))}"
  else
    printf '%s' "$text"
  fi
}

# Model metadata has its own dim line and never consumes the name's budget.
# Wrap long model identifiers instead of silently clipping their version suffix.
# The prefix begins each line: spaces, or a tree's guide (`    │    `) so the
# tree stays drawn down to the next group.
model_lines() {
  local model="$1" prefix="$2" room="$3" content_width
  [ -n "$model" ] || return 0
  (( ${#prefix} < room )) || prefix=""
  content_width=$((room - ${#prefix}))
  (( content_width > 0 )) || content_width=1
  while [ -n "$model" ]; do
    printf '\n\033[2m%s%s\033[0m' "$prefix" "${model:0:content_width}"
    model="${model:content_width}"
  done
}

# An agent of a team: its id and state, in the tree of its team's groups, or
# under the team's workspace when peeked there.
agent_row() {
  local pane="$1" place="$2" id agent_state mark mark_plain session team model prefix label
  IFS='|' read -r session id agent_state model <<< "$(printf '%s\n' "$pane_snapshot" |
    awk -F '\t' -v pane="$pane" '$2 == pane { print $1 "|" $3 "|" $4 "|" ($6 != "" ? $6 : $7) }')"
  case "$agent_state" in
    working) mark=" ⠋"; mark_plain=" ⠋" ;;
    waiting) mark=" \033[33m● needs you\033[0;2m"; mark_plain=" ● needs you" ;;
    done) mark=" \033[32m✓ done\033[0;2m"; mark_plain=" ✓ done" ;;
    *) mark=""; mark_plain="" ;;
  esac
  team="$(printf '%s\n' "$group_snapshot" | awk -F '\t' -v session="$session" '$2 == session { print $1 }')"
  case "$place" in
    peeked) prefix="    ◦ " ;;
    last) prefix="       ◦ " ;;
    *) prefix="    │  ◦ " ;;
  esac
  if [ "${row_width:-80}" -lt 32 ]; then
    prefix="    "
    case "$agent_state" in
      working) mark=" ⠋"; mark_plain=" ⠋" ;;
      waiting) mark=" \033[33m●\033[0m"; mark_plain=" ●" ;;
      done) mark=" \033[32m✓\033[0m"; mark_plain=" ✓" ;;
    esac
  fi
  label="$(truncate_label "$id" "$((${row_width:-80} - ${#prefix} - ${#mark_plain}))")"
  printf 'agent:%s\t\033[2m%s\033[0m%s%b\033[0m' "$pane" "$prefix" "$label" "$mark"
  # Under a group with another below it, the model line carries the │ on.
  local model_prefix
  model_prefix="$(printf '%*s' "${#prefix}" '')"
  [[ "$prefix" == "    │"* ]] && model_prefix="    │${model_prefix:5}"
  model_lines "$model" "$model_prefix" "${row_width:-80}"
  printf '\t%s %s %s %s %s ' "$session" "$team" "$id" "$agent_state" "$model"
  printf '%s\n' "$session_snapshot" | awk -F '\t' -v session="$session" '$1 == session { printf "%s", $4 }'
  printf '\0'
}

# A group's workspace in its team's tree: ├ Seniors, └ Floor 2 for the last;
# ● while one of its agents waits; highlighted when you are in it.
group_row() {
  local session="$1" place="$2" current="$3" row_width="$4" team group alert number branch="├" mark="" mark_plain="" indent="    "
  IFS='|' read -r team group alert <<< "$(tmux display-message -p -t "=$session:" \
    '#{@devws_team_of}|#{@devws_group}|#{@devws_tab_alert}' 2>/dev/null)"
  # dev-app-floor-2 is Floor 2 (team_groups.sh names them so).
  number="${session#"$team-$(printf '%s' "$group" | tr '[:upper:] ' '[:lower:]-')"}"
  case "$number" in -[0-9]*) group="$group ${number#-}" ;; esac
  [ "$place" = last ] && branch="└"
  if is_expanded "$session"; then group="▾ $group"; else group="▸ $group"; fi
  mark_plain="$(activity_counts "$session")"
  mark="$mark_plain"
  [ -n "$mark_plain" ] || { [ -z "$alert" ] || { mark=" ●"; mark_plain=" ●"; }; }
  [ "$row_width" -ge 32 ] || indent="  "
  printf 'group:%s\t' "$session"
  if [ "$session" = "$current" ]; then
    print_padded_row '\033[1;37;48;5;238m' "$row_width" "$indent$branch $group$mark_plain"
  else
    printf '%s\033[2m%s\033[0m %s%b' "$indent" "$branch" "$group" "$mark"
  fi
  printf '\t%s\0' "$(search_text "$session")"
}

# Normal development workspaces retain their branch, repository state and
# agent/editor details. Team trees use the compact renderer below.
normal_workspace_row() {
  local session="$1" current="$2" row_width="$3" agent="$4" editor="$5" root="$6" agent_state="$7" frozen="$8" needs_reopen="$9"
  local model="${10:-}"
  local branch="—" state="  " state_plain="  " activity="" activity_plain="" pause_mark="" pr_file pr_state
  case "$agent_state" in
    working) activity=" ⠋"; activity_plain=" ⠋" ;;
    waiting) activity=" \033[33m● needs you\033[0;2m"; activity_plain=" ● needs you" ;;
    done) activity=" \033[32m✓ done\033[0;2m"; activity_plain=" ✓ done" ;;
  esac
  if [ -n "$frozen" ] && [ -n "$needs_reopen" ]; then
    pause_mark=" · partly paused"
  elif [ -n "$frozen" ]; then
    pause_mark=" · paused"
  elif [ -n "$needs_reopen" ]; then
    pause_mark=" · reopen to pause"
  fi
  if [ -n "$root" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    branch="$(git -C "$root" branch --show-current 2>/dev/null)"
    branch="${branch:-detached}"
    pr_file="$(git -C "$root" rev-parse --path-format=absolute --git-dir 2>/dev/null)/devws-pr"
    if [ -r "$pr_file" ]; then
      read -r _ pr_state < "$pr_file"
      case "${pr_state:-none}" in none) ;; *) branch="$branch · PR $pr_state" ;; esac
    fi
    grep -qxF "$root" "$running_file" 2>/dev/null && branch="$branch · ▶ running"
    # A cancelled redraw must never leave Git holding the worktree index lock.
    if [ -n "$(git --no-optional-locks -C "$root" status --porcelain 2>/dev/null)" ]; then
      state="\033[33m●\033[0m "; state_plain="● "
    else
      state="\033[32m✓\033[0m "; state_plain="✓ "
    fi
  fi
  printf 'workspace:%s\t' "$session"
  if [ "$session" = "$current" ]; then
    print_padded_row '\033[1;37;48;5;238m' "$row_width" "$state_plain  $session"
    printf '\n'
    print_padded_row '\033[2;37;48;5;238m' "$row_width" "    $branch"
    printf '\n'
    print_padded_row '\033[2;37;48;5;238m' "$row_width" "    ${agent:-unknown}$activity_plain$pause_mark · ${editor:-unknown}"
  else
    printf '%b  %s\033[0m\n    \033[2m%s\033[0m\n    \033[2m%s%b%s · %s\033[0m' "$state" "$session" "$branch" "${agent:-unknown}" "$activity" "$pause_mark" "${editor:-unknown}"
  fi
  model_lines "$model" "    " "$row_width"
}

# A task's heading: its workspaces' project in bold green, the task (its
# branch) dim below; just the task when no workspace has a project folder.
task_row() {
  local task="$1" row_width="$2" project
  project="$(task_project "$task")"
  printf 'task:%s\t' "$task"
  if [ -z "$project" ]; then
    printf '\033[36m▸ %s\033[0m\t' "$(truncate_label "$task" "$((row_width - 2))")"
    return
  fi
  printf '\033[1;32m▸ %s\033[0m\n  \033[2m%s\033[0m\t' \
    "$(truncate_label "$project" "$((row_width - 2))")" "$(truncate_label "$task" "$((row_width - 2))")"
}

is_team_workspace() {
  [ -n "$2" ] || grep -qxF "$1" <<<"$team_snapshot"
}

workspace_rows() {
  local current="$1"
  local kind session place root agent_state frozen row_width disclosure counts pause_mark member agent editor team_of needs_reopen model label task

  row_width="$(
    tmux display-message -p -t "$picker_pane" '#{pane_width}' 2>/dev/null
  )"
  case "$row_width" in
    ''|*[!0-9]*) row_width=80 ;;
  esac
  # fzf reserves gutter/marker columns beyond the text area. Leave them out
  # of the label budget so model names never push status behind its ellipsis.
  (( row_width > 4 )) && row_width=$((row_width - 4))

  while IFS="$tab" read -r kind session place; do
    if [ "$kind" = task ]; then
      task_row "$session" "$row_width"
      while IFS="$tab" read -r _ member; do
        [ -z "$member" ] || search_text "$member"
      done < <(printf '%s\n' "$session_snapshot" | awk -F '\t' -v task="$session" '$2 == task { print $2 "\t" $1 }')
      printf '\0'
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
    IFS='|' read -r root agent_state frozen agent editor team_of needs_reopen <<< "$(tmux display-message -p -t "=$session:" \
      '#{@devws_root}|#{@devws_agent_state}|#{@devws_frozen}|#{@devws_agent}|#{@devws_editor}|#{@devws_team_of}|#{@devws_needs_reopen}' 2>/dev/null)"
    model="$(printf '%s\n' "$pane_snapshot" | awk -F '\t' -v session="$session" '$1 == session && $5 == "agent" { print ($6 != "" ? $6 : $7); exit }')"
    [ -z "$root" ] || pr_roots+=("$root")
    if ! is_team_workspace "$session" "$team_of"; then
      normal_workspace_row "$session" "$current" "$row_width" "$agent" "$editor" "$root" "$agent_state" "$frozen" "$needs_reopen" "$model"
      printf '\t%s\0' "$(search_text "$session")"
      continue
    fi
    pause_mark=""
    [ -z "$frozen" ] || pause_mark=" · paused"

    disclosure="▸"
    is_expanded "$session" && disclosure="▾"
    counts="$(activity_counts "$session")"
    if [ -z "$counts" ]; then
      case "$agent_state" in
        working) counts=" ⠋ working" ;;
        waiting) counts=" ● needs you" ;;
        done) counts=" ✓ done" ;;
      esac
    fi
    label="$session"
    task=""
    if [ "$place" = heading ]; then
      task="$(printf '%s\n' "$session_snapshot" | awk -F '\t' -v session="$session" '$1 == session { print $2 }')"
      label="$(task_project "$task")"
      label="${label:-$session}"
    fi
    if [ "$row_width" -lt 32 ]; then
      [ "$label" != "$session" ] || label="${session#dev-}"
      case "$counts" in
        *●*) counts=" ●" ;;
        *⠋*) counts=" ⠋" ;;
        *✓*) counts=" ✓" ;;
      esac
    fi
    label="$(truncate_label "$label" "$((row_width - 2 - ${#counts} - ${#pause_mark}))")"
    printf 'workspace:%s\t' "$session"
    if [ "$session" = "$current" ]; then
      print_padded_row '\033[1;37;48;5;238m' "$row_width" "$disclosure $label$counts$pause_mark"
    elif [ -n "$task" ]; then
      printf '%s \033[1;32m%s\033[0m%s%s' "$disclosure" "$label" "$counts" "$pause_mark"
    else
      printf '%s %s%s%s' "$disclosure" "$label" "$counts" "$pause_mark"
    fi
    # A team leading its task shows the task (its branch) dim below.
    [ -z "$task" ] || printf '\n  \033[2m%s\033[0m' "$(truncate_label "$task" "$((row_width - 2))")"
    model_lines "$model" "  " "$row_width"
    printf '\t%s\0' "$(search_text "$session")"
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

# Swapped in whole, so a spinner frame never reads half of the rows. Its own
# drafts (.$$): a redraw (--redraw) and fzf's --rows may draw at once.
write_rows() {
  local new="$rows_file.new.$$" filtered="$rows_file.filtered.$$" display="$rows_file.display.$$"
  pr_roots=()
  picker_current="${DEVWS_PICKER_CURRENT:-$(tmux display-message -p '#S' 2>/dev/null)}"
  # Not a subshell: the rows collect pr_roots.
  prepare_expansion
  workspace_rows "$picker_current" > "$new"
  if [ -n "$picker_query" ]; then
    local filter_status=0
    # Filtering the complete records preserves matching ancestors while the
    # interactive picker displays only field two (the hidden index stays hidden).
    fzf --filter "$picker_query" --no-sort --read0 --print0 --delimiter="$tab" --nth=2.. \
      < "$new" > "$filtered" || filter_status=$?
    [ "$filter_status" -le 1 ] || { rm -f "$new" "$filtered"; return "$filter_status"; }
    mv "$filtered" "$new"
  fi
  # Search indexes are private to filtering. Emit only key + visible label so
  # already-running pickers using --with-nth=2.. cannot display metadata.
  local record
  while IFS= read -r -d '' record; do
    printf '%s\0' "${record%$'\t'*}"
  done < "$new" > "$display"
  mv "$display" "$rows_file"
  rm -f "$new"
  refresh_states
}

# --redraw <socket>: a state changed (refresh_devws_pickers.sh --reload). The
# rows are drawn here, beside fzf, which then only reads them: drawing them in
# fzf (reload-sync of --rows) blocked its keys and clicks the whole time, on
# every change. Redraws asked while one runs fold into one more; a lock left
# by a redraw that was killed is taken over.
redraw() {
  local socket="$1" lock="$rows_file.lock" holder
  : > "$rows_file.again"
  while [ -e "$rows_file.again" ]; do
    if ! mkdir "$lock" 2>/dev/null; then
      holder="$(cat "$lock/pid" 2>/dev/null)"
      # Another redraw runs: it sees .again and draws once more.
      [ -z "$holder" ] || kill -0 "$holder" 2>/dev/null && return 0
      rm -rf "$lock"
      continue
    fi
    printf '%s\n' "$$" > "$lock/pid"
    while [ -e "$rows_file.again" ]; do
      rm -f "$rows_file.again" "$rows_file.stale"
      write_rows
      curl -s --max-time 2 --unix-socket "$socket" http://localhost \
        -d "reload-sync(cat $(printf '%q' "$rows_file"))" >/dev/null 2>&1 ||
        : > "$rows_file.stale"
    done
    rm -rf "$lock"
  done
}

if [ "${1:-}" = --expand ] || [ "${1:-}" = --collapse ]; then
  prepare_expansion
  key="${2:-}"
  key="${key#*:}"
  case "${2:-}" in workspace:*|group:*)
    value=open
    [ "$1" != --collapse ] || value=closed
    set_expanded "$key" "$value"
    ;;
  esac
  exit 0
fi

if [ "${1:-}" = --search ]; then
  set_picker_option @devws_picker_query "${2:-}"
  set -- --rows
fi

# Navigation runs after an fzf callback may have discovered a new task.
workspace_for_task() {
  prepare_expansion
  workspace_order | awk -F '\t' -v task="$1" '
    $1 == "task" && $2 == task { found = 1; next }
    found && $1 == "workspace" { print $2; exit }
    found { exit }'
}

if [ "${1:-}" = --task-workspace ]; then
  workspace_for_task "${2:-}"
  exit 0
fi

if [ "${1:-}" = --position ]; then
  prepare_expansion
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

if [ "${1:-}" = --redraw ]; then
  rows_file="${DEVWS_PICKER_ROWS:?}"
  redraw "${2:?a sidebar socket}"
  exit 0
fi

# <key>: what a row (or the header's ☰ Menu, "menu") leads to. Run by fzf
# itself (--go) on Enter or a click, so the sidebar never closes to do it:
# closing and drawing it again flickered, and left no sidebar when anything
# failed in between.
go_to() {
  local key="$1" workspace
  case "$key" in
    # Every workspace action is in the tabbed menu (devws_menu.sh).
    menu) "$HOME/.tmuxinator/devws_menu.sh" --popup ;;
    workspace:*) workspace="${key#workspace:}" ;;
    # A group of an agent team: its workspace.
    group:*) workspace="${key#group:}" ;;
    # An agent of a team: its group's workspace, the agent.
    agent:*)
      tmux switch-client -t "${key#agent:}" 2>/dev/null
      "$HOME/.tmuxinator/team_groups.sh" go "${key#agent:}" 2>/dev/null
      workspace="$(tmux display-message -p -t "${key#agent:}" '#S' 2>/dev/null)"
      ;;
    # A task's heading takes you to its first workspace.
    task:*) workspace="$(workspace_for_task "${key#task:}")" ;;
  esac
  [ -n "${workspace:-}" ] || return 0
  [[ "$key" == agent:* ]] || tmux switch-client -t "$workspace" 2>/dev/null
  point_at_own_workspace "$workspace"
}

# <workspace>: its sidebar's cursor on its own row, as each sidebar starts,
# so it shows where you are when you get there.
point_at_own_workspace() {
  local pane socket position
  IFS='|' read -r pane socket <<<"$(tmux list-panes -t "=$1:" -F '#{pane_title}|#{pane_id}|#{@devws_picker_socket}' 2>/dev/null |
    awk -F '|' '$1 == "devws-picker" { print $2 "|" $3; exit }')"
  [ -n "$pane" ] && [ -S "$socket" ] || return 0
  position="$(TMUX_PANE="$pane" "$picker_script" --position "$1")"
  [ -n "$position" ] || return 0
  curl -s --max-time 2 --unix-socket "$socket" http://localhost -d "pos($position)" >/dev/null 2>&1
}

if [ "${1:-}" = --go ]; then
  go_to "${2:-}"
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
trap 'rm -rf "$rows_file" "$rows_file".* "$fzf_socket"' EXIT

# fzf runs only once a sidebar: Enter and clicks act from inside it (--go),
# in the background. fzf stops tracking the mouse at the next key after a
# command it waited on for a while (the menu's popup, waking a frozen
# workspace), and the sidebar no longer answered clicks.
# It ends only on Esc, or when refresh_devws_pickers.sh restarts it, and
# starts again on the current workspace with an empty search.
while tmux list-sessions >/dev/null 2>&1; do
  set_picker_option @devws_picker_query ""
  picker_current="${DEVWS_PICKER_CURRENT:-$(tmux display-message -p '#S' 2>/dev/null)}"
  current="$picker_current"
  current_position="$("$picker_script" --position "$current")"
  [ -n "$current_position" ] || current_position=1
  rm -f "$rows_file.stale"
  write_rows
  go="execute-silent('$picker_script' --go {1} >/dev/null 2>&1 &)"
  fzf --height=100% --layout=reverse --ansi --read0 --gap=0 --track --id-nth=1 --disabled --no-sort \
      --highlight-line \
      --listen="$fzf_socket" \
      --color='bg+:244,fg+:255' \
      --delimiter=$'\t' --with-nth=2 \
      --bind="right:execute-silent('$picker_script' --expand {1})+reload-sync('$picker_script' --rows)" \
      --bind="left:execute-silent('$picker_script' --collapse {1})+reload-sync('$picker_script' --rows)" \
      --bind="change:reload-sync('$picker_script' --search {q})" \
      --bind="load:pos($current_position)+unbind(load)" \
      --bind="enter:$go" --bind="left-click:$go" --bind="double-click:$go" \
      --bind="start:transform:[ -e '$rows_file.stale' ] && printf 'reload-sync(%q --rows)' '$picker_script'" \
      --bind="click-header:transform:
        [ \"\$FZF_CLICK_HEADER_LINE\" = 1 ] && printf '%s\\n' \"execute-silent('$picker_script' --go menu >/dev/null 2>&1 &)\"" \
      --header=$'\033[36m☰ Menu\033[0m  \033[2mC-b Space\033[0m\n\n\033[2mOPEN WORKSPACES\033[0m' \
      --prompt='Find › ' --footer='← collapse · → expand' \
      --info=hidden --no-separator --border=none --margin=0 --padding=0 \
      < "$rows_file" >/dev/null
done
