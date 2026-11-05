#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-suspend.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-suspend-test-$$"
CLIENT_SOCKET="devws-suspend-client-$$"

cleanup() {
  # Whatever a failed test left frozen is let go, so the servers can end it.
  [ -n "${SUSPEND:-}" ] && "$SUSPEND" resume --all >/dev/null 2>&1
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  # The editors' sockets live in /tmp, as the real ones do.
  [ -f "$TEST_ROOT/nvim.sockets" ] && xargs rm -f < "$TEST_ROOT/nvim.sockets"
  # The panes' shells may still be writing as the server goes: try again.
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'Suspend test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server; ~/.tmuxinator holds the real
# scripts. claude, nvim and fzf are stubs that just wait, as the real ones do
# when nothing happens. TMPDIR keeps the list of frozen processes apart.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/tmp"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for program in claude nvim fzf; do
  printf '#!/usr/bin/env bash\nexec sleep 600\n' > "$TEST_ROOT/bin/$program"
done
# With STUB_JOB set, the agent leaves a job running in a shell, as Claude's
# background commands do; with STUB_BUSY, it runs a task with no shell
# around it (a build it started directly), which keeps a core busy.
# (An "if" so the task is the stub's own child, not a subshell's.) Its
# arguments go to claude.args.
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$@" > "%s/claude.args"\n[ -n "${STUB_JOB:-}" ] && sh -c "sleep 600; :" &\nif [ -n "${STUB_BUSY:-}" ]; then yes > /dev/null & fi\nexec sleep 600\n' \
  "$TEST_ROOT" > "$TEST_ROOT/bin/claude"
# Neovim started with --listen answers through that socket with its pid and
# how many buffers are modified (nvim.modified, 0 unless a test says
# otherwise); without --listen nothing answers.
cat > "$TEST_ROOT/bin/nvim" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = --server ]; then
  [ -f "$TEST_ROOT/nvim.pid.\${2##*/}" ] || exit 1
  printf '%s %s\n' "\$(cat "$TEST_ROOT/nvim.pid.\${2##*/}")" "\$(cat "$TEST_ROOT/nvim.modified" 2>/dev/null || echo 0)"
  exit 0
fi
if [ "\${1:-}" = --listen ]; then
  echo \$\$ > "$TEST_ROOT/nvim.pid.\${2##*/}"
  : > "\$2"
  echo "\$2" >> "$TEST_ROOT/nvim.sockets"
fi
exec sleep 600
EOF
# The header's sources stay off the network.
printf '#!/usr/bin/env bash\nexit 7\n' > "$TEST_ROOT/bin/curl"
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS"/*.sh "$SCRIPTS"/*.json "$SCRIPTS"/*.md; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
# A minute, for the header's ticker; the sweeps called here say their own.
printf 'DEVWS_AGENTS="claude"\nDEVWS_SUSPEND_AFTER=1\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp"
# The editors' sockets apart from the real ones, which nvim_servers.sh
# would otherwise stop.
export DEVWS_NVIM_SOCKET_PREFIX="/tmp/devws-nvim-test-$$-"
unset TMUX DEVWS_SUSPEND_AFTER
SUSPEND="$HOME/.tmuxinator/suspend_workspace.sh"

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

# A workspace as dev.yml opens it: the sidebar, and the editor, agent and
# terminal typed into an interactive shell, as tmuxinator does.
create_workspace() {
  local session="$1" role pane
  tmux new-session -d -s "$session" -x 200 -y 50 "$HOME/.tmuxinator/session_picker.sh"
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'zsh -f -i')"
    tmux select-layout -t "$session:" tiled
    case "$role" in
      editor) tmux send-keys -t "$pane" "$HOME/.tmuxinator/editor_runner.sh nvim" Enter ;;
      agent) tmux send-keys -t "$pane" "$HOME/.tmuxinator/agent_runner.sh claude" Enter ;;
      terminal)
        tmux set-option -p -t "$pane" @devws_role terminal
        tmux send-keys -t "$pane" 'sleep 600' Enter
        ;;
    esac
    wait_until program_runs "$pane" || fail "$session: the $role did not start"
  done
  wait_until sidebar_waits "$session" || fail "$session: the sidebar did not start"
}

pane_id() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

pane_pid() {
  tmux display-message -p -t "$1" '#{pane_pid}'
}

# The program at the bottom of a pane: the shell's child, its child, and so on.
program_pid() {
  local pid child
  pid="$(pane_pid "$1")"
  while child="$(pgrep -P "$pid" | head -n 1)" && [ -n "$child" ]; do
    pid="$child"
  done
  printf '%s\n' "$pid"
}

program_runs() {
  [ "$(ps -o command= -p "$(program_pid "$1")")" = "sleep 600" ]
}

parent_of() {
  ps -o ppid= -p "$1" | tr -d ' '
}

state_of() {
  ps -o state= -p "$1" | cut -c1
}

# The program is stopped; the shell and what sits between them are not.
frozen() {
  local program
  program="$(program_pid "$1")"
  [ "$(state_of "$program")" = T ] && [ "$(state_of "$(parent_of "$program")")" != T ] &&
    [ "$(state_of "$(pane_pid "$1")")" != T ]
}

running() {
  [ "$(state_of "$(program_pid "$1")")" != T ]
}

picker() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }'
}

# The sidebar's fzf (the stub), in the subshell that reads its choice.
sidebar_fzf() {
  local subshell
  for subshell in $(pgrep -P "$(pane_pid "$(picker "$1")")"); do
    pgrep -f -P "$subshell" '^sleep 600$'
  done
}

sidebar_waits() {
  [ -n "$(picker "$1")" ] && [ -n "$(sidebar_fzf "$1")" ]
}

# The rows a sidebar shows, one workspace a line.
sidebar_rows() {
  tr '\0\n' '\n ' < "$(tmux show-options -pqv -t "$(picker "$1")" @devws_picker_rows)"
}

sidebar_shows() {
  sidebar_rows "$1" | grep -q -- "$2"
}

attached() {
  [ "$(tmux display-message -p -t "$1" '#{session_attached}')" -gt 0 ]
}

option() {
  tmux show-options -qv -t "$1" "$2"
}

# Idle at once: the workspace was last seen long ago.
make_idle() {
  tmux set-option -t "$1" @devws_seen 1
}

test_an_idle_workspace_is_frozen() {
  local session=dev-idle agent
  create_workspace "$session"
  agent="$(program_pid "$(pane_id "$session" agent)")"
  [[ "$(ps -o command= -p "$(parent_of "$agent")")" == *agent_runner.sh* ]] ||
    fail "the agent runner replaced itself with the agent: nothing shields the shell"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep || fail "the sweep failed"
  frozen "$(pane_id "$session" agent)" || fail "the idle agent was not frozen"
  frozen "$(pane_id "$session" editor)" || fail "the idle editor was not frozen"
  [ "$(state_of "$(sidebar_fzf "$session")")" = T ] || fail "the idle sidebar was not frozen"
  [ "$(state_of "$(pane_pid "$(picker "$session")")")" != T ] || fail "the sidebar's script was stopped"
  running "$(pane_id "$session" terminal)" || fail "the terminal was frozen"
  [ "$(option "$session" @devws_frozen)" = 1 ] || fail "the workspace is not marked frozen"
}

# C-b $ types into the panes that run a shell: an agent's runner is a bash,
# but no shell to type into.
test_renewing_the_environment_skips_the_agent() {
  local agent
  agent="$(pane_id dev-idle agent)"
  tmux run-shell -t dev-idle: "$ROOT/config/tmux/renew_env.sh"
  sleep 0.5
  ! tmux capture-pane -p -t "$agent" | grep -q 'show-env' ||
    fail "C-b \$ typed into the agent: $(tmux capture-pane -p -t "$agent")"
}

# Continued in the right order, the shells never see their program stop: the
# runner is still in the foreground, and no "suspended" job is reported.
test_resume_gives_the_shells_their_programs_back() {
  local session=dev-idle pane
  "$SUSPEND" resume "$session" || fail "resume failed"
  for pane in "$(pane_id "$session" agent)" "$(pane_id "$session" editor)"; do
    running "$pane" || fail "$pane is still frozen after resume"
  done
  [ "$(state_of "$(sidebar_fzf "$session")")" != T ] || fail "the sidebar is still frozen after resume"
  sleep 0.5
  [ "$(tmux display-message -p -t "$(pane_id "$session" agent)" '#{pane_current_command}')" = bash ] ||
    fail "the agent's shell took the terminal back"
  ! tmux capture-pane -p -t "$(pane_id "$session" agent)" | grep -qi suspended ||
    fail "the agent's shell reported it suspended: $(tmux capture-pane -p -t "$(pane_id "$session" agent)")"
  [ -z "$(option "$session" @devws_frozen)" ] || fail "the workspace is still marked frozen"
  [ "$(option "$session" @devws_seen)" -gt 1 ] || fail "resume did not mark the workspace seen"
}

test_busy_or_recent_workspaces_stay() {
  local session state rows_file
  for state in working waiting; do
    session="dev-$state"
    create_workspace "$session"
    make_idle "$session"
    tmux set-option -t "$session" @devws_agent_state "$state"
  done
  create_workspace dev-recent
  tmux set-option -t dev-recent @devws_seen "$(date +%s)"
  create_workspace dev-done
  make_idle dev-done
  tmux set-option -t dev-done @devws_agent_state done
  tmux set-option -t dev-done @devws_agent_state_since "$(date +%s)"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep || fail "the sweep failed"
  # An agent team's workspace, idle and unseen for long: its agents take
  # orders at any time.
  create_workspace dev-team-idle
  make_idle dev-team-idle
  tmux set-option -t dev-team-idle @devws_task team-1
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep || fail "the sweep failed"
  for session in dev-working dev-waiting dev-recent dev-done dev-team-idle; do
    running "$(pane_id "$session" agent)" || fail "$session was frozen"
  done
  [ -z "$(option dev-team-idle @devws_frozen)" ] || fail "an agent team's workspace was marked frozen"
  # Done, but a job it started still runs: an action, so it stays.
  create_workspace dev-job
  tmux respawn-pane -k -t "$(pane_id dev-job agent)" "STUB_JOB=1 $HOME/.tmuxinator/agent_runner.sh claude"
  wait_until pgrep -f '^sh -c sleep 600; :$' >/dev/null || fail "the agent's job did not start"
  make_idle dev-job
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep || fail "the sweep failed"
  [ -z "$(option dev-job @devws_frozen)" ] || fail "a workspace whose agent runs a job was frozen"
  tmux kill-session -t dev-job

  make_idle dev-idle
  DEVWS_SUSPEND_AFTER=0 "$SUSPEND" sweep || fail "the turned-off sweep failed"
  running "$(pane_id dev-idle agent)" || fail "a sweep turned off froze a workspace"

  # The other workspaces' sidebars are asked to redraw, and show it paused.
  # The stub fzf does not listen, so the test runs the redraw fzf would.
  rows_file="$(tmux show-options -pqv -t "$(picker dev-working)" @devws_picker_rows)"
  rm -f "$rows_file.stale"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  [ -e "$rows_file.stale" ] || fail "freezing did not ask the sidebars to redraw"
  DEVWS_PICKER_ROWS="$rows_file" "$HOME/.tmuxinator/session_picker.sh" --rows >/dev/null ||
    fail "the sidebar's rows could not be redrawn"
  sidebar_shows dev-working 'dev-idle.*paused' ||
    fail "the sidebar does not show the workspace paused: $(sidebar_rows dev-working)"
  for session in dev-working dev-waiting dev-recent dev-done; do tmux kill-session -t "$session"; done
}

# tmux.conf's hooks: switching to a frozen workspace brings it back at once.
# Running work is never frozen: an agent whose task (no shell around it) keeps
# a core busy holds its workspace awake, though its state says done.
test_a_busy_task_keeps_the_workspace_awake() {
  local session=dev-busy agent
  create_workspace "$session"
  agent="$(pane_id "$session" agent)"
  tmux respawn-pane -k -t "$agent" "STUB_BUSY=1 $HOME/.tmuxinator/agent_runner.sh claude"
  busy() { ps -axo pcpu=,command= | awk '$2 == "yes" && $1 >= 50 { found = 1 } END { exit !found }'; }
  wait_until busy || fail "the agent's task did not start"
  make_idle "$session"
  tmux set-option -t "$session" @devws_agent_state done
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep || fail "the sweep failed"
  [ -z "$(option "$session" @devws_frozen)" ] || fail "a workspace running a task was frozen"
  running "$agent" || fail "the busy agent was stopped"
  tmux kill-session -t "$session"
}

# Another tmux server (a test's, say) never lets go of this server's frozen
# workspaces: each server keeps its own records.
test_records_belong_to_their_server() {
  local session=dev-owned other="$TEST_ROOT/other-bin"
  create_workspace "$session"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$(pane_id "$session" agent)" || fail "the workspace did not freeze"
  mkdir -p "$other"
  printf '#!/usr/bin/env bash\nexec "%s" -L "%s-other" -f /dev/null "$@"\n' "$REAL_TMUX" "$SOCKET" > "$other/tmux"
  chmod +x "$other/tmux"
  PATH="$other:$PATH" tmux new-session -d -s elsewhere 'sleep 600'
  PATH="$other:$PATH" "$SUSPEND" resume --all
  PATH="$other:$PATH" tmux kill-server
  frozen "$(pane_id "$session" agent)" || fail "another server's resume let go of this server's workspace"
  "$SUSPEND" resume "$session"
  running "$(pane_id "$session" agent)" || fail "the workspace's own resume did not bring it back"
  tmux kill-session -t "$session"
}

# A frozen mark without a record (its processes were let go behind its back)
# does not keep the workspace from freezing again.
test_a_stale_frozen_mark_heals() {
  local session=dev-stale
  create_workspace "$session"
  tmux set-option -t "$session" @devws_frozen 1
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$(pane_id "$session" agent)" || fail "a stale mark kept the workspace from freezing"
  "$SUSPEND" resume "$session"
  running "$(pane_id "$session" agent)" || fail "the healed workspace did not resume"
  [ -z "$(option "$session" @devws_frozen)" ] || fail "resume left the mark"
  tmux kill-session -t "$session"
}

# A pane that runs its program with no devws script above it (opened before
# the runners waited on their programs) can't be frozen: an idle one is
# opened again under its runner, the agent continuing its conversation, and
# the next sweep freezes it.
old_style() {
  local pane="$1" program="$2"
  tmux respawn-pane -k -t "$pane" "$program"
  wait_until program_runs "$pane" || fail "the old-style $program did not start"
}

under_runner() {
  ps -o command= -p "$(parent_of "$(program_pid "$1")")" | grep -q "/\.tmuxinator/$2"
}

test_old_style_panes_are_reopened_under_their_runners() {
  local session=dev-old agent editor socket
  create_workspace "$session"
  agent="$(pane_id "$session" agent)"
  editor="$(pane_id "$session" editor)"
  tmux set-option -t "$session" @devws_agent claude
  tmux set-option -t "$session" @devws_editor nvim
  socket="$(option "$session" @devws_editor_socket)"
  old_style "$agent" "$TEST_ROOT/bin/claude"
  old_style "$editor" "$TEST_ROOT/bin/nvim --listen $socket ."
  make_idle "$session"

  # Working: nothing is touched.
  tmux set-option -t "$session" @devws_agent_state working
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  ! under_runner "$agent" agent_runner.sh || fail "a working agent was reopened"
  tmux set-option -u -t "$session" @devws_agent_state

  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  wait_until under_runner "$agent" agent_runner.sh || fail "the idle old-style agent was not reopened under its runner"
  wait_until program_runs "$agent" || fail "the reopened agent did not start"
  grep -qx -- --continue "$TEST_ROOT/claude.args" || fail "the reopened agent did not continue its conversation"
  wait_until under_runner "$editor" editor_runner.sh || fail "the clean old-style editor was not reopened under its runner"
  wait_until program_runs "$editor" || fail "the reopened editor did not start"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$agent" && frozen "$editor" || fail "the reopened panes were not frozen by the next sweep"
  "$SUSPEND" resume "$session"
  tmux kill-session -t "$session"
}

# A workspace frozen with an old-style pane still running (frozen before
# panes were reopened) is brought back, so the pane can be reopened and the
# whole workspace frozen.
test_a_frozen_workspace_with_an_old_style_pane_is_redone() {
  local session=dev-half agent
  create_workspace "$session"
  agent="$(pane_id "$session" agent)"
  tmux set-option -t "$session" @devws_agent claude
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  [ "$(option "$session" @devws_frozen)" = 1 ] || fail "the workspace did not freeze"
  old_style "$agent" "$TEST_ROOT/bin/claude"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  wait_until under_runner "$agent" agent_runner.sh || fail "the frozen workspace's old-style agent was not reopened"
  wait_until program_runs "$agent" || fail "the reopened agent did not start"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$agent" || fail "the reopened agent was not frozen by the next sweep"
  "$SUSPEND" resume "$session"
  tmux kill-session -t "$session"
}

# An old-style editor with unsaved changes, or that can't be asked, is left
# as it is, and the sidebar says the workspace needs reopening to pause.
test_an_old_style_editor_with_changes_is_kept() {
  local session=dev-dirty editor socket rows_file
  create_workspace "$session"
  editor="$(pane_id "$session" editor)"
  tmux set-option -t "$session" @devws_editor nvim
  socket="$(option "$session" @devws_editor_socket)"
  old_style "$editor" "$TEST_ROOT/bin/nvim --listen $socket ."
  echo 2 > "$TEST_ROOT/nvim.modified"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  sleep 0.5
  ! under_runner "$editor" editor_runner.sh || fail "an editor with unsaved changes was reopened"
  [ "$(option "$session" @devws_needs_reopen)" = 1 ] || fail "the workspace is not marked as needing a reopen"
  rows_file="$(tmux show-options -pqv -t "$(picker "$session")" @devws_picker_rows)"
  DEVWS_PICKER_ROWS="$rows_file" "$HOME/.tmuxinator/session_picker.sh" --rows | tr '\0\n' '\n ' |
    grep -q "dev-dirty.*partly paused" || fail "the sidebar does not say the workspace is only partly paused"
  rm -f "$TEST_ROOT/nvim.modified"

  # Without a socket of its own, it can't be asked: kept too.
  old_style "$editor" "$TEST_ROOT/bin/nvim ."
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  sleep 0.5
  ! under_runner "$editor" editor_runner.sh || fail "an editor that could not be asked was reopened"
  "$SUSPEND" resume "$session"
  tmux kill-session -t "$session"
}

test_switching_back_resumes() {
  local session=dev-idle
  grep 'suspend_workspace' "$ROOT/config/tmux/tmux.conf" | grep -v '^#' > "$TEST_ROOT/suspend.conf"
  tmux source-file "$TEST_ROOT/suspend.conf"
  tmux new-session -d -s dev-other -x 200 -y 50 'sleep 600'
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-other"
  wait_until attached dev-other || fail "the client did not attach"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$(pane_id "$session" agent)" || fail "the workspace did not freeze again"
  # The attached one stays, however long ago it was seen.
  make_idle dev-other
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  [ -z "$(option dev-other @devws_frozen)" ] || fail "the attached workspace was frozen"

  tmux switch-client -c "$(tmux list-clients -F '#{client_name}' | head -n 1)" -t "$session"
  wait_until running "$(pane_id "$session" agent)" || fail "switching to the workspace did not resume it"
}

# devws reload restarts the sidebars: a frozen one would be left stopped, so
# every workspace comes back first.
test_reload_resumes_every_workspace() {
  local session=dev-reload
  create_workspace "$session"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  frozen "$(pane_id "$session" agent)" || fail "the workspace to reload did not freeze"
  : > "$HOME/.tmux.conf"
  "$HOME/.tmuxinator/reload_devws.sh" >/dev/null || fail "the reload failed"
  running "$(pane_id "$session" agent)" || fail "the reload left the workspace frozen"
  [ -z "$(option "$session" @devws_frozen)" ] || fail "the reloaded workspace is still marked frozen"
  tmux kill-session -t "$session"
}

# Closing a frozen workspace leaves nothing stopped behind.
test_closing_lets_the_processes_go() {
  local session=dev-closing agent editor
  create_workspace "$session"
  make_idle "$session"
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  agent="$(program_pid "$(pane_id "$session" agent)")"
  editor="$(program_pid "$(pane_id "$session" editor)")"
  [ "$(state_of "$agent")" = T ] || fail "the workspace to close did not freeze"
  tmux kill-session -t "$session"
  wait_until sh -c "! ps -p $agent,$editor -o state= | grep -q T" ||
    fail "closing a frozen workspace left stopped processes: $(ps -o pid=,state=,command= -p "$agent,$editor")"
}

# The header's ticker sweeps every minute, from its first turn on.
test_the_header_ticker_sweeps() {
  local session=dev-ticker
  create_workspace "$session"
  make_idle "$session"
  "$HOME/.tmuxinator/header_ticker.sh" --start
  wait_until frozen "$(pane_id "$session" agent)" || fail "the header's ticker did not freeze the idle workspace"
  kill "$(tmux show-option -gqv @devws_header_ticker)" 2>/dev/null
  tmux kill-session -t "$session"
}

test_an_idle_workspace_is_frozen
test_the_header_ticker_sweeps
test_renewing_the_environment_skips_the_agent
test_resume_gives_the_shells_their_programs_back
test_busy_or_recent_workspaces_stay
test_a_busy_task_keeps_the_workspace_awake
test_records_belong_to_their_server
test_a_stale_frozen_mark_heals
test_old_style_panes_are_reopened_under_their_runners
test_a_frozen_workspace_with_an_old_style_pane_is_redone
test_an_old_style_editor_with_changes_is_kept
test_switching_back_resumes
test_reload_resumes_every_workspace
test_closing_lets_the_processes_go

printf 'Suspend tests passed.\n'
