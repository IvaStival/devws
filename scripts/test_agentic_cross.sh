#!/usr/bin/env bash
# The real devws and the real devagentics (agentic/) together, on an isolated
# tmux server, with stub agents. Nothing here touches the user's tmux or
# accounts. Skipped without cargo.
set -euo pipefail

DEVWS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { printf 'Agentic cross test skipped: cargo not found.\n'; exit 0; }
cargo build --quiet --manifest-path "$DEVWS_ROOT/agentic/Cargo.toml"
DA_BIN="$DEVWS_ROOT/agentic/target/debug"
ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/cross.XXXXXX")" && pwd -P)"
SOCKET="cross-$$"
REAL_TMUX="$(command -v tmux)"
# The panes' processes may still write into ROOT for a moment after the server
# dies, so removing it is retried instead of failing the run.
cleanup() {
  local attempt
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  for attempt in 1 2 3 4 5; do
    rm -rf "$ROOT" 2>/dev/null && return 0
    sleep 1
  done
  return 0
}
trap cleanup EXIT
fail() {
  printf 'CROSS CHECK FAILED: %s\n' "$*" >&2
  tmux list-panes -a -F '#{pane_id} role=#{@devws_role} cmd=#{pane_start_command}' 2>&1 | head -10 >&2
  ps -axo pid,ppid,command | grep -E 'agent_runner|[c]laude|sleep 600' | grep -v grep | cut -c1-200 >&2
  ls -la "$ROOT/started" >&2
  for pane in $(tmux list-panes -a -F '#{pane_id}' 2>/dev/null); do echo "--- $pane"; tmux capture-pane -p -t "$pane" 2>&1 | grep -v '^$' | head -8; done >&2
  exit 1
}
ok() { printf 'ok  %s\n' "$*"; }

mkdir -p "$ROOT/bin" "$ROOT/home/.tmuxinator" "$ROOT/started" "$ROOT/repo"
cat > "$ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for program in claude codex nvim fzf; do
  cat > "$ROOT/bin/$program" <<EOF
#!/usr/bin/env bash
printf '%s\0' "\$@" > "$ROOT/started/$program.\$\$"
exec sleep 600
EOF
done
chmod +x "$ROOT/bin/"*
ln -s "$DEVWS_ROOT/bin/devws" "$ROOT/bin/devws"
for f in "$DEVWS_ROOT"/config/tmuxinator/*.sh "$DEVWS_ROOT"/config/tmuxinator/*.json "$DEVWS_ROOT"/config/tmuxinator/*.md "$DEVWS_ROOT"/config/tmuxinator/*.yml; do
  ln -s "$f" "$ROOT/home/.tmuxinator/$(basename "$f")"
done
mkdir -p "$ROOT/da/roles"; cp "$DEVWS_ROOT"/agentic/roles/samples/*.md "$ROOT/da/roles/"
printf 'DEVWS_AGENTS="claude codex"\nDEVWS_EDITORS="nvim"\n' > "$ROOT/home/.tmuxinator/.env"

export PATH="$ROOT/bin:$DA_BIN:$PATH" HOME="$ROOT/home"
export DEVAGENTICS_HOME="$ROOT/da" DEVAGENTICS_DEVWS="$ROOT/bin/devws" DEVAGENTICS_TMUX="$ROOT/bin/tmux"
export DEVWS_NOTIFY_COMMAND=true DEVWS_AUTO_RESTORE=0 DEVWS_HOME="$ROOT/devws"
unset TMUX

git -C "$ROOT/repo" init -q -b feat/x
git -C "$ROOT/repo" -c user.name=t -c user.email=t@t commit -q --allow-empty -m first

wait_until() { local n; for n in $(seq 1 60); do "$@" && return 0; sleep 0.25; done; return 1; }
recordings() { ls "$ROOT/started/$1".* 2>/dev/null | wc -l | tr -d ' '; }
at_least() { [ "$(recordings "$1")" -ge "$2" ]; }
args_of() { tr '\0' '\036' < "$(ls -t "$ROOT/started/$1".* | head -1)"; }

# 1. devws agentic, with the real devagentics.
session="$(devws agentic "$ROOT/repo" </dev/null | tail -1)"
[ "$session" = dev-repo ] || fail "devws agentic printed '$session'"
ok "devws agentic opened $session"

wait_until at_least claude 1 || fail "the leader did not start"
leader="$(args_of claude)"
case "$leader" in *"leader of an agent team"*) ;; *) fail "the leader did not get its role: $leader" ;; esac
case "$leader" in *"Task feat-x on branch feat/x"*) ;; *) fail "the leader is not told its task" ;; esac
case "$leader" in *"--mcp-config"*"/da/tasks/feat-x/mcp.json"*) ;; *) fail "the leader has no MCP config" ;; esac
ok "the leader got its role, task and MCP config"

log="$(tmux list-panes -t =dev-repo -F '#{@devws_role} #{pane_pid}' | awk '$1=="team-log"{print $2}')"
[ -n "$log" ] || fail "no log pane"
wait_until sh -c "ps -o command= -p \$(pgrep -P $log | head -1) 2>/dev/null | grep -q 'devagentics board feat-x'" ||
  wait_until sh -c "ps -axo command= | grep -q '[d]evagentics watch feat-x'" || fail "the log pane does not run devagentics watch"
ok "the board pane runs devagentics board feat-x"

[ "$(tmux show-option -qv -t =dev-repo: @devws_task)" = feat-x ] || fail "the workspace has no task"
devagentics task list | grep -q 'feat-x  feat/x  planning' || fail "task list: $(devagentics task list)"
ok "the task is listed as planning"

# 2. The leader's tools, over the real MCP server: a planner.
reply="$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"role":"planner"}}}' |
  devagentics mcp feat-x | tail -1)"
case "$reply" in *"Started planner-1"*) ;; *) fail "spawn_agent: $reply" ;; esac
ok "spawn_agent started planner-1"

pane="$(printf '%s' "$reply" | sed -n 's/.*in pane \(%[0-9]*\).*/\1/p')"
[ -n "$pane" ] || fail "no pane id in: $reply"
[ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] || fail "the planner's panel is not an extra agent"
[ "$(tmux show-options -pqv -t "$pane" @devws_label)" = Planner ] || fail "the planner's panel has no label"
[ "$(tmux show-options -pqv -t "$pane" @devagentics_agent)" = planner-1 ] || fail "the pane is not marked"
[ "$(tmux show-options -pqv -t "$pane" @devws_team)" = 1 ] || fail "the pane is not marked as the team's"
[ "$(tmux display-message -p -t "$pane" '#{session_name}')" = dev-repo-seniors ] || fail "the planner is not in the Seniors workspace"
main="$(tmux list-windows -t =dev-repo -F '#{window_id} #{@devws_main}' | awk '$2 == 1 { print $1 }')"
[ -n "$main" ] || fail "the workspace's window is not marked"
[ "$(tmux display-message -p -t "$main" '#{window_panes}')" = 3 ] ||
  fail "an agent opened in the workspace's window [$main]: $(tmux list-windows -t =dev-repo -F '#{window_id} #{window_name} #{@devws_main} #{window_panes}')"
ok "the planner's panel is labelled, marked, and in its group's workspace"

wait_until at_least claude 2 || fail "the planner did not start"
planner="$(args_of claude)"
case "$planner" in *"Reply with exactly: ready"*"--permission-mode"*) ;; *) fail "the planner's first message: $planner" ;; esac
ok "the planner started with its role and 'ready'"

# 3. The plan, from the planner's pane, then approval.
printf '1. s1: add a file\n' | TMUX_PANE="$pane" devagentics plan write | grep -q 'the user, who approves it' || fail "plan write"
devagentics task approve feat-x | grep -q approved || fail "task approve"
ok "the plan was stored from the planner's pane and approved"

# 4. The dispatcher in the log pane is the only one, and it works the team.
if devagentics watch feat-x --once >/dev/null 2>&1; then fail "a second dispatcher was allowed"; fi
ok "a second dispatcher is refused"

planner_ready() { devagentics task status feat-x | grep -q '"ready": true'; }
devws agent mark "$pane" done
wait_until planner_ready || fail "the dispatcher did not see the planner done"
ok "the dispatcher made the planner ready after its first answer"

reply="$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send_order","arguments":{"agent":"planner-1","step":"plan","title":"Plan it","text":"Write the plan","check":"true"}}}' |
  devagentics mcp feat-x | tail -1)"
case "$reply" in *"queued for planner-1"*) ;; *) fail "send_order: $reply" ;; esac
typed_in() { tmux capture-pane -p -J -t "$pane" | grep -q "$1"; }
wait_until typed_in 'ORDER o1' || fail "the order did not reach the pane: $(tmux capture-pane -p -t "$pane")"
wait_until typed_in 'Write the plan' || fail "the order's text is missing in the pane"
wait_until typed_in 'devagentics report done' || fail "the order's report instruction is missing in the pane"
ok "the order was typed into the planner's real pane, whole"

# 5. The senior's execution plan, accepted by the CTO; a hammer gets a step of
# it, reads the plan and asks the senior, in real panes.
leader_calls() {
  { printf '%s\n' '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}'
    printf '%s\n' "$@"; } | devagentics mcp feat-x
}
printf '1. s1: create a.txt\n' | TMUX_PANE="$pane" devagentics plan write | grep -q 'Execution plan of step plan' ||
  fail "the senior's execution plan was not stored"
TMUX_PANE="$pane" devagentics report done "planned" >/dev/null || fail "the senior's report"
reply="$(leader_calls \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"accept_step","arguments":{"step":"plan"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"role":"executor"}}}' | tail -1)"
case "$reply" in *"Started executor-1"*) ;; *) fail "spawn the hammer: $reply" ;; esac
hammer="$(printf '%s' "$reply" | sed -n 's/.*in pane \(%[0-9]*\).*/\1/p')"
[ "$(tmux display-message -p -t "$hammer" '#{session_name}')" = dev-repo-floor ] || fail "the hammer is not in the Floor workspace"
devws agent mark "$hammer" done
wait_until sh -c "devagentics task status feat-x | grep -A3 '\"executor-1\"' >/dev/null && devagentics task status feat-x | tr -d ' \n' | grep -q '\"id\":\"executor-1\".*\"ready\":true'" ||
  fail "the hammer did not become ready"
reply="$(leader_calls \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send_order","arguments":{"agent":"executor-1","step":"s1","title":"a.txt","text":"Create a.txt","check":"true","plan":"plan"}}}' | tail -1)"
case "$reply" in *"queued for executor-1"*) ;; *) fail "the order to the hammer: $reply" ;; esac
hammer_typed() { tmux capture-pane -p -J -t "$hammer" | grep -q "$1"; }
wait_until hammer_typed 'devagentics plan show plan' || fail "the hammer's order does not name its plan: $(tmux capture-pane -p -t "$hammer")"
[ "$(TMUX_PANE="$hammer" devagentics plan show plan)" = "1. s1: create a.txt" ] || fail "plan show"
TMUX_PANE="$hammer" devagentics ask "Which folder?" | grep -q 'Asked planner-1' || fail "the question did not go to the senior"
devws agent mark "$pane" done
wait_until typed_in 'QUESTION q' || fail "the question was not typed into the senior: $(tmux capture-pane -p -t "$pane")"
question="$(devagentics task status feat-x | sed -n 's/.*"id": "\(q[0-9]*\)".*/\1/p' | head -1)"
TMUX_PANE="$pane" devagentics answer "$question" "the root" >/dev/null || fail "the senior's answer"
devws agent mark "$hammer" done
wait_until hammer_typed 'ANSWER to' || fail "the answer was not typed into the hammer"
ok "a senior's plan went to a hammer, which asked and got the senior's answer"

# 6. Reach an agent: peek it under the board and back, go to its workspace;
# the team's workspace is still saved as its own window.
devws agent peek "$hammer" || fail "peek"
[ "$(tmux display-message -p -t "$hammer" '#{window_id}')" = "$main" ] || fail "the peeked hammer is not in the workspace's window"
devws agent peek "$hammer" || fail "peek back"
[ "$(tmux display-message -p -t "$hammer" '#{session_name}')" = dev-repo-floor ] || fail "the hammer did not go back to Floor"
devws agent go "$hammer" || fail "go"
[ "$(tmux display-message -p -t =dev-repo-floor: '#{pane_id}')" = "$hammer" ] || fail "go did not focus the hammer"
"$HOME/.tmuxinator/save_state.sh"
awk -F '\t' '$1 == "P" && $2 == "dev-repo" && $4 == "team-log"' "$HOME/.devws-state/sessions.tsv" | grep -q . ||
  fail "saving lost the team workspace's own panels: $(cat "$HOME/.devws-state/sessions.tsv")"
ok "an agent is peeked and sent back, shown in its workspace, and the team still saves"

board="$(devagentics board feat-x --once --width 100 2>&1)" || fail "board --once: $board"
for expected in "[overview]" "PROGRESS" "CTO" "L  leader" "SENIORS" "FLOOR" "planner-1" "executor-1"; do
  case "$board" in *"$expected"*) ;; *) fail "the board lacks '$expected': $board" ;; esac
done
case "$board" in *"need you"*) fail "the board asks for you with nothing waiting: $board" ;; esac
ok "the overview shows progress, leader identity and active agents"
board_pane="$(tmux list-panes -t =dev-repo -F '#{pane_id} #{@devws_role}' | awk '$2=="team-log"{print $1}')"
pane_text="$(tmux capture-pane -p -t "$board_pane")"
case "$pane_text" in *"Refresh failed:"*) fail "the board pane shows an error: $pane_text" ;; esac
ok "the live board pane shows no error"

# The Team menu targets the existing board from a group workspace; the board
# consumes the request and opens complete step details without a new dispatcher.
board_shows() { tmux capture-pane -p -t "$board_pane" | grep -qF "$1"; }
TMUX_PANE="$hammer" "$HOME/.tmuxinator/devws_menu.sh" --run team-steps
wait_until board_shows '[steps]' || fail "the board did not consume the Team steps request"
[ -z "$(tmux show-options -pqv -t "$board_pane" @devagentics_board_view)" ] || fail "the board left its view request pending"
tmux send-keys -t "$board_pane" Enter
wait_until board_shows 'Create a.txt' || fail "the board did not show the full step order"
tmux send-keys -t "$board_pane" Escape
TMUX_PANE="$hammer" "$HOME/.tmuxinator/devws_menu.sh" --run team-usage
wait_until board_shows '[usage]' || fail "the board did not consume the Team usage request"
if devagentics watch feat-x --once >/dev/null 2>&1; then fail "view navigation released the dispatcher lock"; fi
ok "Team menu requests reach the live board and full details preserve order text"

# 7. A restart: the sidebar groups the task, restore leaves the team alone.
rows="$(DEVWS_PICKER_CURRENT=dev-repo TMUX_PANE="$board_pane" DEVWS_PICKER_ROWS="$ROOT/rows" "$HOME/.tmuxinator/session_picker.sh" --rows | tr '\0' '\n' | grep -E "^(workspace|task|group|agent):" | cut -f1 | tr '\n' ' ')"
[ "$rows" = "workspace:dev-repo group:dev-repo-seniors agent:$pane group:dev-repo-floor agent:$hammer " ] ||
  fail "sidebar rows: $rows"
ok "the sidebar lists the team's workspaces and agents under its task"

printf 'Agentic cross tests passed.\n'
