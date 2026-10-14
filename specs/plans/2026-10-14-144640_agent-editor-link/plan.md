# Plan: Link the agent with the editor

- **Date:** 14-10-2026
- **Branch:** feat/agent-editor-link
- **Status:** Executed

## Context

In a devws workspace the agent (Claude Code or Codex) and Neovim sit side by side but know
nothing of each other: the editor only reloads changed files (`config/nvim/lua/config/autocmds.lua`
timer). The user wants (1) the editor to **follow the agent**: show each file the agent changes,
at the changed line; and (2) the agent to **open things in the editor** when asked ("show me X").
Decisions: following is on by default with a toggle; both Claude Code and Codex get the link.

Today: `editor_runner.sh` runs plain `nvim .` (no server socket); `agent_runner.sh` runs bare
`claude` / `codex` (no flags). Nothing touches the user's own agent settings, and nothing should.

## Approach

1. **Neovim listens.** `config/tmuxinator/editor_runner.sh`: for nvim, `exec nvim --listen
   "$socket" .` with `socket=${TMPDIR:-/tmp}/devws-nvim-<session>.sock` (stale file removed first);
   the session option `@devws_editor_socket` records it. Other editors: unchanged, no link.

2. **One command opens a location in the editor:** new `config/tmuxinator/editor_open.sh
   <file>[:line] [--follow]` (linked to `~/.tmuxinator/` by install.sh's existing glob).
   - Finds the session from `$TMUX_PANE` (agents and their hooks run inside the agent pane),
     reads `@devws_editor_socket`; no socket / not nvim → exits quietly (hooks must never fail
     the agent).
   - `--follow` (used by hooks) does nothing when following is off (`@devws_follow` = off).
   - Calls `nvim --server "$socket" --remote-expr "v:lua.require'devws.follow'.show(...)"`.
     It never changes the tmux focus: the user stays in the agent pane.

3. **Neovim side:** new `config/nvim/lua/devws/follow.lua`, `show(path, line, text)`:
   - Relative paths resolve against the project root; opens the file (switches to it if open),
     reloads it, puts the cursor on `line` (or on the first line of `text` when only the
     changed text is known), centres it and briefly highlights the changed lines.
   - Does not disturb the user: while they are in insert / command mode, or the current buffer has
     unsaved changes, it only shows a notification ("agent changed foo.php:42") instead of jumping.

4. **Follow hooks**, new `config/tmuxinator/agent_follow_hook.sh <claude|codex>`: reads the
   hook JSON on stdin with `jq` (ships with macOS), extracts the file and changed text, and calls
   `editor_open.sh --follow`. Always exits 0.
   - Claude Code: `PostToolUse` on `Edit|MultiEdit|Write|NotebookEdit` (`tool_input.file_path`,
     `new_string` / `content` → first changed line).
   - Codex: `PostToolUse` on `apply_patch`; the patch text names the files (`*** Update File:` /
     `*** Add File:`) and its first `+` line locates the change; the last file is shown.

5. **Wiring without touching the user's settings** (`config/tmuxinator/agent_runner.sh`):
   - claude: `exec claude --settings ~/.tmuxinator/agent_settings.json --append-system-prompt
     "$(cat ~/.tmuxinator/agent_editor_prompt.md)"`; new `agent_settings.json` holds only the hook
     (Claude merges it with the user's own settings).
   - codex: `exec codex -c 'hooks.PostToolUse=[…]' -c "developer_instructions=…"` with the same
     prompt. Codex may ask once to trust the hook (its `trusted_hash`); documented.
   - New `config/tmuxinator/agent_editor_prompt.md`, short: "The user's Neovim editor is beside
     you. To show them a file or a place in it, run `~/.tmuxinator/editor_open.sh <path>[:<line>]`.
     Use it when they ask to see, open or show something."

6. **Toggle:** session option `@devws_follow` (default on, set in `dev.yml` `pre_window`).
   - `C-b F` in `config/tmux/tmux.conf` (devws bindings section, after line 435) runs new
     `toggle_follow.sh '#{session_name}'`, which flips it and shows a tmux message.
   - Workspace menu entry "Follow agent in editor" (`session_picker.sh`: function, header line,
     click number, dispatch case — renumbering later entries).
   - Agent pane header (`tmux.conf:418` `pane-border-format`) shows `· following` when on.

7. **Docs:** README "Editor: Neovim with LazyVim" (line 203) gets a "Following the agent"
   subsection; the shortcut table (line 308) gets `C-b F`; Configuration file map lists the new
   files.

## Reuse

- `workspace_panes.sh` `pane_with_role` (not needed for nvim, which is reached by socket).
- install.sh / delete.sh globs link and unlink new `config/tmuxinator/*` files automatically.
- Test harness pattern of `scripts/test_workspace_database.sh` (isolated tmux socket, PATH stubs,
  `wait_until`, sourcing only the devws bindings from tmux.conf).
- `autocmds.lua` reload timer stays as is.

## Tests (new `scripts/test_agent_follow.sh`, added to `scripts/check.sh` in its 3 lists)

- A real headless Neovim (`--listen` socket, devws `lua/` on the runtime path) in an isolated
  tmux session with `@devws_editor_socket` set:
  - `editor_open.sh src/a.php:3` → Neovim's current file and cursor line are a.php / 3.
  - Claude `Edit` and `Write` payloads, and a Codex `apply_patch` payload, through
    `agent_follow_hook.sh` → the right file and line.
  - Following off → `--follow` does nothing; a plain `editor_open.sh` still opens.
  - Insert mode → no jump, a notification instead.
  - No socket / Neovim not running → exit 0, no output.
- `C-b F` toggles `@devws_follow` and the header text.
- `agent_runner.sh` with stub `claude` / `codex` on PATH → receives the `--settings` /
  `--append-system-prompt` and `-c hooks…` / `developer_instructions` arguments.

## Verification

1. `./scripts/check.sh` (syntax, luac, all tests).
2. Real workspace: `devws ~/Projects/devws claude` — ask Claude to edit a file → the editor jumps
   to the change while focus stays in the agent pane; "show me the bufferline config" → it opens;
   `C-b F` off → edits no longer move the editor.
3. Same with `codex` (approve the hook once if Codex asks).
4. Commit with the commit skill (no AI credits).

## Notes from implementation

- Claude Code: `agent_settings.json` also allows `Bash(~/.tmuxinator/editor_open.sh:*)`, so
  "show me" requests don't ask for permission each time.
- Codex runs a new hook only after the user reviews it (TUI "Hooks need review"); `codex exec`
  skips untrusted hooks silently. Verified with `--dangerously-bypass-hook-trust` in a test run
  only; devws does not bypass trust.
- editor_open.sh gives up on the Neovim call after 5 s: a Neovim waiting on a prompt would
  otherwise block every call.
- Verified with the real agents against a headless Neovim: a Claude edit moved the editor to
  a.php:4; "show me line two" opened a.php:2 without a permission prompt; a Codex patch moved
  it to a.php:5.

## Follow-up: follow modes (14-10-2026)

The on/off toggle became a menu of modes (`follow_mode.sh`, `C-b F`, menu "◎ Agent follow"),
stored in `@devws_follow`: `follow` (default), `notify`, `focus` (also selects the editor pane),
`explore` (also follows Claude Code's Read calls) and `off`; the old "on" counts as `follow`.
The agent header names the mode. Verified with a real tmux menu (pick by key) and a real Claude
Read in explore mode.
