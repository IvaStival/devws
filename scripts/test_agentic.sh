#!/usr/bin/env bash
# `devws agentic`: the workspace for an agent team (a leader, a log, and the
# panels the leader opens), and what it needs of the rest of devws.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Physical: devws resolves folders with pwd -P, and /var is /private/var here.
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-agentic.XXXXXX")" && pwd -P)"
SCRIPTS_SRC="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-agentic-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'Agentic test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server. The agents and Neovim are stubs
# that record their arguments, NUL-separated, and wait; fzf is one too.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project" "$TEST_ROOT/started"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for program in claude codex nvim fzf; do
  cat > "$TEST_ROOT/bin/$program" <<EOF
#!/usr/bin/env bash
printf '%s\0' "\$@" > "$TEST_ROOT/started/$program.\$\$"
exec sleep 600
EOF
done
# The devAgentics side, as devws expects it: `task start <folder>` prints the
# task id, the leader's agent and the path of its arguments file; `board <id>`
# runs in the board pane. Calls are recorded like the agents' starts.
printf '%s\0' --model opus > "$TEST_ROOT/leader-args"
cat > "$TEST_ROOT/bin/devagentics" <<EOF
#!/usr/bin/env bash
printf '%s\0' "\$@" > "$TEST_ROOT/started/devagentics-\$1.\$\$"
case "\$1" in
  task) printf 'team-1\nclaude\n$TEST_ROOT/leader-args\n' ;;
  board) exec sleep 600 ;;
esac
EOF
ln -s "$ROOT/bin/devws" "$TEST_ROOT/bin/devws"
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS_SRC"/*.sh "$SCRIPTS_SRC"/*.json "$SCRIPTS_SRC"/*.md "$SCRIPTS_SRC"/*.yml; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
printf 'DEVWS_AGENTS="claude codex"\nDEVWS_EDITORS="nvim"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
export DEVWS_NOTIFY_COMMAND=true DEVWS_AUTO_RESTORE=0
export DEVWS_HOME="$TEST_ROOT/devws" DEVWS_CLONE_BASE="$TEST_ROOT/remotes/"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"

wait_until() {
  local attempt
  for attempt in $(seq 1 50); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

field() {
  tmux display-message -p -t "$1" "#{$2}"
}

RS=$'\036'

# The newest recording of a stub, its arguments each followed by RS.
started_args() {
  local file
  file="$(ls -t "$TEST_ROOT/started/$1".* 2>/dev/null | head -1)"
  [ -n "$file" ] || return 1
  tr '\0' "$RS" < "$file"
}

status_of() {
  local code=0
  "$@" >/dev/null 2>&1 || code=$?
  printf '%s' "$code"
}

pane_with_role() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' | awk -v role="$2" '$2 == role { print $1; exit }'
}

# A workspace with no editor, as an agent team has: the sidebar, a leader
# (role agent) and a terminal under it.
create_team_workspace() {
  local session="$1" pane
  tmux new-session -d -s "$session" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$session" @devws_root "$TEST_ROOT/project"
  tmux set-option -t "$session" @devws_agent claude
  tmux select-pane -t "$session:" -T devws-picker
  pane="$(tmux split-window -h -l 175 -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$pane" @devws_role agent
  pane="$(tmux split-window -v -l 8 -t "$pane" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$pane" @devws_role terminal
}

test_a_workspace_with_no_editor_is_left_alone() {
  local session=dev-noeditor output layout script
  create_team_workspace "$session"
  layout="$(field "$session:" window_layout)"

  for script in "minimize_panes.sh $session toggle agents" "toggle_layout.sh $session" "open_database.sh $session"; do
    # shellcheck disable=SC2086
    output="$("$SCRIPTS"/$script 2>&1)" || fail "$script failed without an editor: $output"
    case "$output" in
      *"no editor"*) ;;
      *) fail "$script did not say the workspace has no editor: $output" ;;
    esac
  done
  [ "$(field "$session:" window_layout)" = "$layout" ] || fail "an editor-only action changed the layout"
  [ -z "$(pane_with_role "$session" database)" ] || fail "a database pane opened without an editor"

  # Expanding a group that cannot be minimized is silent: New panel does it
  # before every panel.
  output="$("$SCRIPTS/minimize_panes.sh" "$session" restore agents 2>&1)" || fail "restore failed without an editor"
  [ -z "$output" ] || fail "restore said something without an editor: $output"
}

test_new_panel_works_without_an_editor() {
  local session=dev-noeditor2 pane
  create_team_workspace "$session"
  pane="$("$SCRIPTS/new_panel.sh" "$session" right agent claude)" || fail "New panel failed without an editor"
  [ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] || fail "the panel is not an extra agent"
}

conf_option() {
  sed -n "s/^set -g $1 \"\\(.*\\)\"\$/\\1/p" "$ROOT/config/tmux/tmux.conf"
}

test_a_label_names_the_panel_in_its_header() {
  local session=dev-label leader panel header
  create_team_workspace "$session"
  leader="$(pane_with_role "$session" agent)"
  header="$(conf_option pane-border-format)"
  [ -n "$header" ] || fail "could not read the header format"
  # The state icons and badges tmux.conf defines, which the header uses.
  grep -E '^(color_[a-z_]+=|set -g @devws_(agent|pane)_)' "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/formats.conf"
  tmux source-file "$TEST_ROOT/formats.conf"

  [ "$(tmux display-message -p -t "$leader" "$header" | grep -c 'Agent · claude')" = 1 ] ||
    fail "a panel with no label does not say Agent: $(tmux display-message -p -t "$leader" "$header")"
  tmux set-option -p -t "$leader" @devws_label Leader
  tmux display-message -p -t "$leader" "$header" | grep -q 'Leader · claude' || fail "the leader's header does not say Leader"
  ! tmux display-message -p -t "$leader" "$header" | grep -q 'Agent' || fail "the label did not replace Agent"

  panel="$("$SCRIPTS/new_panel.sh" "$session" right agent codex --label 'Backend tests')" || fail "New panel with a label failed"
  [ "$(tmux show-options -pqv -t "$panel" @devws_label)" = 'Backend tests' ] || fail "New panel did not set the label"
  "$SCRIPTS/agent_status.sh" "$session" waiting "$panel"
  tmux display-message -p -t "$panel" "$header" | grep -q 'Backend tests · codex .*● needs you' ||
    fail "the labelled panel's header lost its state: $(tmux display-message -p -t "$panel" "$header")"

  panel="$("$SCRIPTS/agent_cli.sh" spawn "$session" claude --label Frontend)" || fail "spawn with a label failed"
  [ "$(tmux show-options -pqv -t "$panel" @devws_label)" = Frontend ] || fail "spawn did not set the label"
}

# The sidebar, a leader (role agent) and the log under it (role team-log),
# the shape of an agent team before it has any panel.
create_team_with_log() {
  local session="$1" leader log
  create_team_workspace "$session"
  log="$(pane_with_role "$session" terminal)"
  tmux set-option -p -t "$log" @devws_role team-log
}

test_team_panels_fill_the_right_half_in_a_grid() {
  local session=dev-grid leader log first second third focused width height picker sidebar_width panel
  create_team_with_log "$session"
  leader="$(pane_with_role "$session" agent)"
  log="$(pane_with_role "$session" team-log)"
  # As in a workspace devws agentic has just opened: the focus is on the sidebar.
  picker="$(tmux list-panes -t "$session:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }')"
  tmux select-pane -t "$picker"
  focused="$(field "$session:" pane_id)"
  sidebar_width="$(field "$picker" pane_width)"
  width="$(field "$session:" window_width)"
  height="$(field "$session:" window_height)"

  first="$("$SCRIPTS/agent_cli.sh" spawn "$session" claude --place team --label Backend)" || fail "the first team panel failed"
  [ "$(field "$picker" pane_width)" = "$sidebar_width" ] || fail "the sidebar lost its width: $(field "$picker" pane_width)"
  # Full height on the right; the leader and the log keep their column.
  [ "$(field "$first" pane_top)" = 0 ] && [ "$(field "$first" pane_height)" = "$height" ] ||
    fail "the first panel does not span the window's height"
  [ "$(field "$first" pane_left)" -gt "$(field "$leader" pane_left)" ] || fail "the first panel is not right of the leader"
  [ "$(field "$leader" pane_left)" = "$(field "$log" pane_left)" ] &&
    [ "$(field "$leader" pane_width)" = "$(field "$log" pane_width)" ] || fail "the log left the leader's column"
  [ "$(field "$log" pane_top)" -gt "$(field "$leader" pane_top)" ] || fail "the log is not under the leader"
  # About half of what the sidebar leaves.
  [ "$(( $(field "$first" pane_width) - $(field "$leader" pane_width) ))" -le 2 ] &&
    [ "$(( $(field "$leader" pane_width) - $(field "$first" pane_width) ))" -le 2 ] ||
    fail "the leader and the first panel do not share the width: $(field "$leader" pane_width) / $(field "$first" pane_width)"

  second="$("$SCRIPTS/agent_cli.sh" spawn "$session" codex --place team)" || fail "the second team panel failed"
  [ "$(field "$second" pane_left)" = "$(field "$first" pane_left)" ] &&
    [ "$(field "$second" pane_top)" -gt "$(field "$first" pane_top)" ] || fail "the second panel is not below the first"

  third="$("$SCRIPTS/agent_cli.sh" spawn "$session" claude --place team)" || fail "the third team panel failed"
  [ "$(field "$third" pane_top)" = "$(field "$first" pane_top)" ] &&
    [ "$(field "$third" pane_left)" -gt "$(field "$first" pane_left)" ] || fail "the third panel is not beside the first"

  [ "$(field "$session:" pane_id)" = "$focused" ] || fail "spawning moved the focus"
  for panel in "$first" "$second" "$third"; do
    [ "$(tmux show-options -pqv -t "$panel" @devws_place)" = team ] || fail "$panel is not marked as a team panel"
    [ "$(tmux show-options -pqv -t "$panel" @devws_role)" = extra-agent ] || fail "$panel is not an extra agent"
  done
  [ "$(tmux show-options -pqv -t "$first" @devws_label)" = Backend ] || fail "the label was lost"
  [ "$(field "$session:" window_width)" = "$width" ] || fail "the window changed size"

  [ "$(status_of "$SCRIPTS/agent_cli.sh" spawn "$session" claude --place sideways)" = 2 ] || fail "an unknown place was accepted"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" spawn "$session" claude --place team --beside "$leader")" = 2 ] ||
    fail "--place with --beside was accepted"
}

test_the_first_agent_goes_under_the_board() {
  local session=dev-anchor leader board first second
  create_team_with_log "$session"
  leader="$(pane_with_role "$session" agent)"
  board="$(pane_with_role "$session" team-log)"
  # The board is a column to the right of the leader, full height.
  tmux kill-pane -t "$board"
  board="$(tmux split-window -h -l 70 -t "$leader" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$board" @devws_role team-log
  tmux set-option -p -t "$board" @devws_place anchor

  first="$("$SCRIPTS/agent_cli.sh" spawn "$session" claude --place team --label Backend)" || fail "the first panel failed"
  [ "$(field "$first" pane_left)" = "$(field "$board" pane_left)" ] &&
    [ "$(field "$first" pane_top)" -gt "$(field "$board" pane_top)" ] || fail "the first panel is not under the board"
  [ "$(field "$first" pane_height)" -gt "$(field "$board" pane_height)" ] || fail "the board kept more than the agent"
  second="$("$SCRIPTS/agent_cli.sh" spawn "$session" codex --place team)" || fail "the second panel failed"
  # The first is wider than twice its height: the next goes beside it.
  [ "$(field "$second" pane_top)" = "$(field "$first" pane_top)" ] &&
    [ "$(field "$second" pane_left)" -gt "$(field "$first" pane_left)" ] || fail "the second panel did not go beside the first"
}

test_agentic_opens_the_leader_and_the_board() {
  local folder session output leader log header
  folder="$TEST_ROOT/projects/team-app"
  mkdir -p "$folder"

  output="$(devws agentic "$folder" </dev/null)" || fail "devws agentic failed: $output"
  session=dev-team-app
  case "$output" in *"$session"*) ;; *) fail "devws agentic did not name the session: $output" ;; esac
  tmux has-session -t "=$session" || fail "devws agentic did not start the workspace"
  [ "$(tmux show-option -qv -t "=$session:" @devws_template)" = agentic ] || fail "the workspace is not marked as an agent team"
  [ "$(tmux show-option -qv -t "=$session:" @devws_task)" = team-1 ] || fail "the task is not the one devagentics gave"

  wait_until started_args claude >/dev/null || fail "the leader did not start"
  case "$(started_args claude)" in
    *"--append-system-prompt$RS"*"${RS}--model${RS}opus$RS") ;;
    *) fail "the leader did not get devws's and devagentics's arguments: $(started_args claude)" ;;
  esac
  wait_until started_args devagentics-board >/dev/null || fail "the board pane did not run devagentics board"
  [ "$(started_args devagentics-board)" = "board${RS}team-1$RS" ] || fail "board got: $(started_args devagentics-board)"
  [ "$(started_args devagentics-task)" = "task${RS}start${RS}$folder$RS" ] || fail "task start got: $(started_args devagentics-task)"

  leader="$(pane_with_role "$session" agent)"
  log="$(pane_with_role "$session" team-log)"
  [ -n "$leader" ] && [ -n "$log" ] || fail "the leader or the log pane is missing"
  [ -z "$(pane_with_role "$session" editor)" ] || fail "an agent team opened an editor"
  [ "$(field "$log" pane_left)" -gt "$(field "$leader" pane_left)" ] &&
    [ "$(field "$log" pane_top)" = "$(field "$leader" pane_top)" ] || fail "the board is not beside the leader"
  [ "$(field "$log" pane_height)" = "$(field "$leader" pane_height)" ] || fail "the board is not as tall as the leader"
  [ "$(tmux show-options -pqv -t "$log" @devws_place)" = anchor ] || fail "the board is not the anchor of the team area"
  wait_until test "$(tmux show-options -pqv -t "$leader" @devws_label)" = Leader || fail "the leader's panel is not labelled"

  # The log pane has a header too.
  header="$(conf_option pane-border-format)"
  tmux display-message -p -t "$log" "$header" | grep -q 'Team' || fail "the board pane has no header: '$(tmux display-message -p -t "$log" "$header")'"
}

test_agentic_makes_a_worktree_for_a_repo_and_a_branch() {
  local remote seed folder output
  remote="$TEST_ROOT/remotes/acme/app.git"
  seed="$TEST_ROOT/seed"
  git init -q --bare -b main "$remote"
  git init -q -b main "$seed"
  git -C "$seed" -c user.name=t -c user.email=t@t commit -q --allow-empty -m first
  git -C "$seed" remote add origin "$remote"
  git -C "$seed" push -q origin main

  output="$(devws agentic acme/app feat/x </dev/null 2>/dev/null)" || fail "devws agentic on a repo failed: $output"
  folder="$DEVWS_HOME/acme/app/app-feat-x"
  [ "$(git -C "$folder" symbolic-ref --short HEAD)" = feat/x ] || fail "the worktree is not on the branch"
  tmux has-session -t =dev-app-feat-x || fail "the workspace is not on the worktree"
  wait_until test "$(tmux show-option -qv -t =dev-app-feat-x: @devws_root)" = "$folder" ||
    fail "the workspace's folder is not the worktree"
}

test_agentic_needs_devagentics() {
  local code=0
  DEVWS_AGENTIC_COMMAND=no-such-devagentics devws agentic "$TEST_ROOT/projects" </dev/null >"$TEST_ROOT/out" 2>&1 || code=$?
  [ "$code" = 1 ] || fail "devws agentic without devagentics exited $code"
  grep -q 'no-such-devagentics' "$TEST_ROOT/out" || fail "it did not name the missing program: $(cat "$TEST_ROOT/out")"
  ! tmux has-session -t =dev-projects 2>/dev/null || fail "a workspace opened without devagentics"
  # Nothing is made before it is known to work: no clone, no worktree.
  DEVWS_AGENTIC_COMMAND=no-such-devagentics devws agentic acme/app feat/never </dev/null >/dev/null 2>&1 || true
  [ ! -e "$DEVWS_HOME/acme/app/app-feat-never" ] || fail "a worktree was made before devagentics was found"

  [ "$(status_of devws agentic one two three)" = 2 ] || fail "devws agentic accepted three arguments"
  [ "$(status_of devws agentic --bogus "$TEST_ROOT/projects")" = 2 ] || fail "devws agentic accepted an unknown option"
  [ "$(status_of devws agentic acme/app)" = 2 ] || fail "devws agentic accepted a repo with no branch"
}

# Without devagentics, devws agentic offers to install it (agentic_install.sh)
# and goes on once it is there; a refusal or failure stops it, nothing made.
test_agentic_offers_to_install_devagentics() {
  local folder="$TEST_ROOT/projects/needs-install" path="" entry code=0
  local -a entries
  mkdir -p "$folder"
  # Only the stubs: no devagentics this machine may have on its PATH.
  IFS=: read -r -a entries <<<"$PATH"
  for entry in "${entries[@]}"; do
    [ "$entry" != "$TEST_ROOT/bin" ] && [ -x "$entry/devagentics" ] && continue
    path="${path:+$path:}$entry"
  done
  mv "$TEST_ROOT/bin/devagentics" "$TEST_ROOT/bin/devagentics.off"
  rm "$SCRIPTS/agentic_install.sh"
  cat > "$SCRIPTS/agentic_install.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$TEST_ROOT/installer.log"
[ -e "$TEST_ROOT/installer.fails" ] && exit 1
mv "$TEST_ROOT/bin/devagentics.off" "$TEST_ROOT/bin/devagentics"
EOF
  chmod +x "$SCRIPTS/agentic_install.sh"

  touch "$TEST_ROOT/installer.fails"
  PATH="$path" devws agentic "$folder" </dev/null >"$TEST_ROOT/out" 2>&1 || code=$?
  [ "$code" = 1 ] || fail "devws agentic went on after the install failed ($code)"
  grep -q 'devagentics is not installed' "$TEST_ROOT/out" || fail "no explanation: $(cat "$TEST_ROOT/out")"
  ! tmux has-session -t =dev-needs-install 2>/dev/null || fail "a workspace opened without devagentics"

  rm "$TEST_ROOT/installer.fails"
  PATH="$path" devws agentic "$folder" </dev/null >"$TEST_ROOT/out" 2>&1 ||
    fail "devws agentic failed after installing: $(cat "$TEST_ROOT/out")"
  [ "$(tail -n 1 "$TEST_ROOT/installer.log")" = install ] || fail "the installer was not asked: $(cat "$TEST_ROOT/installer.log")"
  wait_until tmux has-session -t =dev-needs-install || fail "the team did not open once devagentics was installed"

  rm "$SCRIPTS/agentic_install.sh"
  ln -s "$SCRIPTS_SRC/agentic_install.sh" "$SCRIPTS/agentic_install.sh"
}

test_agentic_can_add_an_editor() {
  local folder session
  folder="$TEST_ROOT/projects/with-editor"
  mkdir -p "$folder"
  devws agentic --editor "$folder" </dev/null >/dev/null || fail "devws agentic --editor failed"
  session=dev-with-editor
  wait_until test -n "$(pane_with_role "$session" extra-editor)" || fail "--editor did not add a Neovim panel"
  wait_until started_args nvim >/dev/null || fail "the Neovim panel did not start"
  [ -z "$(pane_with_role "$session" editor)" ] || fail "--editor made the panel the workspace's editor"
}

test_a_workspace_with_no_editor_is_left_alone
test_new_panel_works_without_an_editor
test_a_label_names_the_panel_in_its_header
test_team_panels_fill_the_right_half_in_a_grid
test_the_first_agent_goes_under_the_board
test_agentic_opens_the_leader_and_the_board
test_agentic_makes_a_worktree_for_a_repo_and_a_branch
test_agentic_needs_devagentics
test_agentic_offers_to_install_devagentics
test_agentic_can_add_an_editor

printf 'Agentic tests passed\n'
