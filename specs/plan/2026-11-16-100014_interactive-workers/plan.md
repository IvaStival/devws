# Readable worker panels (option B: protocol-driven, nothing typed)

Status: built and installed (2026-11-16, approved by the user); live check pending

## Why

Reviewed workers (planners, hammers, Codex reviewers) are driven through the
provider's programmatic interface: `codex exec --json` and `claude --print`.
The user wants that kept (2026-11-16, option B, no typing into prompts), but
the panel has to become readable. Today:
- **Codex:** `worker_tools::readable` prints agent messages as raw markdown,
  tool calls as `• worker <action>`, and failures as "no reason given"
  (the error text is dropped).
- **Claude:** `--output-format json` prints one JSON blob when the turn ends,
  and nothing while it runs.
- **Both:** no header says who the agent is, its model, or which order it's
  on, and turns aren't separated.

## Changes

1. **One event model for both providers:** a small `panel_view` module (one
   job: turn provider events into panel lines).
   - Codex `exec --json` events and Claude `--output-format stream-json
     --verbose` events map to the same `Event`s:
     `Message(text)`, `Tool { action, detail, outcome }`, `Command { line, exit }`,
     `Error(text)`, `TurnDone { tokens, seconds }`.
   - Claude moves to `stream-json`. Its session id and success detection keep
     working, so `resumed` still works.
2. **Rendering:**
   - **Agent messages:** rendered as markdown at the pane width (headings, bold,
     inline code, lists, code blocks, tables), with `termimad`. It shares
     crossterm with ratatui, which we already ship.
   - **Tool calls:** one dim line with the action and its key input, e.g.
     `• checkpoint "Add B1-1-AT tests"` or `• execute npm run typecheck`, then
     a green ✓ or a red ✕ with the **real error text**. It's read from
     `item.error.message`, or else the result's `content[].text` when
     `isError`, so "no reason given" only appears when there really is none.
   - **Commands:** `$ cmd`, plus `(exit N)` in red when it fails.
   - **A header per turn:** `── planner-5 · codex · gpt-6.1-sol · medium ·
     order o52 (B1-1-AT) · 10:12 ──`
   - **A footer per turn:** `── done in 2m 14s · 1.1M in (1.0M cached) · 8k out ──`,
     or a red `── failed: … ──`.
   - **Panel start:** the role and the model, so an idle panel isn't blank.
3. **Raw events kept:** each turn's raw provider lines are appended to
   `workers/<agent>/control/turns.jsonl` (the latest 2 MB), so a rendering
   problem or an odd error can be checked against what was really sent.
4. **Nothing else changes:** delivery, the queue, uncertain-turn recovery,
   isolation and the worker tools all stay as they are.
5. **Remove** the old `readable` once `panel_view` replaces it.

## Not doing

Codex `app-server` (JSON-RPC). It would give streaming deltas, but
`exec --json` already sends every finished item, and the thread
resumes. Revisit only if live token-by-token output turns out to matter.

## Tests (first)
- Codex and Claude sample lines (inline in `panel_view` tests, shaped like
  real output) map to the expected `Event`s, including a failed
  tool call that carries its error text.
- Rendering: markdown keeps bold, code and lists at a given width, and wraps
  rather than overflowing; the header and footer format; a failed tool is red with its
  reason.
- The Claude argument test now expects `stream-json` / `--verbose`.

## Verification
- `cargo test`, `cargo clippy --all-targets`, install.
- Live: a new planner/hammer panel shows the header, formatted messages, tool
  lines with reasons and the footer. A Claude worker shows output while it
  works. `turns.jsonl` holds the raw lines.
