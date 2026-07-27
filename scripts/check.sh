#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

bash -n "$ROOT/install.sh"
bash -n "$ROOT/delete.sh"
bash -n "$ROOT/scripts/test_install_lifecycle.sh"
bash -n "$ROOT/scripts/test_markdown_viewer.sh"
for file in "$ROOT"/config/tmux/*.sh "$ROOT"/config/tmuxinator/*.sh; do
  bash -n "$file"
done
zsh -n "$ROOT/shell/devws.zsh"

ruby -rerb -ryaml -e '
  settings = { "agent" => "codex", "project_root" => Dir.pwd }
  context = Object.new
  context.instance_variable_set(:@settings, settings)
  rendered = ERB.new(File.read(ARGV[0])).result(context.instance_eval { binding })
  YAML.safe_load(rendered, aliases: true)
' "$ROOT/config/tmuxinator/dev.yml"

if command -v luac >/dev/null 2>&1; then
  luac -p "$ROOT/config/lvim/config.lua"
fi

for file in "$ROOT/install.sh" "$ROOT/delete.sh" "$ROOT/scripts/check.sh" \
            "$ROOT/scripts/test_install_lifecycle.sh" \
            "$ROOT/scripts/test_markdown_viewer.sh" \
            "$ROOT"/config/tmux/*.sh "$ROOT"/config/tmuxinator/*.sh; do
  [[ -x "$file" ]] || {
    printf 'Not executable: %s\n' "$file" >&2
    exit 1
  }
done

"$ROOT/scripts/test_install_lifecycle.sh"
"$ROOT/scripts/test_markdown_viewer.sh"

printf 'All checks passed.\n'
