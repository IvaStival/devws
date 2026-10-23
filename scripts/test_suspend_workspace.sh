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
# background commands do.
printf '#!/usr/bin/env bash\n[ -n "${STUB_JOB:-}" ] && sh -c "sleep 600; :" &\nexec sleep 600\n' > "$TEST_ROOT/bin/claude"
# The header's sources stay off the network.
printf '#!/usr/bin/env bash\nexit 7\n' > "$TEST_ROOT/bin/curl"
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS"/*.sh "$SCRIPTS"/*.json "$SCRIPTS"/*.md; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
# A minute, for the header's ticker; the sweeps called here say their own.
printf 'DEVWS_AGENTS="claude"\nDEVWS_SUSPEND_AFTER=1\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp"
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
  local session state
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
  for session in dev-working dev-waiting dev-recent dev-done; do
    running "$(pane_id "$session" agent)" || fail "$session was frozen"
  done
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

  # The other workspaces' sidebars show it paused.
  DEVWS_SUSPEND_AFTER=1 "$SUSPEND" sweep
  wait_until sidebar_shows dev-working 'dev-idle.*paused' ||
    fail "the sidebar does not show the workspace paused: $(sidebar_rows dev-working)"
  for session in dev-working dev-waiting dev-recent dev-done; do tmux kill-session -t "$session"; done
}

# tmux.conf's hooks: switching to a frozen workspace brings it back at once.
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
test_switching_back_resumes
test_reload_resumes_every_workspace
test_closing_lets_the_processes_go

printf 'Suspend tests passed.\n'
