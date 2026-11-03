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
# The recent folders (recent.sh) are this test's own.
export HOME="$TEST_ROOT/home"
mkdir -p "$HOME"

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
  "api user") printf 'me\n' ;;
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
# A stub fzf answering from a queue: the first line it is offered that starts
# with the answer, or every line for "@all".
cat > "$FAKE_BIN/fzf" <<EOF
#!/usr/bin/env bash
answer="\$(head -n 1 "$TEST_ROOT/fzf.answers")"
sed -i '' 1d "$TEST_ROOT/fzf.answers"
input="\$(cat)"
# The owners list, for the tests to look at.
case "\$input" in *"My repos"*) printf '%s\n' "\$input" > "$TEST_ROOT/fzf.owners" ;; esac
case "\$input" in *"Type a path"*) printf '%s\n' "\$input" > "$TEST_ROOT/fzf.local" ;; esac
case "\$input" in "+ New branch…"*) printf '%s\n' "\$input" > "$TEST_ROOT/fzf.branches" ;; esac
# An empty answer is Esc.
[ -n "\$answer" ] || exit 130
if [ "\$answer" = @all ]; then printf '%s\n' "\$input"; else printf '%s\n' "\$input" | grep -m 1 -F -- "\$answer"; fi
EOF
chmod +x "$FAKE_BIN/fzf"
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

test_owners_put_mine_first_then_the_orgs() {
  local expected
  expected="me"$'\t'"3"$'\t'"mine"$'\n'"acme"$'\t'"2"$'\t'
  [ "$(github_repo owners)" = "$expected" ] || fail "owners: $(github_repo owners)"
  [ "$(github_repo list --owner acme | tr '\n' ' ')" = "acme/app acme/tools " ] ||
    fail "one owner's repos: $(github_repo list --owner acme | tr '\n' ' ')"
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

# The first workspace of a repo imports its untracked files from a checkout
# you have; the new branch's folder gets them.
test_a_new_workspace_gets_the_imported_files() {
  local checkout="$TEST_ROOT/projects/app" folder
  export DEVWS_PROJECTS="$TEST_ROOT/projects"
  git init -q -b main "$checkout"
  git -C "$checkout" remote add origin git@github.com:acme/app.git
  printf '.env\n' > "$checkout/.gitignore"
  printf 'APP_KEY=x\n' > "$checkout/.env"

  printf '%s\n' "$checkout" @all '+ New branch' '+ New branch' feat > "$TEST_ROOT/fzf.answers"
  folder="$(printf 'seeded\n' | "$GITHUB_WORKSPACE" acme/app 2>/dev/null)" ||
    fail "the GitHub workspace flow failed"
  [ "$folder" = "$DEVWS_HOME/acme/app/app-feat-seeded" ] || fail "the flow gave $folder"
  [ "$(readlink "$folder/.env")" = "$DEVWS_HOME/acme/app/.shared/.env" ] ||
    fail "the new branch did not get the imported .env"

  # Asked once: the next workspace goes straight to the worktrees.
  printf '%s\n' 'feat/seeded' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" acme/app 2>/dev/null </dev/null)" = "$folder" ] ||
    fail "the import was asked again, or the worktree was not reopened"
}

# The start screen: Branch from GitHub, then the owner, then its repo (Esc
# there goes back to the owners), then + New branch and its name (Esc on the
# kind goes back to the branches).
test_a_new_branch_is_picked_by_owner() {
  printf '%s\n' '⎇ Branch' '  acme' '' '  acme' '● app' '+ New branch' '' '+ New branch' 'fix' \
    > "$TEST_ROOT/fzf.answers"
  [ "$(printf 'by owner\n' | "$GITHUB_WORKSPACE" 2>/dev/null)" = "$DEVWS_HOME/acme/app/app-fix-by-owner" ] ||
    fail "acme, then app, then fix 'by owner' did not make app-fix-by-owner"
  grep -q '★ My repos (me)' "$TEST_ROOT/fzf.owners" || fail "your repos are not first: $(cat "$TEST_ROOT/fzf.owners")"
}

# The start screen lists the folders opened before, newest first, and the
# worktrees never opened; picking one reopens it, asking nothing more.
test_the_start_screen_reopens_a_folder() {
  local folder="$DEVWS_HOME/acme/app/app-feat-seeded"
  "$ROOT/config/tmuxinator/recent.sh" add "$folder" codex vim
  printf '%s\n' 'feat/seeded' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" 2>/dev/null </dev/null)" = "$folder" ] || fail "the recent folder was not reopened"
  # Esc on the owners goes back to the start screen, not out.
  printf '%s\n' '⎇ Branch' '' 'feat/seeded' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" 2>/dev/null </dev/null)" = "$folder" ] ||
    fail "Esc on the owners did not go back to the start screen"
}

# Open a local folder: one in DEVWS_PROJECTS (a git repo shows its branch) or
# a typed path; a path that does not exist goes back to the start screen.
test_the_start_screen_opens_a_local_folder() {
  local projects="$TEST_ROOT/local-projects"
  mkdir -p "$projects/notes"
  git init -q -b trunk "$projects/shop"
  export DEVWS_PROJECTS="$projects"
  printf '%s\n' '⌂ Open a local folder' 'shop' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" 2>/dev/null </dev/null)" = "$projects/shop" ] || fail "the project folder was not opened"
  grep -q '^shop .*⎇ trunk' "$TEST_ROOT/fzf.local" || fail "the repo's branch is not shown: $(cat "$TEST_ROOT/fzf.local")"
  grep -q '^notes ' "$TEST_ROOT/fzf.local" || fail "a plain folder is not listed"

  printf '%s\n' '⌂ Open a local folder' '✎ Type a path' > "$TEST_ROOT/fzf.answers"
  [ "$(printf '%s\n' "$projects/notes" | "$GITHUB_WORKSPACE" 2>/dev/null)" = "$projects/notes" ] ||
    fail "a typed path was not opened"

  # Missing: said, then back to the start screen, where Esc leaves.
  printf '%s\n' '⌂ Open a local folder' '✎ Type a path' '' > "$TEST_ROOT/fzf.answers"
  ! printf '%s\n\n' "$projects/missing" | "$GITHUB_WORKSPACE" >/dev/null 2>&1 ||
    fail "a missing folder was opened"
  unset DEVWS_PROJECTS
}

# The GitHub branches, newest commit first, with how long ago; not origin/HEAD.
test_branches_lists_remote_branches_newest_first() {
  local bare="$DEVWS_HOME/acme/app/.bare" listed
  GIT_COMMITTER_DATE='2030-01-01T00:00:00' git -C "$SEED" commit -q --allow-empty -m newer
  git -C "$SEED" push -q origin HEAD:refs/heads/fix/newest
  github_repo ensure acme/app >/dev/null 2>&1 || fail "the repo did not fetch"
  listed="$(github_repo branches "$bare")"
  [ "$(printf '%s\n' "$listed" | head -n 1 | cut -f 1)" = fix/newest ] || fail "the newest branch is not first: $listed"
  printf '%s\n' "$listed" | cut -f 1 | grep -qx feat/remote || fail "feat/remote is not listed: $listed"
  printf '%s\n' "$listed" | cut -f 1 | grep -qx main || fail "main is not listed: $listed"
  ! printf '%s\n' "$listed" | cut -f 1 | grep -qx HEAD || fail "origin/HEAD is listed: $listed"
  printf '%s\n' "$listed" | head -n 1 | cut -f 2 | grep -q . || fail "no date for the branch: $listed"
}

# A branch already on GitHub opens in a folder of its own, tracking it; picked
# again, it is marked and its folder reopened.
test_an_existing_github_branch_is_opened() {
  local folder="$DEVWS_HOME/acme/app/app-feat-remote"
  printf '%s\n' '⎇ Branch' '  acme' '● app' 'feat/remote' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" 2>/dev/null </dev/null)" = "$folder" ] || fail "feat/remote did not open in $folder"
  [ "$(git -C "$folder" rev-parse --abbrev-ref '@{u}')" = origin/feat/remote ] ||
    fail "feat/remote does not track GitHub's branch"
  grep -q '^+ New branch…' "$TEST_ROOT/fzf.branches" || fail "+ New branch is not first: $(cat "$TEST_ROOT/fzf.branches")"

  printf '%s\n' '⎇ Branch' '  acme' '● app' 'feat/remote' > "$TEST_ROOT/fzf.answers"
  [ "$("$GITHUB_WORKSPACE" 2>/dev/null </dev/null)" = "$folder" ] || fail "feat/remote's folder was not reopened"
  grep -q '^feat/remote.*● folder' "$TEST_ROOT/fzf.branches" || fail "the open branch is not marked: $(cat "$TEST_ROOT/fzf.branches")"
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
test_owners_put_mine_first_then_the_orgs
test_worktrees_lists_branch_and_folder
test_pr_state_is_queried_then_cached
test_a_new_workspace_gets_the_imported_files
test_a_new_branch_is_picked_by_owner
test_the_start_screen_reopens_a_folder
test_the_start_screen_opens_a_local_folder
test_branches_lists_remote_branches_newest_first
test_an_existing_github_branch_is_opened
test_branch_names_from_a_prefix_and_description

printf 'GitHub repo tests passed.\n'
