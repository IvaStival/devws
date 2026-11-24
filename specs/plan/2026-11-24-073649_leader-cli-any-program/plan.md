# Leader CLI reads on every leader program (Claude, Codex, OpenCode)

- **Date** 24-11-2026 · **Branch** `feat/pstack-workflows` (worktree `~/Projects/devws-pstack`, already on top of Codex's latest work, snapshot `7a847e6`)
- **Saved to** `specs/plan/2026-11-24-<hhmmss>_leader-cli-any-program/plan.md` once approved
- **Your choices:** the leader on any program, **including a new OpenCode leader**; CLI reads stay **opt-in per task** until a Claude run shows they save money too. Workers stay on MCP.

## Context

The Codex session showed that the "hybrid" way is cheaper. In it, the leader reads status with a command (`devagentics leader-read …`) and writes through MCP. A finished Notes task cost $3.20 instead of $4.13, and then $2.87. But the code allows it only when the leader runs on Codex. Four places check `codex_team` (the "whole team on Codex" switch), because only Codex's read-only sandbox was trusted to run the command:

| Where | What it checks today |
|---|---|
| `main.rs:1119` (`leader-read`) | reviewed task **and `codex_team`** and `leader_cli` |
| `main.rs:734` (`task leader-cli on`) | refused unless **`codex_team`** |
| `mcp.rs:524` (shorter tool list) | `leader_cli` **and `codex_team`** |
| `start.rs:120-145` (leader launch) | the CLI paragraph is added only on the **Codex** branch |

Claude leaders start with **no shell at all** (`worker_tools::leader_arguments`: `--tools ""`, only `mcp__devagentics` allowed). OpenCode can't lead (`can_lead() == false` in `providers/opencode.rs` and `opencode_can_lead` in devws). So the CLI saving can't reach them.

## In plain words: what changes

1. **One rule instead of a Codex rule.** `task leader-cli <task> on` works for any task with a reviewed leader. Each leader program says how it lets the leader run **exactly one command**, `devagentics leader-read <this task> …`, and nothing else:
   - **Codex:** as today. Its read-only sandbox runs the command, and the MCP tools are the only tools approved in advance.
   - **Claude:** gets its Bash tool back, but only one command is allowed: `Bash(<devagentics> leader-read <task>:*)`. The permission mode `dontAsk` refuses anything else without asking. Example: `devagentics leader-read feat-x progress` runs; `cat ~/.ssh/id_rsa` or `devagentics task …` is refused.
   - **OpenCode:** a deny-all configuration. The leader's MCP tools are allowed, and bash allows only `<devagentics> leader-read <task> *`.
2. **The same instructions for every program.** The CLI paragraph Codex added to the opening message (progress, order, inbox; inbox_ack through MCP; MCP as fallback) becomes one shared function. Every leader program gets it, and gets it again after a handover. Example: a Claude leader that runs out of credit and moves to Codex keeps the CLI reads, because the switch belongs to the task, not to the program.
3. **The shorter tool list for everyone.** When CLI reads are on, any leader gets the narrower `task_status` and the shorter `leader_inbox` text (Codex's `--leader-cli-catalog`).
4. **OpenCode can lead a reviewed task.** If you pick an OpenCode account as leader, devAgentics starts it the way it starts OpenCode workers:
   - **Isolated:** its own config, home and cache. The repo's own `opencode.json` is ignored, and nothing from your own OpenCode setup is loaded except its login.
   - **Version-checked:** it runs only on the verified OpenCode 1.19.6.
   - **Status plugin kept:** the devws plugin still loads, so the board can see "working / waiting" and the dispatcher can wake the leader.
5. **Nothing changes for workers**, and nothing changes while `leader-cli` is off. Off is still the default.

## Design

### A. One code path for any leader program
- **Remove the `codex_team` gate from CLI reads.** `main.rs` `leader-read` and `TaskAction::LeaderCli` need `workflow == Reviewed`, plus `leader_cli` for the read. `mcp.rs:524` needs `leader_cli` and `Reviewed`. `codex_team` keeps its other job, deciding whether the team may run on Codex at all.
- **Shared prompt text.** `start::leader_cli_instructions(binary, task_id) -> String` holds Codex's current paragraph unchanged, so Codex's measured behaviour stays the same. `write_leader_args` appends it for **every** reviewed adapter when `state.leader_cli`.
- **Per-adapter permission.** `write_leader_args` passes `leader_cli` to each adapter's argument builder, so the allowed command is set in one place per program:
  - `worker_tools::codex_leader_arguments`: unchanged; it already takes `leader_cli`.
  - `worker_tools::leader_arguments` (Claude) gains `cli: Option<&str>`, the allowed command prefix. When it is set: `--tools Bash`, and `--allowedTools mcp__devagentics,Bash(<prefix>:*)`. `--permission-mode dontAsk` stays. The MCP args get `--leader-cli-catalog`, which today only Codex passes, through `agent_args::write_mcp_config`.
  - **New** `opencode_leader` (below) gets the bash pattern in its permission block.
- **Exact command text.** The allowed prefix uses the same binary path the prompt shows. The prompt uses `check_profiles::shell_quote`; when the path needs no quotes, the prompt shows it bare so it matches the permission rule. A test checks that the prompt's command and the allowed rule start with the same text.

### B. The OpenCode leader adapter
- **Can lead.** `providers/opencode.rs`: `can_lead() → true` and `reviewed_leader() → Opencode`. `leader::reviewed_leader_allowed` returns true for `Opencode`, so picking an OpenCode leader is your opt-in. devws `agents/30-opencode/agent.sh`: `opencode_can_lead` returns 0.
- **Configuration.** New `opencode_worker::leader_configuration(executable, task, home, cli, plugin)` builds on `configuration`:
  - `permission: {"*":"deny", "devagentics_*":"allow", "bash": {"*":"deny", "<prefix> *":"allow"}}`, with the bash part only when `cli` is set;
  - MCP `devagentics` = `[executable, "mcp", task, (--leader-cli-catalog)]`;
  - an agent `devagentics-leader`;
  - `plugin: [devws_plugin.js]`, so state and transcript reporting keep working.
- **Launch.** It reuses the worker path. `agent_runner.sh` already hands `--devagentics-worker` args to the devagentics binary. Add the twin `--devagentics-leader <exe> opencode-leader <task> <args-file>`. A new `opencode-leader` subcommand:
  - builds `opencode_worker::Adapter::prepare(..)` with the leader configuration;
  - runs `verify()` (version 1.19.6 and the resolved permissions);
  - sets the pane's tmux options the way `agent_runner.sh` does;
  - then `exec`s the OpenCode **TUI** (`opencode --agent devagentics-leader --model …`, with the opening prompt) with the worktree as the current folder. `leader-read` needs that folder.
- **Accounts.** Usage and limits come from the existing OpenCode usage code (`usage.sh`, `devws.credit`). Handover (`leader::led_by`, `take_over`) rewrites `leader.args` through `write_leader_args`, so moving to and from OpenCode needs nothing more.
- **Not in scope:** Antigravity as a leader; workers on CLI reads.

### C. Measuring (it stays opt-in until then)
- CLI reads are already counted on every program. This branch's `telemetry::cli` records each `leader-read` as a leader call named `cli:<op>`.
- **Claude A/B:** the same Notes fixture, with a Claude leader, MCP-only (A) against hybrid (B). Same pinned binary and same method as the Codex runs. If B costs less per finished task, the plan proposes making it the default in a separate step.
- **OpenCode smoke run:** one reviewed Notes task with an OpenCode leader, with CLI reads on. It passes if it finishes, uses only the allowed tools, and the board shows the leader's state.

## Files
- `agentic/src/main.rs`: `leader-read` gate, `TaskAction::LeaderCli`, new `opencode-leader` subcommand
- `agentic/src/mcp.rs`: catalog condition
- `agentic/src/start.rs`: `leader_cli_instructions`, `write_leader_args` for all adapters
- `agentic/src/worker_tools.rs`: `leader_arguments` CLI option
- `agentic/src/agent_args.rs`: `write_mcp_config` catalog flag
- `agentic/src/opencode_worker.rs`: `leader_configuration`, plus the TUI launch next to `Adapter`
- `agentic/src/providers/opencode.rs` and `agentic/src/leader.rs`: allow an OpenCode leader
- `config/tmuxinator/agent_runner.sh`: `--devagentics-leader` path
- `config/tmuxinator/agents/30-opencode/agent.sh`: `opencode_can_lead`
- Docs: `agentic/docs/usage.md` (`leader-cli` for any program), `docs/user/devagentics-bench.md` (the Claude arm), and the plan status

## Tests (written first)
- **Claude:** `task leader-cli on` is accepted on a Claude-led reviewed task. Its `leader.args` has `--tools Bash`, the single allowed rule `Bash(<bin> leader-read <task>:*)`, `dontAsk`, the CLI paragraph and the narrow catalog. With CLI reads off, the args are byte-for-byte today's.
- **Codex:** its args are unchanged, compared against the current output.
- **Handover:** CLI reads stay on after a handover from Claude to Codex and from Codex to Claude.
- **Gate:** `leader-read` works for a Claude-led task without `codex_team`, and still refuses a legacy task or a task with `leader_cli` off.
- **OpenCode:** its leader configuration denies everything except the leader MCP and the one bash pattern. An ignored test runs the real `opencode debug config` on 1.19.6, as the worker one does. A legacy task still refuses an OpenCode leader.
- **devws:** `agent_runner.sh` sends `--devagentics-leader` to the binary (`scripts/test_agent_cli.sh` pattern).
- **Live checks with the real programs, run by hand:**
  - a Claude leader runs `leader-read progress` without a prompt, and `ls` is refused;
  - the same for an OpenCode leader.
- **The usual gates:** `cargo test --offline`, the Docker tests (`-- --ignored`), `clippy -D warnings`, `fmt --check` and `make check`.

## Risks
- **Claude's Bash runs on the host, not in a sandbox.** Only the one prefix is allowed, and `dontAsk` refuses the rest. `leader-read` itself is read-only: it checks the task id and the folder, and leaves the task's state file unchanged, which a test already checks. The live check above confirms that refusals happen.
- **OpenCode models are cheap and may lead worse.** It's only used when you pick it. The smoke run shows whether one can finish a task.
- **The Codex session may edit the same files** (`start.rs`, `mcp.rs`, `main.rs`). The work is done in this worktree and rebased again, as before. Please tell me before Codex starts another change to leader-read.
- **Installing waits for this work.** The same question about the open board windows applies then.
