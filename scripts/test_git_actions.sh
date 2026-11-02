#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-git-actions.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
GIT_ACTIONS="$ROOT/config/tmuxinator/git_actions.sh"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-git-actions-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Git actions test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com

# tmux goes to an isolated server; fzf keeps every line it is offered except
# those listed in fzf.drop (the files the user unticks); gh answers from files.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/gh" "$TEST_ROOT/home"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
cat > "$TEST_ROOT/bin/fzf" <<EOF
#!/usr/bin/env bash
if [ -s "$TEST_ROOT/fzf.drop" ]; then grep -vxF -f "$TEST_ROOT/fzf.drop" || true; else cat; fi
EOF
cat > "$TEST_ROOT/bin/gh" <<EOF
#!/usr/bin/env bash
data="$TEST_ROOT/gh"
printf '%s\n' "\$*" >> "\$data/calls"
case "\$1 \$2" in
  "pr view")
    case " \$* " in *" --web "*) exit 0 ;; esac
    if [ -f "\$data/pr" ]; then
      case " \$* " in *" url"*) printf 'https://github.com/acme/app/pull/7\n' ;; *) cat "\$data/pr" ;; esac
    else
      printf 'no pull requests found for branch\n' >&2
      exit 1
    fi
    ;;
  "pr create") printf 'open\n' > "\$data/pr" ;;
esac
EOF
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/fzf" "$TEST_ROOT/bin/gh"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX TMUX_PANE

# A worktree-like repo on feat/login with a GitHub-ish origin.
REMOTE="$TEST_ROOT/remote.git"
REPO="$TEST_ROOT/app"
git init -q --bare -b main "$REMOTE"
git init -q -b main "$REPO"
git -C "$REPO" commit -q --allow-empty -m first
git -C "$REPO" remote add origin "$REMOTE"
git -C "$REPO" push -q origin main
git -C "$REPO" switch -q -c feat/login

# A workspace whose agent pane records what it is sent.
tmux new-session -d -s dev-app -x 120 -y 30 -c "$REPO" "cat > '$TEST_ROOT/agent.input'"
AGENT_PANE="$(tmux display-message -p -t dev-app: '#{pane_id}')"
tmux set-option -p -t "$AGENT_PANE" @devws_role agent
tmux set-option -t dev-app @devws_root "$REPO"
tmux set-option -t dev-app @devws_agent claude
tmux split-window -d -t dev-app: 'sleep 600'
TERMINAL_PANE="$(tmux list-panes -t dev-app: -F '#{pane_id}' | grep -vx "$AGENT_PANE")"

# Runs an action from the workspace's terminal pane.
in_workspace() {
  TMUX="fake,1,0" TMUX_PANE="$TERMINAL_PANE" "$GIT_ACTIONS" "$@"
}

test_nothing_to_commit() {
  in_workspace commit --manual </dev/null | grep -q 'Nothing to commit' ||
    fail "a clean tree did not say so"
}

test_manual_commit_commits_only_the_ticked_files() {
  printf 'a\n' > "$REPO/keep.txt"
  printf 'b\n' > "$REPO/skip.txt"
  printf 'skip.txt\n' > "$TEST_ROOT/fzf.drop"
  printf 'feat: add keep\n' | in_workspace commit --manual >/dev/null ||
    fail "the manual commit failed"
  [ "$(git -C "$REPO" log -1 --format=%s)" = "feat: add keep" ] ||
    fail "commit message: $(git -C "$REPO" log -1 --format=%s)"
  [ "$(git -C "$REPO" show --name-only --format= HEAD)" = keep.txt ] ||
    fail "committed: $(git -C "$REPO" show --name-only --format= HEAD)"
  [ "$(git -C "$REPO" status --porcelain)" = '?? skip.txt' ] || fail "skip.txt was touched"
  rm -f "$TEST_ROOT/fzf.drop"
}

test_an_empty_message_commits_nothing() {
  local head
  head="$(git -C "$REPO" rev-parse HEAD)"
  printf '\n' | in_workspace commit --manual >/dev/null 2>&1 || true
  [ "$(git -C "$REPO" rev-parse HEAD)" = "$head" ] || fail "an empty message made a commit"
}

test_an_idle_agent_is_asked_to_commit() {
  tmux set-option -t dev-app -u @devws_agent_state
  in_workspace commit </dev/null >/dev/null || fail "asking the agent failed"
  sleep 0.3
  grep -q 'Commit the current changes' "$TEST_ROOT/agent.input" ||
    fail "the agent got: $(cat "$TEST_ROOT/agent.input")"
}

test_the_commit_skill_is_used_when_present() {
  mkdir -p "$REPO/.claude/skills/commit"
  : > "$REPO/.claude/skills/commit/SKILL.md"
  in_workspace commit </dev/null >/dev/null || fail "asking the agent failed"
  sleep 0.3
  grep -qx '/commit' "$TEST_ROOT/agent.input" || fail "the agent got: $(cat "$TEST_ROOT/agent.input")"
  rm -rf "$REPO/.claude"
}

test_a_busy_agent_falls_back_to_the_popup() {
  : > "$TEST_ROOT/agent.input"
  tmux set-option -t dev-app @devws_agent_state working
  printf 'chore: by hand\n' | in_workspace commit >/dev/null || fail "the fallback commit failed"
  sleep 0.3
  [ ! -s "$TEST_ROOT/agent.input" ] || fail "a busy agent was interrupted"
  [ "$(git -C "$REPO" log -1 --format=%s)" = "chore: by hand" ] || fail "the fallback did not commit"
  tmux set-option -t dev-app -u @devws_agent_state
}

test_push_sets_upstream_and_can_skip_the_pr() {
  printf 'n\n' | in_workspace push >/dev/null 2>&1 || fail "push failed"
  [ "$(git -C "$REPO" rev-parse --abbrev-ref '@{upstream}')" = origin/feat/login ] ||
    fail "the upstream is not origin/feat/login"
  ! grep -q 'pr create' "$TEST_ROOT/gh/calls" || fail "a PR was created after 'n'"
}

test_push_offers_and_creates_a_pr() {
  printf 'd\n' | in_workspace push >/dev/null 2>&1 || fail "push failed"
  grep -q '^pr create --fill --draft' "$TEST_ROOT/gh/calls" || fail "no draft PR: $(cat "$TEST_ROOT/gh/calls")"
  grep -q '^pr view --web' "$TEST_ROOT/gh/calls" || fail "the PR was not opened"
}

# As a background job: the answer comes with the call, nothing is asked.
test_push_with_the_pr_answer_given_asks_nothing() {
  rm -f "$TEST_ROOT/gh/pr"
  : > "$TEST_ROOT/gh/calls"
  git -C "$REPO" commit -q --allow-empty -m more
  in_workspace push --pr no </dev/null >/dev/null 2>&1 || fail "push --pr no failed"
  [ "$(git -C "$REPO" rev-parse origin/feat/login)" = "$(git -C "$REPO" rev-parse HEAD)" ] ||
    fail "push --pr no did not push"
  ! grep -q 'pr create' "$TEST_ROOT/gh/calls" || fail "--pr no created a PR"
  in_workspace push --pr draft </dev/null >/dev/null 2>&1 || fail "push --pr draft failed"
  grep -q '^pr create --fill --draft' "$TEST_ROOT/gh/calls" || fail "--pr draft made no draft PR"
}

test_push_shows_an_existing_pr() {
  : > "$TEST_ROOT/gh/calls"
  in_workspace push </dev/null 2>/dev/null | grep -q 'pull/7' || fail "the existing PR was not shown"
  ! grep -q 'pr create' "$TEST_ROOT/gh/calls" || fail "a second PR was created"
}

test_nothing_to_commit
test_manual_commit_commits_only_the_ticked_files
test_an_empty_message_commits_nothing
test_an_idle_agent_is_asked_to_commit
test_the_commit_skill_is_used_when_present
test_a_busy_agent_falls_back_to_the_popup
test_push_sets_upstream_and_can_skip_the_pr
test_push_offers_and_creates_a_pr
test_push_with_the_pr_answer_given_asks_nothing
test_push_shows_an_existing_pr

printf 'Git actions tests passed.\n'
