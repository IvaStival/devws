# Spec: OpenCode workers in reviewed teams

- **Date:** 17-11-2026
- **Status:** Executed; verification limitations documented in plan.md

## Requirements

Support floor and senior OpenCode workers in reviewed teams through authenticated
worker MCP tools. Preserve account/global/group switches, models, credit guards,
fallback ordering and task-level Codex priority. Keep leaders on Claude/Codex.
Verify isolation from native tools, inherited configuration, plugins, subagents
and unrelated MCP servers before enabling eligibility. Preserve exact sessions,
provider/model metadata, transcript attribution, usage and explicit recovery.

## Success criteria

Regression tests cover selection, configuration isolation, sessions, events, usage,
startup fallback and uncertain delivery. A live installed-binary probe confirms
scoped tool exposure. Reviewed execution reaches checkpoint/check/review. Run
make check and report any independently reproduced baseline failures.
