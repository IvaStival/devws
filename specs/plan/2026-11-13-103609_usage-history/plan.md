# Plan: Usage history in the devws menu

- **Date:** 13-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed
- **Authorization:** The user asked for a menu item showing the complete history of team and workspace token usage ("We can add a new menu item").

## Context

Usage is shown today only for live panes (each pane's conversation log, `@devws_transcript`, through `header_tokens.sh usage`) and for a running team's board. Once a pane closes, its usage disappears from view. Every conversation is still on disk: Claude's under `<account home>/projects`, Codex's under `<account home>/sessions`. Each one records the folder it ran in (`cwd`), and every message records its tokens, model and time. That is enough to rebuild the whole history.

## Design

- **Periods as columns**, so there are no filters to operate: Today · 7 days · 30 days · All time. Each cell shows the cost at API prices and the tokens.
- **Sections:**
  - **Workspaces**: one row per folder, named like the sidebar names it.
  - **Teams**: one row per devAgentics task. It covers the folders under `~/.devagentics/tasks/<task>/` (lanes and agents) plus its leader's workspace folder (`task.toml`).
  - **Accounts**: one row per `DEVWS_AGENTS` entry.
  - **Total.**
- **Reuse:** the pricing and parsing come from `header_tokens.sh` (`price`, `claude_messages`, Codex's `token_usage_record`, `agent_prices.tsv`) and the logs are read through `token_records` (`token_cache.pl`). Nothing is parsed twice.
- **Speed:** each log's per-day totals are cached by its fingerprint (size and modification time). Only new or changed logs are read again.

## Steps

1. `config/tmuxinator/header_tokens.sh`: **new mode** `days <agent> <log>`. It prints one line per day the log has usage: `day input output cache cost unpriced`, using the same rules as `usage`.
2. **New** `config/tmuxinator/usage_history.sh`:
   - `rows` lists every log of every account, with its folder and the per-day lines (cached), as TSV.
   - `show` prints the sections and the period columns.
3. `bin/devws`: **new** `devws usage`, which runs `usage_history.sh show`.
4. Menu: `tools|usage-history|◷ Usage history||show_usage_history` in `devws_menu.sh`. `show_usage_history` in `workspace_actions.sh` pages the table with `less`, the same way `list_network_devices` does.
5. Docs: `docs/guide/commands.md` and the tools page of the guide.

## Tests (`scripts/test_usage_history.sh`)

- Claude and Codex fixture logs in two folders, one of them under a task's lanes. Check that the workspace, team, account and total rows have the right tokens and costs, by period (a message from 10 days ago counts in 30 days and All time only).
- A log seen again unchanged is not parsed again (`DEVWS_TOKEN_CACHE_METRICS`). A changed log is re-read.
- The menu lists the item, and `devws usage` prints the table.
