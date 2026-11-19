# Architecture: AI programs as modules

Each AI program devws runs (Claude Code, Codex, opencode, Antigravity) is one
module on each side: a folder of shell hooks for devws and a Rust provider
for devAgentics. No other script or Rust file names a program. Installing a
new program means adding its two modules. Tests fail when a hook is missing,
when one side has a program the other lacks, or when a shared script
branches on a program by name.

## devws: `config/tmuxinator/agents/<n>-<kind>/`

`agent.sh` defines the program's hooks as `<kind>_<hook>` functions and
nothing else at its top level. The number orders the programs: header rows,
the Register agent kind picker, and the installed programs offered without
registering. Files the program needs sit beside it (opencode's `usage.sh`,
`credit.sh`, `menu.sh` and `devws_plugin.js`; Antigravity's `hook.sh`,
`mcp.sh`, `plugin.sh` and `menu.sh`). They find devws's scripts two folders
up.

`agents.sh` loads every module:
- `agent_kinds` lists the modules' kinds.
- `agent_hook <kind> <hook> [args]` calls the module's function, or the
  default in `agents.sh`.
- `agent_defines` tells whether a module has its own hook.
- The registry functions (`agent_kind`, `agent_home`, `agent_names`…) ask
  the modules too.

**Required hooks:**

| Hook | Answers |
|---|---|
| `program` | the command that runs it (`agy` for antigravity) |
| `default_home` | its account folder when none is registered |
| `launch <resume> <prompt-file> <args…>` | runs it in the agent pane, with devws's editor prompt, hooks and status line, and devAgentics' arguments |
| `interrupt_keys` | the keys that stop a turn, a moment apart (`Escape Escape`) |

**Optional hooks** (the defaults in `agents.sh` do nothing or say no):

| Hook | Used by |
|---|---|
| `account_variables`, `account_env <home>` | `agent_runner.sh`, `memory.sh`: the variables that point it at an account |
| `prepare <agent>` | `agent_runner.sh`: trust a task folder, install a plugin |
| `pane_options` | `agent_runner.sh`: options it records on its pane, cleared when the pane is reused |
| `default_folders` | `workspace_actions.sh`: the folders offered when registering an account |
| `limits`, `live_limits`, `default_model`, `models`, `credit` | `agents.sh`, `agent_cli.sh` (`devws agent limits`, `default-models`, `models`, `credit`) |
| `status_line_limits`, `status_line_model`, `status_line_extra`, `settings_file` | `agent_statusline.sh` |
| `interrupt_mark`, `interrupt_seen`, `interrupted` | `agent_cli.sh interrupt`, `agent_interrupt.sh`: for a program that runs no hook when a turn stops |
| `follow` | `agent_follow_hook.sh`: reads its tool call, prints `<event>`, `<file>`, `<text>` |
| `owns_transcript`, `session`, `usage`, `messages`, `model`, `month`, `log_limits` | `header_tokens.sh`, inside it: they use its jq helpers (`human`, `epoch`, `price`), `token_records`, `recent_logs` and `prices_file` |
| `usage_logs`, or `usage_rows` with `usage_database` | `usage_history.sh` |
| `init_project <project> <template>` | `init_agent.sh` (`agent_init_agents_md` writes AGENTS.md) |
| `memory_client` | `memory.sh`, `register_agent.sh`: its ai-memory client, if any |
| `can_lead`, `team_account` | `agentic.sh`, `devws agent accounts` |
| `menu_entry`, `screen` | `devws_menu.sh`: its own screen in the Agent tab |
| `commit_command` | `git_actions.sh`: its command for committing |

Shared helpers in `agents.sh`: `agent_task_folder` (the current folder, when
it is one devAgentics made) and `agent_module_dir <kind>`.

## devAgentics: `agentic/src/providers/<kind>.rs`

`Kind` is the stable id stored in state and roles. `Kind::provider()` is the
only `match` on it, and `Kind::ALL` lists the kinds. A provider implements
`Provider` (`providers/mod.rs`):
- its arguments;
- the models, efforts and approval modes a role setting takes;
- the floor's family and model;
- the fallback a role without one gets;
- its default approvals and sandbox, shown by the board and `task_status`;
- its capabilities, which have defaults: `can_lead`, `lists_models`,
  `provider_models`, `own_switches`, `prepaid`, `reviewed_worker` and
  `reviewed_leader`.

Reviewed tasks run a program only through a locked-down adapter
(`ReviewedAdapter`). Its provider names the adapter; one without an adapter
is not run there.

## Adding a program

1. **devws:** add `config/tmuxinator/agents/<n>-<kind>/agent.sh` with the
   required hooks and whichever optional ones apply, plus its own files.
2. **devAgentics:**
   - add `agentic/src/providers/<kind>.rs`;
   - add a `Kind` variant, its line in `Kind::provider` and its place in
     `Kind::ALL`.
3. **Tests:**
   - Give the module a test like `scripts/test_antigravity.sh`.
   - The suites below check the rest.

| Test | Fails when |
|---|---|
| `scripts/test_agent_modules.sh` | a module misses a required hook or runs code when loaded; a shared script branches on a program; or a new module, added alone, is not offered, listed, shown in the header, registered and run |
| `providers::tests::devws_has_a_module_for_every_provider` | the devws modules and the Rust providers differ |
| the compiler | a `Kind` has no provider, or a provider misses a method |
