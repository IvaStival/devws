# The leader's tools always run the installed devAgentics

Status: built and installed (2026-11-16, approved by the user); live check done

## Why

On 2026-11-16 the feat-system-observer leader kept failing to reach things:
- its devagentics tool server had started at 12:28 and still ran the binary
  deleted by later reinstalls, so it never got `amend` or the section reads,
  and asked the user for grep, sed or a switch to Codex;
- `task_status` (299 KB) and workflow status (246 KB) were too big, so Claude
  Code saved them to files the leader cannot read;
- two errors didn't say what to do: `send_order` with `check` on a repo
  with named profiles, and `run_check` on an order already closed.

The user asked that the leader always load the entire updated version.

## Changes
1. **Relay to the installed binary** (`mcp::Server::serve`): when this
   server's binary has been replaced (`executable::replacement`), each request
   line goes to a fresh `<installed> mcp <task>`, and its answer comes back
   as it is. The handler keeps no state between requests, so nothing is
   lost. The first relayed request also sends
   `notifications/tools/list_changed`, so the client reloads the tool list
   and descriptions, and `initialize` declares `tools.listChanged`.
2. **Compact JSON** for `task_status` and `application_workflow` results
   (pretty printing doubled their size).
3. **A size guard:** a result over 80,000 characters is refused, with its
   size and how to ask for less, instead of being turned into a file the
   leader cannot open.
4. **Errors that say what to do:**
   - `send_order` with `check` on a repo with named profiles names the
     profiles and says to leave `check` out or to pass `profiles`.
   - `run_check` on a closed order says to send a new order for its step.

## Tests first
- `replacement` gives the installed path only for a replaced binary that
  has a replacement.
- The relay sends the line to the child and returns its output. A
  notification gets nothing back. `list_changed` is sent once.
- A result over the limit becomes an error with its size and a hint.
- The two error texts.

## Verification
`cargo test`, `cargo clippy --all-targets`, install. Reconnect the leader's
server once, since the running one predates the relay. Then install again
and check that the leader's next call runs on the new binary.

## Follow-up: a reviewed lane takes the team's history (2026-11-16)
When the leader accepted planning steps B1-1-plan and B1-2-plan, devAgentics
committed their history to the task branch while the front lane still held
B1-1's unmerged candidate (d7e1a9f). Reviewed catch-up allowed only a
fast-forward, so every new B1-1 order was refused. Now `lanes::catch_up_reviewed`
merges when every path the branch added is under `.devagentics/history/`.
Any other divergence is still refused as needing a separately reviewed
integration. Live: order o161 went through, and the lane merged the history
commit and kept d7e1a9f.

## Follow-up: a reviewer sees the checks and the plan (2026-11-16)
code-reviewer-3 blocked r171 with no code defect. It couldn't read the check
results, because `review_diff` gave only evidence hashes and its role can't
run checks. `plan_read B1-1` also found nothing, because the plan is stored
as B1-1-plan. Now:
- `review_diff` adds `plan` and `checks` (`reviews::check_results`). These are
  the order's and the candidate's profiles, with outcome, exit code, time and
  diagnostic.
- `plan_read` with no step reads the plan of the order under review. A named
  step without a plan of its own gives the plan its latest order followed.
