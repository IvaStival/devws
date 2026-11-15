#!/usr/bin/env bash
# The commands a program (devAgentics) drives devws with: `devws open`,
# `devws agent …` and `devws worktree ensure`, run through bin/devws and the
# real tmuxinator on an isolated tmux server.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Physical: devws resolves folders with pwd -P, and /var is /private/var here.
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-cli.XXXXXX")" && pwd -P)"
SCRIPTS_SRC="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-cli-test-$$"
DEVWS="$ROOT/bin/devws"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'devws CLI test failed: %s\n' "$*" >&2
  exit 1
}

command -v tmuxinator >/dev/null 2>&1 || {
  printf 'devws CLI tests skipped: tmuxinator is not installed\n'
  exit 0
}
command -v zsh >/dev/null 2>&1 || {
  printf 'devws CLI tests skipped: zsh is not installed\n'
  exit 0
}

# Every tmux call goes to an isolated server. The agents and Neovim are stubs
# that record their arguments, NUL-separated, and wait; fzf is one too, for
# the sidebar.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/projects" "$TEST_ROOT/started"
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
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS_SRC"/*.sh "$SCRIPTS_SRC"/*.json "$SCRIPTS_SRC"/*.md "$SCRIPTS_SRC"/*.yml; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
printf 'DEVWS_AGENTS="claude codex"\nDEVWS_EDITORS="nvim"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
export DEVWS_HOME="$TEST_ROOT/devws" DEVWS_CLONE_BASE="$TEST_ROOT/remotes/"
export DEVWS_NOTIFY_COMMAND=true DEVWS_AUTO_RESTORE=0
unset TMUX

wait_until() {
  local attempt
  for attempt in $(seq 1 50); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

status_of() {
  local code=0
  "$@" >/dev/null 2>&1 || code=$?
  printf '%s' "$code"
}

RS=$'\036'

# The newest recording of a stub, its arguments each followed by RS.
started_args() {
  local file
  file="$(ls -t "$TEST_ROOT/started/$1".* 2>/dev/null | head -1)"
  [ -n "$file" ] || return 1
  tr '\0' "$RS" < "$file"
}

session_option() {
  tmux show-option -qv -t "=$1:" "$2"
}

new_folder() {
  mkdir -p "$TEST_ROOT/projects/$1"
  printf '%s' "$TEST_ROOT/projects/$1"
}

test_open_starts_a_detached_workspace_for_a_task() {
  local folder args session
  folder="$(new_folder open-me)"
  args="$TEST_ROOT/open-args"
  printf '%s\0' --model opus > "$args"

  session="$("$DEVWS" open --no-attach --task team-1 --agent-args "$args" "$folder" claude nvim </dev/null)" ||
    fail "devws open failed"
  [ "$session" = dev-open-me ] || fail "open printed '$session', not the session's name"
  tmux has-session -t "=$session" || fail "open did not start the workspace"
  [ "$(session_option "$session" @devws_task)" = team-1 ] || fail "open did not set the task"
  wait_until test "$(session_option "$session" @devws_agent)" = claude || fail "open did not record the agent"
  wait_until started_args claude >/dev/null || fail "the agent did not start"
  case "$(started_args claude)" in
    *"--append-system-prompt$RS"*"${RS}--model${RS}opus$RS") ;;
    *) fail "the agent did not get devws's and the file's arguments: $(started_args claude)" ;;
  esac
  [ "$(tmux list-clients | wc -l | tr -d ' ')" = 0 ] || fail "open attached a client"
  ! grep -qF "$folder" "$HOME/.devws-state/recent.tsv" 2>/dev/null || fail "a task's folder reached the recent folders"
  wait_until grep -qF "$folder" "$HOME/.devws-state/sessions.tsv" || fail "open did not save the workspace"
  grep -F "$session" "$HOME/.devws-state/sessions.tsv" | grep -q . || fail "the saved workspace is missing"
}

test_open_without_a_task_takes_the_defaults_and_is_remembered() {
  local folder session
  folder="$(new_folder plain)"
  session="$("$DEVWS" open --no-attach "$folder" </dev/null)" || fail "devws open without an agent failed"
  [ "$session" = dev-plain ] || fail "open printed '$session'"
  [ -z "$(session_option "$session" @devws_task)" ] || fail "a workspace got a task it was not given"
  wait_until test "$(session_option "$session" @devws_agent)" = claude || fail "open did not use the first agent"
  grep -qF "$folder" "$HOME/.devws-state/recent.tsv" || fail "open did not add the folder to the recent ones"
}

test_open_refuses_what_it_cannot_do() {
  local folder
  folder="$(new_folder refused)"
  [ "$(status_of "$DEVWS" open)" = 2 ] || fail "open without a folder was accepted"
  [ "$(status_of "$DEVWS" open --no-attach)" = 2 ] || fail "open with options and no folder was accepted"
  [ "$(status_of "$DEVWS" open --bogus "$folder")" = 2 ] || fail "open accepted an unknown option"
  [ "$(status_of "$DEVWS" open --no-attach --task 'a b' "$folder")" = 2 ] || fail "open accepted a task id with a space"
  [ "$(status_of "$DEVWS" open --no-attach --agent-args "$TEST_ROOT/missing" "$folder")" = 1 ] ||
    fail "open accepted an unreadable arguments file"
  [ "$(status_of "$DEVWS" open --no-attach "$folder" nobody)" = 2 ] || fail "open accepted an unregistered agent"
  [ "$(status_of "$DEVWS" open --no-attach "$TEST_ROOT/nowhere")" = 1 ] || fail "open accepted a missing folder"
  ! tmux has-session -t =dev-refused 2>/dev/null || fail "a refused open started a workspace"
}

test_agent_commands_are_reached_through_devws() {
  [ "$("$DEVWS" agent state %999)" = gone ] || fail "devws agent state did not reach agent_cli.sh"
  [ "$(status_of "$DEVWS" agent bogus)" = 2 ] || fail "devws agent accepted an unknown command"
}

test_worktree_ensure_makes_a_worktree_and_opens_nothing() {
  local repo folder again
  repo="$(new_folder repo)"
  git init -q -b main "$repo"
  git -C "$repo" -c user.name=t -c user.email=t@t commit -q --allow-empty -m first

  folder="$("$DEVWS" worktree ensure "$repo" feat/x)" || fail "worktree ensure on a folder failed"
  [ "$folder" = "$repo-feat-x" ] || fail "worktree ensure printed '$folder'"
  [ "$(git -C "$folder" symbolic-ref --short HEAD)" = feat/x ] || fail "the worktree is not on the branch"
  again="$("$DEVWS" worktree ensure "$repo" feat/x)" || fail "worktree ensure twice failed"
  [ "$again" = "$folder" ] || fail "a second ensure printed '$again'"
  ! tmux has-session -t =dev-repo-feat-x 2>/dev/null || fail "worktree ensure opened a workspace"

  [ "$(status_of "$DEVWS" worktree ensure "$repo")" = 2 ] || fail "ensure without a branch was accepted"
}

test_worktree_ensure_clones_a_github_repo() {
  local remote seed folder
  remote="$TEST_ROOT/remotes/acme/app.git"
  seed="$TEST_ROOT/seed"
  git init -q --bare -b main "$remote"
  git init -q -b main "$seed"
  git -C "$seed" -c user.name=t -c user.email=t@t commit -q --allow-empty -m first
  git -C "$seed" remote add origin "$remote"
  git -C "$seed" push -q origin main

  folder="$("$DEVWS" worktree ensure acme/app feat/team 2>/dev/null)" || fail "worktree ensure on a GitHub repo failed"
  [ "$folder" = "$DEVWS_HOME/acme/app/app-feat-team" ] || fail "the GitHub worktree is at '$folder'"
  [ "$(git -C "$folder" symbolic-ref --short HEAD)" = feat/team ] || fail "the GitHub worktree is not on the branch"
  # Off the fetched default branch.
  [ "$(git -C "$folder" rev-parse HEAD)" = "$(git -C "$seed" rev-parse main)" ] || fail "the branch did not start from main"
}

# `devws` alone, outside tmux: with a workspace running, it joins the first
# one (an agent team's group session is not one) and asks nothing; with none,
# it shows the start screen. On a tmux server of its own, whose attach is
# recorded: the tests have no terminal to attach.
test_devws_alone_joins_the_first_running_workspace() {
  local socket="$SOCKET-attach" start_screen="$HOME/.tmuxinator/github_workspace.sh"
  local attach_tmux="$TEST_ROOT/attach-bin/tmux"
  mkdir -p "$TEST_ROOT/attach-bin"
  cat > "$attach_tmux" <<EOF
#!/usr/bin/env bash
[ "\$1" = attach-session ] && { printf '%s\n' "\$*" > "$TEST_ROOT/attached"; exit 0; }
exec "$REAL_TMUX" -L "$socket" -f /dev/null "\$@"
EOF
  chmod +x "$attach_tmux"
  rm "$start_screen"
  printf '#!/usr/bin/env bash\ntouch "%s"\nexit 1\n' "$TEST_ROOT/start-screen" > "$start_screen"
  chmod +x "$start_screen"
  alone() { PATH="$TEST_ROOT/attach-bin:$PATH" TMUX= zsh "$DEVWS" </dev/null >/dev/null 2>&1 || true; }

  "$attach_tmux" new-session -d -s dev-a-floor
  "$attach_tmux" set-option -t =dev-a-floor: @devws_team_of dev-b
  alone
  [ ! -e "$TEST_ROOT/attached" ] || fail "devws joined an agent team's group session: $(cat "$TEST_ROOT/attached")"
  [ -e "$TEST_ROOT/start-screen" ] || fail "devws without a workspace running did not show the start screen"

  "$attach_tmux" new-session -d -s dev-b
  "$attach_tmux" new-session -d -s dev-c
  rm -f "$TEST_ROOT/start-screen"
  alone
  [ "$(cat "$TEST_ROOT/attached" 2>/dev/null)" = "attach-session -t =dev-b" ] ||
    fail "devws did not join the first workspace: '$(cat "$TEST_ROOT/attached" 2>/dev/null)'"
  [ ! -e "$TEST_ROOT/start-screen" ] || fail "devws showed the start screen with a workspace running"

  "$REAL_TMUX" -L "$socket" kill-server 2>/dev/null || true
  rm "$start_screen"
  ln -s "$SCRIPTS_SRC/github_workspace.sh" "$start_screen"
}

test_devws_alone_joins_the_first_running_workspace
test_open_starts_a_detached_workspace_for_a_task
test_open_without_a_task_takes_the_defaults_and_is_remembered
test_open_refuses_what_it_cannot_do
test_agent_commands_are_reached_through_devws
test_worktree_ensure_makes_a_worktree_and_opens_nothing
test_worktree_ensure_clones_a_github_repo

printf 'devws CLI tests passed\n'
