# Spec: Remote team panel and clipboard

- **Date:** 13-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed

## Context

On connor-iva, devagentics exists in ~/.cargo/bin but the team board's Bash shell cannot find it. The clipboard helper emits a tmux-wrapped OSC 52 sequence, while the configuration does not explicitly enable passthrough. No live tmux server was available during inspection, so clipboard failure still needs regression coverage and terminal verification.

## Requirements

1. Team boards must find the installed Rust executable even when a new pane's shell omits ~/.cargo/bin from PATH.
2. Remote copying must deliver OSC 52 to the attached terminal with appropriate handling for direct SSH and nested tmux.
3. Add behavioral regression checks for a minimal Bash PATH and emitted clipboard data.
4. Document the commands needed after pulling to apply the fix to existing workspaces.

## Constraints

- Preserve unrelated working tree changes and existing agent sessions.
- Do not change the server during local implementation.
- Preserve mouse selection and existing local clipboard backends.
- Do not assume a successful terminal clipboard update from automated byte-level checks alone.

## Files Involved

Candidate files, to narrow during planning: config/tmuxinator/agentic.sh, config/tmuxinator/agentic.yml, config/tmux/yank.sh, config/tmux/tmux.conf, and relevant scripts/test_*.sh suites.

## Success Criteria

- [x] A board starts with the installed executable despite a minimal pane PATH.
- [x] Clipboard regression tests verify the correct OSC 52 payload and destination.
- [x] Relevant tests and syntax/lint checks pass.
- [x] Pull/reload instructions explain how to recover the failed board.
