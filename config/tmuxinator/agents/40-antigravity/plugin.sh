#!/usr/bin/env bash
# devws's plugin for Antigravity (agy). agy takes neither hooks nor MCP
# servers on its command line, only from plugins in its config folder, so
# devws keeps one there, ~/.gemini/config/plugins/devws: its hooks report to
# devws (hook.sh) and its MCP server carries devAgentics' leader
# tools (mcp.sh). Both do nothing in an agy session devws did not
# start. agent_runner.sh installs it before each agy launch.
#
#   plugin.sh install          writes the plugin, when it differs
#   plugin.sh remove           deletes it
#   plugin.sh trust <folder>   trusts a folder, as answering agy's
#                                          "Do you trust …?" would
#
# DEVWS_AGY_CONFIG replaces ~/.gemini/config and DEVWS_AGY_HOME
# ~/.gemini/antigravity-cli, for tests.

set -u

module_dir="$(cd "$(dirname "$0")" && pwd)"
config="${DEVWS_AGY_CONFIG:-$HOME/.gemini/config}"
home="${DEVWS_AGY_HOME:-$HOME/.gemini/antigravity-cli}"
plugin="$config/plugins/devws"

# <file> <contents>: written only when it changed, so a running agy is not
# disturbed by a launch next to it.
write() {
  [ "$(cat "$1" 2>/dev/null)" = "$2" ] && return 0
  printf '%s\n' "$2" > "$1.tmp" && mv "$1.tmp" "$1"
}

install() {
  local hook hooks
  mkdir -p "$plugin" || return 1
  hook="$module_dir/hook.sh"
  hooks="$(jq -n --arg hook "$hook" '
    def run($event): [{type: "command", command: "\"\($hook)\" \($event)", timeout: 30}];
    {devws: {
      SessionStart: run("SessionStart"),
      PreInvocation: run("PreInvocation"),
      PreToolUse: [{matcher: "", hooks: run("PreToolUse")}],
      PostToolUse: [{matcher: "", hooks: run("PostToolUse")}],
      Stop: run("Stop")}}')"
  write "$plugin/plugin.json" '{"name": "devws"}' &&
    write "$plugin/hooks.json" "$hooks" &&
    write "$plugin/mcp_config.json" "$(jq -n --arg command "$module_dir/mcp.sh" \
      '{mcpServers: {devagentics: {command: $command, args: []}}}')"
}

trust() {
  local folder="$1" settings="$home/settings.json" current
  folder="$(cd "$folder" 2>/dev/null && pwd -P)" || return 1
  current="$(cat "$settings" 2>/dev/null)"
  [ -n "$current" ] || current='{}'
  printf '%s' "$current" | jq -e --arg folder "$folder" '(.trustedWorkspaces // []) | index($folder)' >/dev/null 2>&1 && return 0
  mkdir -p "$home" &&
    printf '%s' "$current" | jq --arg folder "$folder" '.trustedWorkspaces = ((.trustedWorkspaces // []) + [$folder])' > "$settings.tmp" &&
    mv "$settings.tmp" "$settings"
}

case "${1:-}" in
  install) install ;;
  remove) rm -rf "$plugin" ;;
  trust) trust "${2:-}" ;;
  *)
    printf 'Usage: plugin.sh install | remove | trust <folder>\n' >&2
    exit 2
    ;;
esac
