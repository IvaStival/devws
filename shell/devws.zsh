# devws shell integration

devws() {
  if [[ "${1:-}" == "md" ]]; then
    shift
    devws-markdown "$@"
    return
  fi

  if [[ "${1:-}" == "menu" ]]; then
    devws-menu "${2:-toggle}" "${3:-}"
    return
  fi

  local project_root="${1:-$PWD}"
  local agent="${2:-}"
  local editor="${3:-}"
  local agent_config="$HOME/.tmuxinator/.env"

  if [[ -r "$agent_config" ]]; then
    source "$agent_config"
  fi

  local -a available_agents
  available_agents=(${=DEVWS_AGENTS:-claude codex})

  if [[ -z "$agent" && -t 0 && -t 1 && -x "$(command -v fzf)" ]]; then
    agent="$(printf '%s\n' "${available_agents[@]}" | fzf \
      --height=~10 --layout=reverse --prompt='Agent > ')"
    [[ -n "$agent" ]] || return 0
  fi
  agent="${agent:-${DEVWS_DEFAULT_AGENT:-${available_agents[1]}}}"

  if (( ${available_agents[(Ie)$agent]} == 0 )); then
    print -u2 "devws: unknown agent '$agent' (choose: ${available_agents[*]})"
    return 2
  fi

  local -a available_editors
  available_editors=(${=DEVWS_EDITORS:-lvim nvim vim code zed})

  if [[ -z "$editor" && -t 0 && -t 1 && -x "$(command -v fzf)" ]]; then
    editor="$(printf '%s\n' "${available_editors[@]}" | fzf \
      --height=~10 --layout=reverse --prompt='Editor > ')"
    [[ -n "$editor" ]] || return 0
  fi
  editor="${editor:-${DEVWS_DEFAULT_EDITOR:-${available_editors[1]}}}"

  if (( ${available_editors[(Ie)$editor]} == 0 )); then
    print -u2 "devws: unknown editor '$editor' (choose: ${available_editors[*]})"
    return 2
  fi

  tmuxinator start dev project_root="$project_root" agent="$agent" editor="$editor"
  [[ -n "$TMUX" ]] && "$HOME/.tmuxinator/refresh_devws_pickers.sh"
}

devws-markdown() {
  local markdown_file
  local self_dir="${${(%):-%x}:A:h}"

  if (( $# > 1 )); then
    print -u2 "usage: devws md [file.md]"
    return 2
  fi
  if ! command -v glow >/dev/null 2>&1; then
    print -u2 "devws md: Glow is required; run ./install.sh --deps"
    return 127
  fi

  if (( $# == 1 )); then
    markdown_file="$1"
  else
    if ! command -v fzf >/dev/null 2>&1; then
      print -u2 "devws md: fzf is required for interactive selection"
      return 127
    fi
    markdown_file="$(
      find . -type f \
        \( -iname '*.md' -o -iname '*.markdown' \) \
        ! -path '*/.git/*' -print |
        LC_ALL=C sort |
        fzf --height=~60% --layout=reverse \
          --prompt='Markdown > ' \
          --header='Choose a Markdown file to open with Glow'
    )"
    [[ -n "$markdown_file" ]] || return 0
  fi

  if [[ ! -f "$markdown_file" ]]; then
    print -u2 "devws md: file not found: $markdown_file"
    return 2
  fi
  if [[ ! -r "$markdown_file" ]]; then
    print -u2 "devws md: file is not readable: $markdown_file"
    return 2
  fi
  case "${markdown_file:l}" in
    *.md|*.markdown) ;;
    *)
      print -u2 "devws md: expected a .md or .markdown file"
      return 2
      ;;
  esac

  local mermaid_script="$self_dir/render_mermaid_preview.sh"
  [[ -x "$mermaid_script" ]] && "$mermaid_script" "$markdown_file"

  command glow -p "$markdown_file"
}

devws-menu() {
  local action="${1:-toggle}"
  local session="${2:-}"
  local window picker

  if [[ -z "$TMUX" ]]; then
    print -u2 "devws menu: run this command inside tmux"
    return 1
  fi

  session="${session:-$(tmux display-message -p '#S')}"
  [[ "$session" == dev-* ]] || session="dev-$session"
  window="$session:workspace"

  if ! tmux list-windows -t "$session" -F '#{window_name}' 2>/dev/null |
      grep -qx 'workspace'; then
    print -u2 "devws menu: workspace window not found in '$session'"
    return 1
  fi

  picker="$(
    tmux list-panes -t "$window" -F '#{pane_id}|#{pane_title}' 2>/dev/null |
      awk -F '|' '$2 == "devws-picker" { print $1; exit }'
  )"

  case "$action" in
    open)
      [[ -n "$picker" ]] && return 0
      tmux split-window -d -f -h -b -l 25% -t "$window" \
        "$HOME/.tmuxinator/session_picker.sh"
      ;;
    close)
      [[ -z "$picker" ]] || tmux kill-pane -t "$picker"
      ;;
    toggle)
      if [[ -n "$picker" ]]; then
        tmux kill-pane -t "$picker"
      else
        tmux split-window -d -f -h -b -l 25% -t "$window" \
          "$HOME/.tmuxinator/session_picker.sh"
      fi
      ;;
    *)
      print -u2 "usage: devws menu {open|close|toggle} [workspace]"
      return 2
      ;;
  esac
}

# Use a compact prompt inside devws panes only.
if [[ -n "$TMUX" ]] && [[ "$(tmux display-message -p '#S' 2>/dev/null)" == dev-* ]]; then
  autoload -Uz vcs_info
  precmd_functions+=(vcs_info)
  zstyle ':vcs_info:git:*' formats ' %F{yellow}‹%b›%f'
  setopt PROMPT_SUBST
  PROMPT='╭─%B%F{blue}%1~%f%b${vcs_info_msg_0_}
╰─➤ '
fi
