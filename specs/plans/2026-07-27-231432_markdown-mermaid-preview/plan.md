# Plan: Browser fallback for Mermaid diagrams in Markdown preview

- **Date:** 27-07-2026
- **Branch:** feat/interpret-md-files
- **Status:** Executed

## Context

Implements `specs/plans/2026-07-27-231432_markdown-mermaid-preview/spec.md`.
Glow cannot render Mermaid diagrams graphically, so a new shared script opens
a CDN-backed HTML preview in the system browser whenever a Markdown file has
a Mermaid fence, called from both `devws md` and LunarVim's `<leader>mp`,
alongside (not instead of) the existing Glow preview.

## Step 1 — Create the shared companion script

**Files:**
- `shell/render_mermaid_preview.sh` (new, executable)

**Actions:**
- `#!/usr/bin/env bash`, `set -euo pipefail`.
- Require exactly one argument (the Markdown file path); print usage to
  stderr and exit 2 otherwise. Exit 2 with an actionable message if the file
  is missing/unreadable.
- Detect a Mermaid fence with:
  `grep -Eiq '^[[:space:]]*(`{3,}|~{3,})[[:space:]]*mermaid[[:space:]]*$'`.
  If it doesn't match, exit 0 immediately — no file, no process, no output.
- On a match:
  - `tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/devws-mermaid.XXXXXX")"`,
    `html_file="$tmp_dir/preview.html"`.
  - Base64-encode the source file: `markdown_b64="$(base64 < "$markdown_file")"`.
  - Write a self-contained HTML file via heredoc:
    - `<script>` tags loading `marked@12` and `mermaid@10` from
      `cdn.jsdelivr.net` (pinned major versions).
    - The base64 string embedded in a JS template literal (`` `...` ``) —
      base64's alphabet never contains a backtick or `${`, so no escaping is
      needed.
    - Client-side JS: strip whitespace from the base64 string, decode through
      `Uint8Array` + `TextDecoder('utf-8')` (not bare `atob`, so accented
      characters — e.g. Portuguese — survive), `marked.parse()` the result
      into a container div, convert every `code.language-mermaid` element
      into a `<div class="mermaid">` with the same text content, then
      `mermaid.initialize({ startOnLoad: false }); mermaid.run();`.
    - Minimal readable CSS (max-width, margin, monospace code blocks).
  - Open it: `open "$html_file"` if available; else
    `(xdg-open "$html_file" >/dev/null 2>&1 &)`; else print a warning to
    stderr with `$html_file` so the user can open it manually. Never exit
    non-zero from this branch — this script is best-effort and must not break
    the caller's Glow preview.
- `chmod +x shell/render_mermaid_preview.sh`.

**Verification:**
- `bash -n shell/render_mermaid_preview.sh`.
- Manually run it against a scratch file with a ` ```mermaid ` fence and
  confirm a browser tab opens with a rendered diagram; run it against a file
  without one and confirm no file/process is created.

## Step 2 — Wire it into `devws md`

**Files:**
- `shell/devws.zsh`

**Actions:**
- In `devws-markdown()`, resolve this file's own directory with the
  `${(%):-%x}` prompt-expansion idiom (robust inside a sourced function,
  unlike `$0` in zsh): `local self_dir="${${(%):-%x}:A:h}"`.
- After the existing extension/readability validation and before
  `command glow -p "$markdown_file"`, add:
  ```zsh
  local mermaid_script="$self_dir/render_mermaid_preview.sh"
  [[ -x "$mermaid_script" ]] && "$mermaid_script" "$markdown_file"
  ```
- Keep the existing `command glow -p "$markdown_file"` call unchanged and
  last, so Glow always still runs.

**Verification:**
- `zsh -n shell/devws.zsh`.
- Manually source `shell/devws.zsh` and run `devws md` on a Mermaid-bearing
  fixture; confirm both a browser tab and the Glow pager open.

## Step 3 — Wire it into `<leader>mp` in LunarVim

**Files:**
- `config/lvim/config.lua`

**Actions:**
- Add a `repo_root()` helper near `preview_markdown_with_glow()`: read this
  chunk's own source path via `debug.getinfo(1, "S").source:sub(2)`, resolve
  it through the symlink with `vim.loop.fs_realpath(...)` (config.lua is
  symlinked to `~/.config/lvim/config.lua`; this recovers the real clone
  path), then `vim.fn.fnamemodify(real_file, ":h:h:h")` to go from
  `<root>/config/lvim/config.lua` up to `<root>`.
- Add a `run_mermaid_preview(markdown_file)` helper that builds
  `repo_root() .. "/shell/render_mermaid_preview.sh"` and, if
  `vim.fn.executable(script) == 1`, runs `vim.fn.system({ script, markdown_file })`.
- In `preview_markdown_with_glow()`, call `run_mermaid_preview(markdown_file)`
  right after `vim.cmd.write()` (so the on-disk file is current) and before
  the `botright 90vsplit` / `termopen` block that launches Glow.

**Verification:**
- `luac -p config/lvim/config.lua` (if `luac` is available, matching
  `scripts/check.sh`'s existing conditional check).
- Manually open a Mermaid-bearing Markdown file in LunarVim, press
  `<leader>mp`, and confirm a browser tab opens in addition to the Glow
  terminal split, and that the split still closes normally on Glow exit.

## Step 4 — Extend the test suite

**Files:**
- `scripts/test_markdown_viewer.sh`

**Actions:**
- In `make_fake_commands()`, add fake `open` and `xdg-open` executables that
  append their arguments to a log file path taken from an env var (e.g.
  `OPEN_LOG`), mirroring the existing fake `glow`/`fzf` pattern.
- In `run_devws()`, pass through `OPEN_LOG="$TEST_ROOT/open.log"` alongside
  the existing exported vars.
- Add `test_mermaid_fence_opens_browser()`: write a fixture `.md` file with a
  ` ```mermaid ` fence, run `devws md` on it, assert `open.log` contains an
  invocation with an `.html` path, and assert that path exists on disk.
- Add `test_no_mermaid_fence_skips_browser()`: reuse an existing non-Mermaid
  fixture, run `devws md` on it, assert `open.log` is empty/absent.
- Register both new test functions in the run list at the bottom of the file.

**Verification:**
- `bash scripts/test_markdown_viewer.sh` passes, including the two new cases,
  on a clean run.

## Step 5 — Update `scripts/check.sh`

**Files:**
- `scripts/check.sh`

**Actions:**
- Add `bash -n "$ROOT/shell/render_mermaid_preview.sh"` next to the other
  `bash -n` calls.
- Add `"$ROOT/shell/render_mermaid_preview.sh"` to the executable-bit check
  loop.

**Verification:**
- `bash scripts/check.sh` passes.

## Step 6 — Update documentation

**Files:**
- `README.md`

**Actions:**
- In the "Markdown preview" section, add a short paragraph: files containing
  a Mermaid fence additionally open in the system's default browser via a
  CDN-backed HTML preview (client-side Mermaid rendering), alongside the
  existing Glow terminal preview; this requires internet access, unlike the
  fully-offline Glow path.
- In the "Configuration" section's file list, add
  `Mermaid preview: shell/render_mermaid_preview.sh`.

**Verification:**
- Read-through: confirm no stale claims (e.g. don't imply Glow itself renders
  diagrams; don't imply the browser preview works offline).

## Final Verification

1. `make check` passes (runs `scripts/check.sh`, which runs both test
   scripts and the new/extended `bash -n` and executable-bit checks).
2. Manual smoke test of both entry points against the Mermaid fixture from
   the bug report (a `flowchart TB` block), confirming the diagram actually
   renders in the browser tab.
3. Manual smoke test of both entry points against a Mermaid-free Markdown
   file, confirming no behavior change (no browser activity, only Glow).
