# devws shell integration: the editor and the prompt. The `devws` command
# itself is bin/devws, on PATH (~/.local/bin/devws, linked by ./install.sh),
# so every pane runs the checkout's current version.

# A shell that sourced an older version of this file defined `devws` as a
# function, which would shadow the command.
unfunction devws devws-restore devws-root devws-run devws-push devws-jobs \
  devws-markdown devws-menu 2>/dev/null

# Neovim (with the devws LazyVim config) is the default text editor for tools
# that open one, such as git or crontab. A value set before this file is
# sourced wins.
if command -v nvim >/dev/null 2>&1; then
  export EDITOR="${EDITOR:-nvim}"
  export VISUAL="${VISUAL:-nvim}"
fi

# Use a compact prompt inside devws panes only.
if [[ -n "$TMUX" ]] && [[ "$(tmux display-message -p '#S' 2>/dev/null)" == dev-* ]]; then
  autoload -Uz vcs_info
  precmd_functions+=(vcs_info)
  zstyle ':vcs_info:git:*' formats ' %F{yellow}‹%b›%f'
  setopt PROMPT_SUBST
  PROMPT='╭─%B%F{blue}%1~%f%b${vcs_info_msg_0_}
╰─➤ '
fi
