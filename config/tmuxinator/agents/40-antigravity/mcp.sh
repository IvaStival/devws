#!/usr/bin/env bash
# The devws plugin's MCP server for Antigravity (plugin.sh). agy
# reads MCP servers only from its config folder, for every session, so the
# server is always there; devAgentics' leader tools are what it serves only
# when a devws pane launched agy with them. agent_runner.sh exports
# DEVWS_AGY_MCP_CONFIG, the file devAgentics wrote for the leader
# ({"mcpServers": {"devagentics": {"command", "args"}}}); this runs that
# server. Without it, it is a server with no tools, which agy shows nowhere.
#
#   mcp.sh   (MCP over stdio)

set -u

config="${DEVWS_AGY_MCP_CONFIG:-}"
if [ -n "$config" ] && [ -r "$config" ]; then
  command="$(jq -r '.mcpServers.devagentics.command // empty' "$config" 2>/dev/null)"
  if [ -n "$command" ]; then
    arguments=()
    while IFS= read -r -d '' argument; do
      arguments+=("$argument")
    done < <(jq -j '.mcpServers.devagentics.args // [] | .[] | tostring | ., "\u0000"' "$config")
    exec "$command" ${arguments[@]+"${arguments[@]}"}
  fi
fi

# No tools: answer the handshake and the listings; notifications get no answer.
while IFS= read -r line; do
  printf '%s' "$line" | jq -c 'select(.id != null) | {jsonrpc: "2.0", id,
    result: (if .method == "initialize" then
        {protocolVersion: (.params.protocolVersion // "2025-03-26"), capabilities: {tools: {}},
         serverInfo: {name: "devws", version: "1"}}
      elif .method == "tools/list" then {tools: []}
      else {} end)}' 2>/dev/null
done
