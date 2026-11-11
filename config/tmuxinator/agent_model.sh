#!/usr/bin/env bash
# Runtime model metadata, separate from launch aliases. Hooks record direct
# evidence; the periodic header update backfills older panes from cached logs.
#   agent_model.sh record <pane> <model>
#   agent_model.sh refresh [--force]
set -u
scripts_dir="$(cd "$(dirname "$0")" && pwd)"

record() {
  local pane="$1" model="$2" source="${3:-runtime}" observed="${4:-0}" transcript="${5:-}"
  local previous old_model old_source old_seen old_transcript current_transcript
  # These values become single-line sidebar fields.
  [ -n "$model" ] && [ "$model" != '<synthetic>' ] || return 0
  # A class, not a range of bytes: macOS's bash 3.2 does not match a newline
  # against [$'\001'-$'\037'].
  [[ "$model" != *[[:cntrl:]]* ]] || return 0
  previous="$(tmux display-message -p -t "$pane" '#{@devws_model}|#{@devws_model_source}|#{@devws_model_seen}|#{@devws_model_transcript}|#{@devws_transcript}' 2>/dev/null)" || return
  IFS='|' read -r old_model old_source old_seen old_transcript current_transcript <<< "$previous"
  case "$old_seen" in ''|*[!0-9]*) old_seen=0 ;; esac
  if [ "$source" = transcript ] && [ "$old_source" = runtime ] &&
      [ "$old_transcript" = "$transcript" ] && [ "$observed" -le "$old_seen" ]; then
    # A cached transcript may lag a direct hook/status-line observation.
    return 0
  fi
  if [ "$source" = runtime ]; then
    observed="$(date +%s)"
    transcript="$current_transcript"
  fi
  if [ "$old_model|$old_source|$old_transcript" = "$model|$source|$transcript" ]; then
    # Repeated runtime evidence can be newer than a conflicting transcript
    # record. Advance only its timestamp, without repainting unchanged text.
    if [ "$source" = runtime ] && [ "$old_seen" != "$observed" ]; then
      tmux set-option -p -t "$pane" @devws_model_seen "$observed"
    fi
    return 0
  fi
  if [ "$old_model" != "$model" ]; then
    tmux set-option -p -t "$pane" @devws_model "$model" || return
  fi
  tmux set-option -p -t "$pane" @devws_model_source "$source" || return
  tmux set-option -p -t "$pane" @devws_model_seen "$observed" || return
  tmux set-option -p -t "$pane" @devws_model_transcript "$transcript" || return
  [ "$old_model" = "$model" ] || "$scripts_dir/refresh_devws_pickers.sh" --reload
}

refresh() {
  local now checked pane role transcript info observed model failed=0
  now="$(date +%s)"
  checked="$(tmux show-option -gqv @devws_models_checked 2>/dev/null)"
  case "$checked" in ''|*[!0-9]*) checked=0 ;; esac
  [ "${1:-}" = --force ] || [ $((now - checked)) -ge 30 ] || return 0
  tmux set-option -g @devws_models_checked "$now" || return
  while IFS='|' read -r pane role transcript; do
    case "$role" in agent|extra-agent) ;; *) continue ;; esac
    [ -n "$transcript" ] || continue
    if ! info="$("$scripts_dir/header_tokens.sh" model-info "$transcript")"; then
      printf 'Cannot read model metadata for pane %s.\n' "$pane" >&2
      failed=1
      continue
    fi
    [ -n "$info" ] || continue
    IFS=$'\t' read -r observed model <<< "$info"
    case "$observed" in ''|*[!0-9]*) observed=0 ;; esac
    record "$pane" "$model" transcript "$observed" "$transcript" || failed=1
  done < <(tmux list-panes -a -F '#{pane_id}|#{@devws_role}|#{@devws_transcript}')
  return "$failed"
}

case "${1:-}" in
  record) [ "$#" -eq 3 ] || exit 2; record "$2" "$3" ;;
  refresh) refresh "${2:-}" ;;
  *) printf 'Usage: agent_model.sh record <pane> <model> | refresh [--force]\n' >&2; exit 2 ;;
esac
