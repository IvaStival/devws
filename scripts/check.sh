#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

bash -n "$ROOT/install.sh"
bash -n "$ROOT/delete.sh"
bash -n "$ROOT/scripts/test_install_lifecycle.sh"
bash -n "$ROOT/scripts/test_markdown_viewer.sh"
bash -n "$ROOT/scripts/test_workspace_state.sh"
bash -n "$ROOT/scripts/test_workspace_layout.sh"
bash -n "$ROOT/scripts/test_workspace_minimize.sh"
bash -n "$ROOT/scripts/test_workspace_database.sh"
bash -n "$ROOT/scripts/test_agent_follow.sh"
bash -n "$ROOT/scripts/test_agent_status.sh"
"$ROOT/scripts/test_jump_agent.sh"
bash -n "$ROOT/scripts/test_header.sh"
bash -n "$ROOT/scripts/test_devws_reload.sh"
bash -n "$ROOT/scripts/test_new_panel.sh"
bash -n "$ROOT/scripts/test_devws_menu.sh"
bash -n "$ROOT/scripts/test_register_agent.sh"
bash -n "$ROOT/scripts/test_iterm_keys.sh"
bash -n "$ROOT/scripts/setup_iterm_keys.sh"
bash -n "$ROOT/shell/render_mermaid_preview.sh"
for file in "$ROOT"/config/tmux/*.sh "$ROOT"/config/tmuxinator/*.sh; do
  bash -n "$file"
done
zsh -n "$ROOT/shell/devws.zsh"

ruby -rerb -ryaml -e '
  settings = { "agent" => "codex", "project_root" => Dir.pwd }
  context = Object.new
  context.instance_variable_set(:@settings, settings)
  rendered = ERB.new(File.read(ARGV[0])).result(context.instance_eval { binding })
  config = YAML.safe_load(rendered, aliases: true)
  # A stray " #" in an unquoted value silently truncates it as a comment.
  abort "dev.yml: pre_window was cut short" unless config["pre_window"].end_with?("; clear")
' "$ROOT/config/tmuxinator/dev.yml"

if command -v luac >/dev/null 2>&1; then
  while IFS= read -r -d '' lua_file; do
    luac -p "$lua_file"
  done < <(find "$ROOT/config/nvim" -name '*.lua' -print0)
fi

for file in "$ROOT/install.sh" "$ROOT/delete.sh" "$ROOT/scripts/check.sh" \
            "$ROOT/scripts/test_install_lifecycle.sh" \
            "$ROOT/scripts/test_markdown_viewer.sh" \
            "$ROOT/scripts/test_workspace_state.sh" \
            "$ROOT/scripts/test_workspace_layout.sh" \
            "$ROOT/scripts/test_workspace_minimize.sh" \
            "$ROOT/scripts/test_workspace_database.sh" \
            "$ROOT/scripts/test_agent_follow.sh" \
            "$ROOT/scripts/test_agent_status.sh" \
            "$ROOT/scripts/test_jump_agent.sh" \
            "$ROOT/scripts/test_header.sh" \
            "$ROOT/scripts/test_devws_reload.sh" \
            "$ROOT/scripts/test_new_panel.sh" \
            "$ROOT/scripts/test_devws_menu.sh" \
            "$ROOT/scripts/test_register_agent.sh" \
            "$ROOT/scripts/test_iterm_keys.sh" \
            "$ROOT/scripts/setup_iterm_keys.sh" \
            "$ROOT/shell/render_mermaid_preview.sh" \
            "$ROOT"/config/tmux/*.sh "$ROOT"/config/tmuxinator/*.sh; do
  [[ -x "$file" ]] || {
    printf 'Not executable: %s\n' "$file" >&2
    exit 1
  }
done

"$ROOT/scripts/test_install_lifecycle.sh"
"$ROOT/scripts/test_markdown_viewer.sh"
"$ROOT/scripts/test_workspace_state.sh"
"$ROOT/scripts/test_workspace_layout.sh"
"$ROOT/scripts/test_workspace_minimize.sh"
"$ROOT/scripts/test_workspace_database.sh"
"$ROOT/scripts/test_agent_follow.sh"
"$ROOT/scripts/test_agent_status.sh"
"$ROOT/scripts/test_header.sh"
"$ROOT/scripts/test_devws_reload.sh"
"$ROOT/scripts/test_new_panel.sh"
"$ROOT/scripts/test_devws_menu.sh"
"$ROOT/scripts/test_register_agent.sh"
"$ROOT/scripts/test_iterm_keys.sh"

printf 'All checks passed.\n'
