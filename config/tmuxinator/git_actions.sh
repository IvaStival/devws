#!/usr/bin/env bash
# Commit and push the workspace's changes: the devws menu's Git tab and
# `devws commit` / `devws push`. They work on the workspace's folder
# (@devws_root), or the current folder outside a workspace.
#
#   git_actions.sh commit            asks the workspace's agent to commit (its
#                                    /commit skill when there is one); a busy
#                                    agent, or none, gets the manual commit
#   git_actions.sh commit --manual   ticks the changed files (Tab unticks one)
#                                    and asks for the message, suggested from
#                                    the branch: feat/login → "feat: "
#   git_actions.sh push              pushes the branch, setting its upstream;
#                                    offers to open a PR when it has none
#
# Nothing here forces a push or adds AI credits to a message.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

commit_types='feat fix chore refactor docs test perf style build ci'
agent_request='Commit the current changes: a conventional commit message, no AI or co-author credits.'

die() {
  printf 'devws: %s\n' "$1" >&2
  exit 2
}

session() {
  [ -n "${TMUX:-}" ] || return 1
  local name
  name="$(tmux display-message -p ${TMUX_PANE:+-t "$TMUX_PANE"} '#{session_name}' 2>/dev/null)" || return 1
  case "$name" in dev-*) printf '%s' "$name" ;; *) return 1 ;; esac
}

# Printed, and on the status bar too: the menu's popup closes when done.
tell() {
  printf '%s\n' "$1"
  [ -n "${TMUX:-}" ] && tmux display-message "$1" 2>/dev/null
  return 0
}

workspace_option() {
  tmux show-options -qv -t "=$1:" "$2" 2>/dev/null
}

root_folder() {
  local workspace root=""
  if workspace="$(session)"; then
    root="$(workspace_option "$workspace" @devws_root)"
  fi
  root="${root:-$PWD}"
  git -C "$root" rev-parse --show-toplevel 2>/dev/null || die "not in a git repo: $root"
}

has_changes() {
  [ -n "$(git -C "$1" status --porcelain --untracked-files=all)" ]
}

# The changed paths, one per line: a rename gives both of its paths, so the
# commit takes the old one's removal too.
changed_paths() {
  local entry
  git -C "$1" status --porcelain -z --untracked-files=all | while IFS= read -r -d '' entry; do
    printf '%s\n' "${entry:3}"
    case "$entry" in
      R* | C*) IFS= read -r -d '' entry && printf '%s\n' "$entry" ;;
    esac
  done
}

suggested_type() {
  local branch="$1" type
  type="${branch%%/*}"
  [ "$type" != "$branch" ] || return 0
  case " $commit_types " in *" $type "*) printf '%s: ' "$type" ;; esac
}

commit_manually() {
  local root="$1" branch message
  local -a files=()
  if ! has_changes "$root"; then
    tell 'Nothing to commit.'
    return 0
  fi
  while IFS= read -r file; do
    [ -n "$file" ] && files+=("$file")
  done < <(
    changed_paths "$root" |
      fzf --multi --height=~60% --layout=reverse --bind 'load:select-all' \
        --prompt='Commit > ' --header='Files to commit (Tab unticks one, Enter commits, Esc cancels)'
  )
  [ "${#files[@]}" -gt 0 ] || { printf 'Nothing selected; no commit.\n'; return 1; }

  branch="$(git -C "$root" symbolic-ref --quiet --short HEAD 2>/dev/null)"
  read -e -r -i "$(suggested_type "$branch")" -p 'Message: ' message || return 1
  message="${message#"${message%%[![:space:]]*}"}"
  case "$message" in
    '' | *:) printf 'No message; no commit.\n' >&2; return 1 ;;
  esac

  git -C "$root" add -A -- "${files[@]}" || return 1
  git -C "$root" commit -q -m "$message" -- "${files[@]}" || return 1
  tell "$(git -C "$root" log -1 --format='Committed %h %s')"
}

agent_pane() {
  tmux list-panes -s -t "=$1:" -F '#{pane_id}|#{@devws_role}' 2>/dev/null |
    awk -F '|' '$2 == "agent" { print $1; exit }'
}

# /commit when the agent can see a commit skill (the project's or its
# account's), else the request in words.
agent_message() {
  local agent="$1" root="$2" kind home
  kind="$(agent_kind "$agent")" || kind=claude
  home="$(agent_home "$agent" 2>/dev/null)"
  if [ "$kind" = claude ] &&
      { [ -f "$root/.claude/skills/commit/SKILL.md" ] || [ -f "$home/skills/commit/SKILL.md" ]; }; then
    printf '/commit'
  else
    printf '%s' "$agent_request"
  fi
}

commit() {
  local root workspace pane agent state
  root="$(root_folder)" || return
  [ "${1:-}" = --manual ] && { commit_manually "$root"; return; }
  if ! has_changes "$root"; then
    tell 'Nothing to commit.'
    return 0
  fi

  if ! workspace="$(session)" || ! pane="$(agent_pane "$workspace")" || [ -z "$pane" ]; then
    commit_manually "$root"
    return
  fi
  # Working or waiting on you: a commit request now could commit half-done
  # work, so you commit by hand instead.
  state="$(workspace_option "$workspace" @devws_agent_state)"
  case "$state" in
    working | waiting)
      printf 'The agent is busy (%s); commit by hand.\n\n' "$state"
      commit_manually "$root"
      return
      ;;
  esac

  agent="$(workspace_option "$workspace" @devws_agent)"
  tmux send-keys -t "$pane" -l "$(agent_message "${agent:-${DEVWS_DEFAULT_AGENT:-claude}}" "$root")"
  tmux send-keys -t "$pane" Enter
  tell 'Asked the agent to commit.'
}

push() {
  local root branch answer draft="" url
  root="$(root_folder)" || return
  branch="$(git -C "$root" symbolic-ref --quiet --short HEAD)" || die "no branch to push: HEAD is detached"
  git -C "$root" push -u origin "$branch" || return 1

  if ! command -v gh >/dev/null 2>&1; then
    printf 'Pushed %s. Install gh to open its pull request from devws.\n' "$branch"
    return 0
  fi
  if url="$(cd "$root" && gh pr view --json url -q .url 2>/dev/null)" && [ -n "$url" ]; then
    printf 'Pull request: %s\n' "$url"
  else
    read -r -p 'Open a pull request? [Y/n, d = draft] ' answer || answer=n
    case "$answer" in
      n | N | no) return 0 ;;
      d | D | draft) draft=--draft ;;
    esac
    (cd "$root" && gh pr create --fill $draft) || return 1
    (cd "$root" && gh pr view --web) || true
  fi

  # The sidebar shows the new state now rather than at its next check.
  "$script_dir/github_repo.sh" pr-query "$root" >/dev/null 2>&1 || true
  if [ -n "${TMUX:-}" ] && [ -x "$script_dir/refresh_devws_pickers.sh" ]; then
    "$script_dir/refresh_devws_pickers.sh" --reload >/dev/null 2>&1 || true
  fi
}

case "${1:-}" in
  commit) commit "${2:-}" ;;
  push) push ;;
  *) die "usage: git_actions.sh <commit [--manual]|push>" ;;
esac
