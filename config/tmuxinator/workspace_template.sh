#!/usr/bin/env bash
# Workspace templates of your own (devws menu › Templates): a workspace's
# panes, what runs in them and their layout, kept as a tmuxinator file in
# ~/.tmuxinator/templates, to open again on any folder.
#
#   workspace_template.sh list             the saved templates, a name a line
#   workspace_template.sh builtins         devws's own, "<name>\t<what it is>"
#   workspace_template.sh file <name>      its file
#   workspace_template.sh save <session> <name>
#                                          keeps the workspace as <name>
#   workspace_template.sh start <name> <folder> <session> [tmuxinator options…]
#                                          opens <name> on <folder>, named
#                                          <session>; "blank" is the empty
#                                          workspace (blank.yml)
#   workspace_template.sh delete <name>
#   workspace_template.sh is-template <name>
#                                          blank or a saved template: what
#                                          restore_state.sh restarts from it
#
# A pane is saved as what runs in it: devws's own panes (the sidebar, the
# editor, an agent) as the scripts that start them, a shell as a shell, and
# anything else as the command in its foreground, in the folder it is in
# under the workspace's.

set -u

templates_dir="$HOME/.tmuxinator/templates"
# The separator of a pane's fields: not whitespace, so an empty field (a pane
# with no role) keeps its place.
separator=$'\x1f'
# shellcheck source=workspace_panes.sh
. "$(dirname "$0")/workspace_panes.sh"

# devws's own templates, opened by their own menu actions: the names a saved
# template cannot take.
builtin_templates="dev	Folder workspace: the sidebar, an editor, an agent and a terminal
system	System monitor: btop, lazydocker and a log
agentic	Agent team: a task's leader and its agents
blank	Empty workspace: the sidebar and a shell, to arrange and save"

fail() {
  printf 'devws template: %s\n' "$1" >&2
  exit 1
}

# A name is a file name and a tmux option's value: letters, digits, - and _.
# The built-in templates' names are taken.
valid_name() {
  case "$1" in
    '' | *[!A-Za-z0-9_-]*) fail "a template's name is letters, digits, - and _: '$1'" ;;
  esac
  ! printf '%s\n' "$builtin_templates" | cut -f1 | grep -qxF "$1" ||
    fail "'$1' is one of devws's own templates"
}

template_file() {
  printf '%s/%s.yml\n' "$templates_dir" "$1"
}

list_templates() {
  local file
  for file in "$templates_dir"/*.yml; do
    [ -f "$file" ] || continue
    file="${file##*/}"
    printf '%s\n' "${file%.yml}"
  done
}

is_template() {
  [ "$1" = blank ] && return 0
  case "$1" in '' | *[!A-Za-z0-9_-]*) return 1 ;; esac
  [ -f "$(template_file "$1")" ]
}

# The command in the pane's foreground, from its terminal: what was typed in
# its shell, or what the pane was started with.
foreground_command() {
  ps -t "${1#/dev/}" -o pid=,pgid=,tpgid=,args= 2>/dev/null |
    awk '$1 == $2 && $2 == $3 { $1 = $2 = $3 = ""; sub(/^ +/, ""); print; exit }'
}

# A shell waiting for a command: "bash", "-zsh", "bash --noprofile"…
is_shell() {
  local -a words
  local word
  read -r -a words <<<"$1"
  [ "${#words[@]}" -gt 0 ] || return 0
  word="${words[0]#-}"
  case "${word##*/}" in
    bash | zsh | fish | sh | dash | ksh | tcsh) ;;
    *) return 1 ;;
  esac
  for word in "${words[@]:1}"; do
    case "$word" in -*) ;; *) return 1 ;; esac
  done
}

# ~ for the home folder, so a template works for whoever opens it.
home_relative() {
  local text="$1"
  case "$text" in "$HOME/"*) text="~/${text#"$HOME/"}" ;; esac
  printf '%s' "${text//" $HOME/"/" ~/"}"
}

# The command a pane is saved as, empty for a shell.
#   pane_command <role> <title> <tty> <agent> <session>
pane_command() {
  local role="${1#minimized-}" title="$2" tty="$3" agent="$4" session="$5" command
  if [ "$title" = devws-picker ] || [ "$role" = picker ]; then
    printf '%s' '~/.tmuxinator/session_picker.sh'
    return
  fi
  case "$role" in
    editor)
      printf '~/.tmuxinator/editor_runner.sh %s' "$(tmux show-options -qv -t "=$session:" @devws_editor | grep -Ex 'nvim|vim|code|zed' || echo nvim)"
      return
      ;;
    agent | extra-agent)
      agent="${agent:-$(tmux show-options -qv -t "=$session:" @devws_agent)}"
      printf '~/.tmuxinator/agent_runner.sh %s%s' "${agent:-claude}" "$([ "$role" = extra-agent ] && echo ' --extra')"
      return
      ;;
    extra-editor) printf 'nvim .'; return ;;
    # A job's log is long gone by the time the template is opened.
    terminal | job-log) return ;;
  esac
  command="$(foreground_command "$tty")"
  is_shell "$command" || home_relative "$command"
}

# A YAML scalar tmuxinator reads as it is: single-quoted, and "<%" kept from
# ERB, which reads the file first.
yaml_quote() {
  local text="${1//\'/\'\'}"
  printf "'%s'" "${text//<%/<%%}"
}

# One pane's line: its command, after a cd when it was in another folder than
# the workspace's.
pane_line() {
  local command="$1" path="$2" root="$3" folder=""
  if [ "$path" != "$root" ] && [ -n "$path" ]; then
    case "$path" in
      "$root/"*) folder="$(printf '%q' "${path#"$root/"}")" ;;
      *) folder="$(home_relative "$(printf '%q' "$path")")" ;;
    esac
  fi
  if [ "$command" = '~/.tmuxinator/session_picker.sh' ]; then
    printf '        - picker: %s\n' "$command"
  elif [ -n "$folder" ]; then
    printf '        - %s\n' "$(yaml_quote "cd $folder${command:+ && $command}")"
  elif [ -n "$command" ]; then
    printf '        - %s\n' "$(yaml_quote "$command")"
  else
    printf '        - # a shell\n'
  fi
}

save_template() {
  local session="$1" name="$2" window root layout file pane role title tty path agent
  valid_name "$name"
  tmux has-session -t "=$session" 2>/dev/null || fail "no workspace $session"
  window="$(main_window "$session")"
  root="$(tmux show-options -qv -t "=$session:" @devws_root)"
  root="${root:-$(tmux display-message -p -t "$window" '#{pane_current_path}')}"
  layout="$(tmux display-message -p -t "$window" '#{window_layout}')"
  file="$(template_file "$name")"
  mkdir -p "$templates_dir" || fail "cannot create $templates_dir"

  {
    printf '# devws template "%s" (devws menu › Templates), saved from %s.\n' "$name" "$session"
    printf '# Templates › Open a template asks the folder and the name: every pane\n'
    printf '# below runs its command in a shell there. Change the commands freely; a\n'
    printf '# pane more or less needs the layout to match (save the workspace again).\n'
    printf 'name: <%%= @settings["name"] %%>\n'
    printf 'root: <%%= @settings["root"] %%>\n\n'
    printf "pre_window: 'tmux set-option status-position top; tmux set-option status-left-length 40; tmux set-option status-left \"[#{s/^dev-//:session_name}]\"; tmux set-option @devws_agent template; tmux set-option @devws_editor %s; tmux set-option @devws_template %s; tmux set-option @devws_root \"\$PWD\"; tmux set-option -w pane-border-indicators off; tmux set-option -w pane-border-status top; tmux set-option -w @devws_main 1; clear'\n\n" "$name" "$name"
    printf 'windows:\n  - workspace:\n'
    printf '      layout: %s\n      panes:\n' "$(yaml_quote "$layout")"
    tmux list-panes -t "$window" \
      -F "#{pane_id}${separator}#{@devws_role}${separator}#{pane_title}${separator}#{pane_tty}${separator}#{pane_current_path}${separator}#{@devws_agent}" |
      while IFS="$separator" read -r pane role title tty path agent; do
        pane_line "$(pane_command "$role" "$title" "$tty" "$agent" "$session")" "$path" "$root"
      done
  } > "$file.next" || fail "cannot write $file"
  mv "$file.next" "$file"

  # A workspace opened from a template restores from the one it is saved as
  # now; a dev workspace stays one.
  if is_template "$(tmux show-options -qv -t "=$session:" @devws_template)"; then
    tmux set-option -t "=$session:" @devws_template "$name"
    tmux set-option -t "=$session:" @devws_editor "$name"
  fi
}

start_template() {
  local name="$1" folder="$2" session="$3"
  shift 3
  [ -d "$folder" ] || fail "no folder $folder"
  if [ "$name" = blank ]; then
    tmuxinator start "$@" blank root="$folder" name="$session"
    return
  fi
  is_template "$name" || fail "no template $name"
  tmuxinator start "$@" -p "$(template_file "$name")" root="$folder" name="$session"
}

delete_template() {
  valid_name "$1"
  is_template "$1" || fail "no template $1"
  rm -f "$(template_file "$1")"
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  list) list_templates ;;
  builtins) printf '%s\n' "$builtin_templates" ;;
  file) [ -n "${1:-}" ] || fail "usage: workspace_template.sh file <name>"; template_file "$1" ;;
  save) [ "$#" -ge 2 ] || fail "usage: workspace_template.sh save <session> <name>"; save_template "$1" "$2" ;;
  start) [ "$#" -ge 3 ] || fail "usage: workspace_template.sh start <name> <folder> <session> [tmuxinator options…]"; start_template "$@" ;;
  delete) [ -n "${1:-}" ] || fail "usage: workspace_template.sh delete <name>"; delete_template "$1" ;;
  is-template) is_template "${1:-}" ;;
  *) fail "usage: workspace_template.sh <list|builtins|file|save|start|delete|is-template>" ;;
esac
