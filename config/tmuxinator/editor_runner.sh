#!/usr/bin/env bash
# Marks the pane as the workspace editor pane, then runs the selected editor.

set -u

config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"

editor="${1:-${DEVWS_DEFAULT_EDITOR:-lvim}}"
allowed=0
for candidate in ${DEVWS_EDITORS:-lvim nvim vim code zed}; do
  if [ "$candidate" = "$editor" ]; then
    allowed=1
    break
  fi
done

if [ "$allowed" -ne 1 ]; then
  printf 'Unsupported devws editor: %s\n' "$editor" >&2
  exit 2
fi
if ! command -v "$editor" >/dev/null 2>&1; then
  printf 'Editor is not installed or not on PATH: %s\n' "$editor" >&2
  exit 127
fi

tmux set-option -p -t "$TMUX_PANE" @devws_role editor
case "$editor" in
  code|zed) exec "$editor" --wait . ;;
  *) exec "$editor" . ;;
esac
