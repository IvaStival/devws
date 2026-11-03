# Spec: ai-memory for devws agents

- **Date:** 02-11-2026
- **Branch:** feat/ai-memory (off feat/stabilize)
- **Status:** Executed

## Context

[ai-memory](https://github.com/akitaonrails/ai-memory) gives coding agents
long-term memory: lifecycle hooks record each session, an MCP server lets an
agent search what was done before (full-text, entities, graph), and a
per-user service (`http://127.0.0.1:49374`, web UI at `/web`) keeps one store.
devws runs several agents and accounts (claude, codex, claude_work) across
many branch folders; shared memory lets any of them recall the others' work.
`ai-memory setup` wires only the default folders (`~/.claude`, `~/.codex`);
its `install-mcp` / `install-hooks` honor `CLAUDE_CONFIG_DIR` / `CODEX_HOME`,
so devws can wire its other accounts the same way. devws keeps starting the
agents itself: user-level hooks and MCP apply alongside its `--settings`.

Decisions come from a grilling session on 02-11-2026.

## Requirements

1. **Install:** `./install.sh` asks `Install ai-memory (long-term memory for
   agents)? [y/N]` when it is missing. Yes runs the official installer
   (`curl -fsSL …/releases/latest/download/install.sh | sh`, which checks its
   own checksum), then `ai-memory setup` (which shows its plan and asks),
   then wires every registered devws agent (3). Declining changes nothing.
2. **`memory.sh`** (new), the one place devws talks to ai-memory:
   `available`, `up` (the `/healthz` check), `wire <agent>` (install-mcp and
   install-hooks for its kind, in its folder), `wire-all`, `search <query>`,
   `recent`, `web`, `doctor`.
3. **Every registered agent is wired:** `memory.sh wire-all` runs for each
   name in `DEVWS_AGENTS` with `CLAUDE_CONFIG_DIR` / `CODEX_HOME` set as
   agent_runner.sh sets them (none for a default folder). Registering a new
   agent in the menu wires it too when ai-memory is installed.
4. **Agent tab:** `⁘ Search memory` (asks a question, pages the answer),
   `◷ Recent memory`, `↗ Open memory web UI`, `✚ Memory doctor` — shown only
   when ai-memory is installed. `devws memory [search <q>|recent|web|doctor|wire]`.
5. **Header:** the agent column's last row ends with `mem ●`: green when the
   service answers, red when installed but down; nothing when not installed.
   Checked with the agents' limits (every minute), never blocking.

## Constraints

- devws's agent launch (agent_runner.sh) is unchanged.
- Nothing is installed or wired without a yes; `--apply` steps keep
  ai-memory's own timestamped backups.
- Tests stub `ai-memory` and `curl`; no network, no real service.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/memory.sh` | Create — ai-memory commands for devws |
| `install.sh` | Modify — offer the install, then wire-all |
| `config/tmuxinator/register_agent.sh` | Modify — wire a newly registered agent |
| `config/tmuxinator/devws_menu.sh`, `workspace_actions.sh` | Modify — Agent-tab memory items |
| `config/tmuxinator/header_agents.sh` | Modify — `mem ●` |
| `shell/devws.zsh` | Modify — `devws memory` |
| `scripts/test_memory.sh` | Create — tests |
| `scripts/test_install_lifecycle.sh`, `test_devws_menu.sh`, `test_header.sh`, `test_register_agent.sh`, `check.sh`, `README.md` | Modify |

## Success Criteria

- [ ] `wire-all` runs install-mcp/install-hooks once per agent with the right folder variable (none for defaults).
- [ ] The installer offers ai-memory only when missing, and does nothing on no.
- [ ] Memory items appear in the Agent tab only with ai-memory installed.
- [ ] The header shows `mem ●` green/red from `/healthz`, nothing without ai-memory.
- [ ] Registering an agent wires it.
- [ ] `scripts/check.sh` passes.
