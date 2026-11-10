#!/usr/bin/env bash
# Every question devws asks from a list, in one look: the prompt, a header
# line saying what is asked, and a footer with the keys — Esc goes back a step
# (the caller's loop does that: Esc exits 130), Enter chooses, Tab unticks in
# a multi-select. The list comes on stdin; the choice is printed.
#
#   picker.sh <prompt> <header> [fzf options...]
#
# The caller's options come last, so they win over the shared ones (a
# --height, a --footer of its own).
#
# devws-ui asks when rust_tools.sh installed it (~/.cargo/bin; DEVWS_UI names
# another): it keeps the terminal cursor hidden, so a popup does not flicker
# each time tmux redraws it. The callers' fzf options are translated; one it
# does not have leaves that question to fzf, as without devws-ui.

set -u

prompt="${1:?Usage: picker.sh <prompt> <header> [fzf options...]}"
header="${2:-}"
shift 2

footer='Esc back · Enter choose'
for option in "$@"; do
  [ "$option" = --multi ] && footer='Tab untick · Esc back · Enter choose'
done
footer="$(printf '\033[2m%s\033[0m' "$footer")"

# devws-ui's options for the callers' fzf ones, or failure for one it lacks.
# Layout-only options (height, borders…) mean nothing to it and are dropped.
ui_options() {
  local option
  while [ "$#" -gt 0 ]; do
    option="$1"
    shift
    case "$option" in
      --multi | --print-query) printf '%s\n' "$option" ;;
      --delimiter | --with-nth | --footer)
        [ "$#" -gt 0 ] || return 1
        printf '%s=%s\n' "$option" "$1"
        shift
        ;;
      --delimiter=* | --with-nth=* | --footer=*) printf '%s\n' "$option" ;;
      --bind)
        [ "${1:-}" = load:select-all ] || return 1
        printf -- '--select-all\n'
        shift
        ;;
      --bind=load:select-all) printf -- '--select-all\n' ;;
      --height | --info | --margin | --padding | --gutter | --layout) shift ;;
      --height=* | --info=* | --border=* | --margin=* | --padding=* | --gutter=* | --layout=* | \
        --border | --no-sort | --ansi | --no-separator) ;;
      *) return 1 ;;
    esac
  done
}

ui="${DEVWS_UI:-$HOME/.cargo/bin/devws-ui}"
if [ -x "$ui" ] && translated="$(ui_options "$@")"; then
  options=("--prompt=$prompt › " "--header=$header" "--footer=$footer")
  while IFS= read -r option; do
    # A caller's own footer replaces the shared one.
    case "$option" in --footer=*) options[2]="$option" ;; '') ;; *) options+=("$option") ;; esac
  done <<<"$translated"
  exec "$ui" pick "${options[@]}"
fi

exec fzf --ansi --layout=reverse --height=~60% \
  "--prompt=$prompt › " "--header=$header" \
  "--footer=$footer" \
  --info=inline-right --no-separator \
  "$@"
