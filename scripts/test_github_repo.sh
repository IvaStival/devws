#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-github.XXXXXX")"
# Resolved, as git reports folders: /var is a link to /private/var on macOS.
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
GITHUB_REPO="$ROOT/config/tmuxinator/github_repo.sh"
GITHUB_WORKSPACE="$ROOT/config/tmuxinator/github_workspace.sh"
WORKTREE="$ROOT/config/tmuxinator/worktree.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'GitHub repo test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
# The clones land here and come from local "GitHub" remotes, never the network.
export DEVWS_HOME="$TEST_ROOT/devws"
export DEVWS_CLONE_BASE="$TEST_ROOT/remotes/"

# A stub gh: repo lists per owner, the user's orgs, and PR states per branch,
# all read from files the tests write; every call is logged.
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN" "$TEST_ROOT/gh"
cat > "$FAKE_BIN/gh" <<EOF
#!/usr/bin/env bash
data="$TEST_ROOT/gh"
printf '%s\n' "\$*" >> "\$data/calls"
case "\$1 \$2" in
  "repo list") owner="\${3:-me}"; [ "\${owner#-}" = "\$owner" ] || owner=me; cat "\$data/repos-\$owner" 2>/dev/null ;;
  "api user/orgs") cat "\$data/orgs" 2>/dev/null ;;
  "config get") printf 'https\n' ;;
  "pr view")
    branch="\$(git symbolic-ref --short HEAD)"
    if [ -f "\$data/pr-\${branch//\//-}" ]; then
      cat "\$data/pr-\${branch//\//-}"
    else
      printf 'no pull requests found for branch "%s"\n' "\$branch" >&2
      exit 1
    fi
    ;;
esac
EOF
chmod +x "$FAKE_BIN/gh"
export PATH="$FAKE_BIN:$PATH"

# acme/app on "GitHub", with main and a feature branch of its own.
REMOTE="$TEST_ROOT/remotes/acme/app.git"
SEED="$TEST_ROOT/seed"
git init -q --bare -b main "$REMOTE"
git init -q -b main "$SEED"
git -C "$SEED" commit -q --allow-empty -m first
git -C "$SEED" remote add origin "$REMOTE"
git -C "$SEED" push -q origin main main:refs/heads/feat/remote

github_repo() {
  "$GITHUB_REPO" "$@"
}

test_home_comes_from_the_environment() {
  [ "$(github_repo home)" = "$DEVWS_HOME" ] || fail "home is $(github_repo home)"
}

test_parse_accepts_names_and_urls() {
  local input
  for input in acme/app https://github.com/acme/app https://github.com/acme/app.git \
    https://github.com/acme/app/ git@github.com:acme/app.git ssh://git@github.com/acme/app.git; do
    [ "$(github_repo parse "$input")" = acme/app ] || fail "$input parsed as $(github_repo parse "$input")"
  done
  ! github_repo parse 'acme' 2>/dev/null || fail "a name without owner was accepted"
  ! github_repo parse '../app' 2>/dev/null || fail "a path was accepted"
  ! github_repo parse 'acme/a b' 2>/dev/null || fail "a name with a space was accepted"
}

test_ensure_clones_a_bare_repo_then_fetches() {
  local bare
  bare="$(github_repo ensure acme/app 2>/dev/null)" || fail "ensure failed"
  [ "$bare" = "$DEVWS_HOME/acme/app/.bare" ] || fail "cloned into $bare"
  [ "$(git -C "$bare" rev-parse --is-bare-repository)" = true ] || fail "the clone is not bare"
  git -C "$bare" show-ref --verify --quiet refs/remotes/origin/feat/remote ||
    fail "the clone has no remote-tracking branches"
  [ "$(github_repo default-branch "$bare")" = main ] || fail "default branch: $(github_repo default-branch "$bare")"

  git -C "$SEED" commit -q --allow-empty -m second
  git -C "$SEED" push -q origin main
  github_repo ensure https://github.com/acme/app.git >/dev/null 2>&1 || fail "the second ensure failed"
  [ "$(git -C "$bare" rev-parse origin/main)" = "$(git -C "$SEED" rev-parse main)" ] ||
    fail "the second ensure did not fetch"
}

test_a_failed_clone_leaves_nothing_behind() {
  ! github_repo ensure acme/missing 2>/dev/null || fail "a missing repo was cloned"
  [ ! -e "$DEVWS_HOME/acme/missing" ] || fail "a failed clone left a folder"
}

test_list_puts_cloned_repos_first_and_caches() {
  printf 'me/zeta\nme/alpha\n' > "$TEST_ROOT/gh/repos-me"
  printf 'acme\n' > "$TEST_ROOT/gh/orgs"
  printf 'acme/app\nacme/tools\n' > "$TEST_ROOT/gh/repos-acme"
  : > "$TEST_ROOT/gh/calls"
  [ "$(github_repo list | tr '\n' ' ')" = "acme/app acme/tools me/alpha me/zeta " ] ||
    fail "list: $(github_repo list | tr '\n' ' ')"
  printf 'me/new\n' >> "$TEST_ROOT/gh/repos-me"
  github_repo list | grep -qx me/new && fail "the cached list asked gh again"
  github_repo list --refresh | grep -qx me/new || fail "--refresh did not ask gh"
}

test_worktrees_lists_branch_and_folder() {
  local bare="$DEVWS_HOME/acme/app/.bare" folder
  folder="$("$WORKTREE" ensure "$bare" feat/login origin/main 2>/dev/null)"
  [ "$(github_repo worktrees "$bare")" = "feat/login"$'\t'"$folder" ] ||
    fail "worktrees: $(github_repo worktrees "$bare")"
}

test_pr_state_is_queried_then_cached() {
  local folder="$DEVWS_HOME/acme/app/app-feat-login"
  [ -z "$(github_repo pr "$folder")" ] || fail "a never-checked branch has a PR state"
  [ "$(github_repo pr-query "$folder")" = none ] || fail "a branch without a PR is not 'none'"
  printf 'open\n' > "$TEST_ROOT/gh/pr-feat-login"
  # Checked moments ago: the refresh leaves it, and reports no change.
  ! github_repo pr-refresh "$folder" || fail "a fresh state was refreshed"
  [ "$(github_repo pr "$folder")" = none ] || fail "the fresh cache changed"
  [ "$(github_repo pr-query "$folder")" = open ] || fail "the PR was not found"
  [ "$(github_repo pr "$folder")" = open ] || fail "the queried state was not cached"
}

test_branch_names_from_a_prefix_and_description() {
  [ "$("$GITHUB_WORKSPACE" branch-name feat 'Login page: OAuth & SSO!')" = feat/login-page-oauth-sso ] ||
    fail "slug: $("$GITHUB_WORKSPACE" branch-name feat 'Login page: OAuth & SSO!')"
  ! "$GITHUB_WORKSPACE" branch-name fix '  !!  ' >/dev/null 2>&1 || fail "an empty description made a branch"
}

test_home_comes_from_the_environment
test_parse_accepts_names_and_urls
test_ensure_clones_a_bare_repo_then_fetches
test_a_failed_clone_leaves_nothing_behind
test_list_puts_cloned_repos_first_and_caches
test_worktrees_lists_branch_and_folder
test_pr_state_is_queried_then_cached
test_branch_names_from_a_prefix_and_description

printf 'GitHub repo tests passed.\n'
