# devAgentics

Agent teams for devws, the module in this folder (it began as its own repo).
A planner
writes the plan; you approve it; a leader agent, the tech lead, starts
executors and helpers in each repo's worktree, hands them one order at a
time, checks their work, and opens linked draft pull requests for you to
review.

devws stays the screen and workspace layer (panes, agent states, worktrees);
devAgentics adds the team: roles, tasks across repos, order queues, the log
and the leader's tools.

Status: v1 built and tested end to end with a stub devws; not yet run with
real agents. Start with [docs/usage.md](docs/usage.md); the design is in
[docs/design.md](docs/design.md).

```sh
cargo test                                    # unit tests and a whole task through the binary
cargo clippy --all-targets -- -D warnings
cargo install --path . --locked              # puts `devagentics` on your PATH (./install.sh does it)
```
