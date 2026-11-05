#!/usr/bin/env bash
# The guide (docs/guide/) keeps up with devws: every menu item, `devws`
# subcommand, prefix key and setting the code has is named in its page, and
# the menu's Help tab and the guide's pages match one to one. A feature added
# without its docs fails here.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUIDE="$ROOT/docs/guide"
MENU="$ROOT/config/tmuxinator/devws_menu.sh"
missing=()

# Variables that are not settings: test overrides (their comments say "for
# tests") and values devws passes between its own scripts (the header's
# colors come from tmux.conf's @devws_color_* options).
internal_variables=" DEVWS_TOKENS_NOW DEVWS_NETWORK_NOW DEVWS_CLOCK_TIME DEVWS_ENV_FILE
  DEVWS_TERMINAL_NOTIFIER DEVWS_PICKER_ROWS DEVWS_AGENT DEVWS_ROOT DEVWS_JOBS_DIR
  DEVWS_AGENT_ DEVWS_PAGER DEVWS_NOTIFY_COMMAND DEVWS_AI_MEMORY DEVWS_COLOR_OK
  DEVWS_COLOR_WARN DEVWS_COLOR_STRESS DEVWS_COLOR_DIM DEVWS_COLOR_RULE DEVWS_COLOR_GHOST
  DEVWS_NVIM_SOCKET_PREFIX DEVWS_NVIM_GRACE DEVWS_BIN"
internal_variables=" $(printf '%s' "$internal_variables" | tr '\n' ' ') "

shopt -s nullglob
pages=("$GUIDE"/*.md)
[ "${#pages[@]}" -gt 0 ] || { printf 'Guide test failed: no pages in %s\n' "$GUIDE" >&2; exit 1; }

# needs <what> <text> <file…>: the text appears in one of the files.
needs() {
  local what="$1" text="$2"
  shift 2
  grep -qF -- "$text" "$@" 2>/dev/null || missing+=("$what: $text")
}

# The actions table: tab|action|label|shortcut|function[|when]. A label's
# first word is its icon.
actions="$(sed -n "/^actions='/,/'\$/p" "$MENU" | sed "s/^actions='//; s/'\$//")"
while IFS='|' read -r tab action label shortcut function when; do
  [ -n "$tab" ] || continue
  if [ "$tab" = help ]; then
    page="${function#open_guide }"
    [ -f "$GUIDE/$page.md" ] || missing+=("Help item $action: no page docs/guide/$page.md")
    continue
  fi
  needs "menu item" "${label#* }" "${pages[@]}"
done <<<"$actions"

for page in "${pages[@]}"; do
  page="$(basename "$page" .md)"
  grep -q "|open_guide $page\$" <<<"$actions" || missing+=("page $page.md: no Help item in the menu")
done

# `devws <subcommand>`, as bin/devws dispatches them.
for subcommand in $(grep -oE '"\$\{1:-\}" == "[a-z]+"' "$ROOT/bin/devws" | sed 's/.*== "//; s/"//' | sort -u); do
  needs "devws subcommand" "devws $subcommand" "$GUIDE/commands.md"
done

# Prefix keys (no -T/-n table), written C-b <key> in the page. Only those
# bound after default-keys.conf is sourced: it wipes every binding before it.
# A | in a Markdown table is written \|.
while read -r key; do
  needs "key" "C-b ${key//|/\\|}" "$GUIDE/shortcuts.md"
done < <(
  sed -n '/^source-file .*default-keys\.conf/,$p' "$ROOT/config/tmux/tmux.conf" |
    grep -E '^\s*bind(-key)? ' |
    grep -vE ' -(T|n) ' |
    sed -E 's/^\s*bind(-key)? (-r )?//' | awk '{ print $1 }' | tr -d "'\"" | sort -u
)

for variable in $(grep -rhoE 'DEVWS_[A-Z0-9_]+' "$ROOT/config" "$ROOT/bin" "$ROOT/install.sh" "$ROOT/shell" | sort -u); do
  case "$internal_variables" in *" $variable "*) continue ;; esac
  needs "setting" "$variable" "$GUIDE/configuration.md"
done

if [ "${#missing[@]}" -gt 0 ]; then
  printf 'Guide test failed: the guide is missing\n' >&2
  printf '  %s\n' "${missing[@]}" >&2
  exit 1
fi
printf 'Guide test passed.\n'
