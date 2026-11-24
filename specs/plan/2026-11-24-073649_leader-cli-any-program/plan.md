# Leader CLI reads on every leader program (Claude, Codex, OpenCode)

- **Date** 24-11-2026 · **Branch** `feat/pstack-workflows` (worktree `~/Projects/devws-pstack`, already on top of Codex's latest work, snapshot `7a847e6`)
- **Status** A and B built and tested (24-11-2026). C, the Claude A/B and the OpenCode smoke run, is left for the user to start.
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

## Results (24-11-2026)

- **A, Claude and Codex.** Commit `44fd0ac`. The `codex_team` gate is gone from all four places. A live probe on Claude Code 2.1.331, with the leader's exact flags and `dontAsk`, ran the read command and refused all of these, without a prompt and without writing a file:
  - `ls /`;
  - `;`, `&&` and a newline;
  - `|`, `>`, `2>` and `<`;
  - `$(…)` and backticks;
  - other `devagentics` commands.
- **Fail closed** (Codex's review point). A Claude version the probe did not pass (only 2.1.331 so far), an OpenCode version other than 1.19.6, or any other program starts the leader on MCP only. The task log says why: "leader reads stay on MCP: …".
- **B, the OpenCode leader.** OpenCode now leads reviewed tasks only, and legacy tasks refuse it. It runs through `agent_runner.sh --devagentics-leader` and `devagentics opencode-leader`:
  - isolated configuration, plus devws's state plugin;
  - checked in the worktree it runs in, with a hostile `opencode.json` there (an ignored test on the real 1.19.6);
  - the fixture-model probe `scripts/probe_opencode_leader.py`.

  The probe found that **OpenCode allowed `leader-read … > file`**. Its bash rules split `;`, `&&`, `|` and `$(…)` into separate commands, but not redirects. The leader configuration now also refuses `*>*`, `*<*` and backticks, and the probe asserts that every form is refused and no file is written.
- **Not done here.** `make check` fails `test_guide` (the guide lacks `DEVWS_PROC` and `DEVWS_SYS`) and `test_workspace_template` ("restore failed"). Both fail the same way on `main` and without these changes.

### Codex review fixes (24-11-2026)

1. **The OpenCode leader's reads found no task.** OpenCode runs with an isolated `HOME`, so `leader-read` looked under the wrong home. The TUI now gets `DEVAGENTICS_HOME`, and a unit test checks the command's environment. The configuration test couldn't catch this, because it doesn't run the read.
2. **The Claude gate allowed untested patches.** It now takes only versions in `CLAUDE_VERIFIED` (2.1.331). `scripts/probe_claude_leader.sh` reruns the probe in one command; it passed on 2.1.331.
3. **Codex reads could be missing from the ledger.** Its read-only sandbox can refuse the ledger write. `bench report --leader-transcript` now counts `leader-read` tool calls in Codex rollouts and Claude sessions, and `leader calls per accepted step` uses the larger count. On the named-section Notes replay's rollout, the count is 33 (28 inbox, 4 progress, 1 order).

### Second Codex review (24-11-2026)

1. **The Claude probe could pass without a read.** It counted zero refusals, which also passes when Claude skips the call or the read fails. It now makes a disposable reviewed task (temporary home, stub devws and tmux). The read must run exactly once and print the task's JSON. Each other command must be refused as that exact command; if Claude runs anything else, the probe fails. It passed on 2.1.331. With the rule widened to all of Bash, it failed at `ls /`.
2. **The transcript counter counted text that never ran**, such as `printf 'leader-read …'`. A call now counts only when it is one plain `<devagentics> leader-read <task> <view>` invocation and its result succeeded: Codex `exit_code` 0, or a Claude `tool_result` that isn't an error. A test covers `printf`, `echo`, a chain, a failed read, another task and a call with no result. Codex's real rollout still counts 33.

### Third Codex review (24-11-2026)

1. **A transcript given twice doubled the count.** Calls are now keyed by their id across all transcripts, and a file passed twice is read once.
2. **In the Claude probe, a refusal could hide a successful run.** A successful result now always wins: a forbidden command that ran even once fails the probe. Checked on fake traces (ran and refused gives "ran"; only refused gives "refused"). The live probe still passes on 2.1.331.
