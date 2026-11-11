#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-tidy.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
TIDY="$ROOT/config/tmuxinator/tidy.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Tidy test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
export DEVWS_HOME="$TEST_ROOT/devws" DEVWS_CLONE_BASE="$TEST_ROOT/remotes/" HOME="$TEST_ROOT/home"
mkdir -p "$HOME"

# gh answers each branch's PR state from gh/pr-<branch>; tmux lists the open
# workspaces' folders from open.roots.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/gh"
cat > "$TEST_ROOT/bin/gh" <<EOF
#!/usr/bin/env bash
[ "\$1 \$2" = "pr view" ] || exit 0
branch="\$(git symbolic-ref --short HEAD)"
[ -f "$TEST_ROOT/gh/pr-\${branch//\//-}" ] || { echo 'no pull requests found' >&2; exit 1; }
cat "$TEST_ROOT/gh/pr-\${branch//\//-}"
EOF
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
[ "\$1" = list-sessions ] && cat "$TEST_ROOT/open.roots" 2>/dev/null
exit 0
EOF
chmod +x "$TEST_ROOT/bin/"*
export PATH="$TEST_ROOT/bin:$PATH"
# Jobs run on their own (job.sh), not through the stub tmux, and post no
# real notifications.
unset TMUX TMUX_PANE
export DEVWS_NOTIFY_COMMAND=true

# acme/app with five branch folders: merged, closed, merged but changed,
# merged but open in a workspace, and one whose PR is still open.
REMOTE="$TEST_ROOT/remotes/acme/app.git"
git init -q --bare -b main "$REMOTE"
SEED="$TEST_ROOT/seed"
git init -q -b main "$SEED"
git -C "$SEED" commit -q --allow-empty -m first
git -C "$SEED" remote add origin "$REMOTE"
git -C "$SEED" push -q origin main
BARE="$("$ROOT/config/tmuxinator/github_repo.sh" ensure acme/app 2>/dev/null)"
for branch in merged closed changed opened open; do
  "$ROOT/config/tmuxinator/worktree.sh" ensure "$BARE" "feat/$branch" origin/main >/dev/null 2>&1
done
printf 'merged\n' > "$TEST_ROOT/gh/pr-feat-merged"
printf 'closed\n' > "$TEST_ROOT/gh/pr-feat-closed"
printf 'merged\n' > "$TEST_ROOT/gh/pr-feat-changed"
printf 'merged\n' > "$TEST_ROOT/gh/pr-feat-opened"
printf 'open\n' > "$TEST_ROOT/gh/pr-feat-open"
printf 'draft\n' > "$DEVWS_HOME/acme/app/app-feat-changed/notes.txt"
# A shared file (.shared, shared_files.sh) linked in is devws's, not a
# change: the merged folder still goes.
mkdir -p "$DEVWS_HOME/acme/app/.shared"
printf '#!/bin/sh\n' > "$DEVWS_HOME/acme/app/.shared/sync-deploy.sh"
"$ROOT/config/tmuxinator/shared_files.sh" link "$DEVWS_HOME/acme/app" "$DEVWS_HOME/acme/app/app-feat-merged" >/dev/null
printf '%s\n' "$DEVWS_HOME/acme/app/app-feat-opened" > "$TEST_ROOT/open.roots"

test_candidates_are_merged_or_closed_clean_and_not_open() {
  local folders
  folders="$("$TIDY" candidates | cut -f2 | LC_ALL=C sort | tr '\n' ' ')"
  [ "$folders" = "$DEVWS_HOME/acme/app/app-feat-closed $DEVWS_HOME/acme/app/app-feat-merged " ] ||
    fail "candidates: $folders"
  grep -q $'^app  *feat/merged  *PR merged\t' <<<"$("$TIDY" candidates)" || fail "a row does not say why: $("$TIDY" candidates)"
}

# Asked, only the ticked folders go, in a background job (the stub fzf keeps
# the closed one ticked).
test_the_ticked_folders_go_in_a_job() {
  local attempt
  export DEVWS_JOBS_DIR="$TEST_ROOT/jobs"
  printf '#!/usr/bin/env bash\ngrep feat/closed\n' > "$TEST_ROOT/bin/fzf"
  chmod +x "$TEST_ROOT/bin/fzf"
  "$TIDY" >/dev/null || fail "tidy failed"
  for attempt in $(seq 1 50); do
    [ -e "$DEVWS_HOME/acme/app/app-feat-closed" ] || break
    sleep 0.1
  done
  [ ! -e "$DEVWS_HOME/acme/app/app-feat-closed" ] || fail "the ticked folder was not removed"
  [ -d "$DEVWS_HOME/acme/app/app-feat-merged" ] || fail "an unticked folder was removed"
  grep -q 'Tidy up 1 folder' "$DEVWS_JOBS_DIR"/*/title || fail "no tidy job"
}

test_remove_keeps_the_branches() {
  "$TIDY" remove "$DEVWS_HOME/acme/app/app-feat-merged" >/dev/null || fail "remove failed"
  [ ! -e "$DEVWS_HOME/acme/app/app-feat-merged" ] || fail "the folder is still there"
  git -C "$BARE" show-ref --verify --quiet refs/heads/feat/merged || fail "tidying deleted the branch"
  [ -d "$DEVWS_HOME/acme/app/app-feat-open" ] || fail "an unlisted folder was removed"
}

test_a_folder_with_changes_is_kept() {
  ! "$TIDY" remove "$DEVWS_HOME/acme/app/app-feat-changed" >/dev/null 2>&1 || fail "a removal with changes succeeded"
  [ -f "$DEVWS_HOME/acme/app/app-feat-changed/notes.txt" ] || fail "the changes were lost"
}

test_candidates_are_merged_or_closed_clean_and_not_open
test_the_ticked_folders_go_in_a_job
test_remove_keeps_the_branches
test_a_folder_with_changes_is_kept

printf 'Tidy tests passed.\n'
