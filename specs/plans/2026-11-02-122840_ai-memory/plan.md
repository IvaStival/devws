# Plan: ai-memory for devws agents

- **Date:** 02-11-2026
- **Branch:** feat/ai-memory
- **Status:** Pending

## Context

Implements `spec.md` in this folder. `memory.sh` first (everything else
calls it), then the installer and registration, then the menu, header and CLI.

## Step 1 — `memory.sh`

**Files:** `config/tmuxinator/memory.sh`, `scripts/test_memory.sh`

**Actions:** `available` (command present), `up` (`curl -fsS --max-time 1
$AI_MEMORY_SERVER_URL/healthz`, default `http://127.0.0.1:49374`), `wire
<agent>` (agents.sh: kind → `claude-code` / `codex`; folder → env as
agent_runner.sh does; `ai-memory install-mcp --client <c> --apply` and
`install-hooks --agent <c> --apply`), `wire-all`, `search <q>` (ai-memory's
query command, paged), `recent`, `web` (`open <url>/web`), `doctor`.
Confirm the exact query subcommand from `ai-memory --help` at
implementation time.

**Verification:** `scripts/test_memory.sh` with a stub `ai-memory` logging
its calls and environment: wire for a default and a relocated account;
wire-all for every registered agent; `up` true/false from a stub curl.

## Step 2 — installer and registration

**Files:** `install.sh`, `config/tmuxinator/register_agent.sh`, their tests

**Actions:** `ensure_ai_memory` after `ensure_github_login`: missing → ask
(default no) → official install script → `ai-memory setup` → `memory.sh
wire-all`; present → `wire-all` quietly (new accounts since). Register:
after `add`, `memory.sh wire <name>` when available.

**Verification:** lifecycle test: no `ai-memory` + "n" → nothing run; "y" →
installer, setup and wire-all called (stubbed `curl`/`sh`/`ai-memory`).
Register test: wire called for the new agent.

## Step 3 — menu, header, CLI

**Files:** `devws_menu.sh`, `workspace_actions.sh`, `header_agents.sh`,
`shell/devws.zsh`, tests

**Actions:** Agent-tab items with a "shown when" rule (an optional fifth
condition column, `memory.sh available`); `header_agents.sh` appends
` mem ●` (ok/stress color) to its last row; `devws memory …`.

**Verification:** menu test: items present with a stub `ai-memory`, absent
without; header test: `mem ●` green with a stub healthy curl, red without.

## Step 4 — docs and check

**Files:** `README.md`, `scripts/check.sh`

**Verification:** `scripts/check.sh` passes.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: `./install.sh` → y → setup → `ai-memory doctor` shows claude,
   codex and claude_work wired; header `mem ●` green; Agent tab → Search
   memory finds something from an earlier session.
