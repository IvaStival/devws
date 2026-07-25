#!/usr/bin/env bash
# Marks the pane as the workspace agent pane, then replaces itself with the
# configured agent process. The pane marker makes restart/switch actions safe.

set -u

agent="${1:-}"
case "$agent" in
  claude|codex) ;;
  *)
    printf 'Unsupported devws agent: %s\n' "$agent" >&2
    exit 2
    ;;
esac

tmux set-option -p -t "$TMUX_PANE" @devws_role agent
exec "$agent"
