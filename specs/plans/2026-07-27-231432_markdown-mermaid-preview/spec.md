# Spec: Browser fallback for Mermaid diagrams in Markdown preview

- **Date:** 27-07-2026
- **Branch:** feat/interpret-md-files
- **Status:** Executed

## Context

`devws md` and LunarVim's `<leader>mp` (added in the Glow markdown-preview
feature, see `specs/plans/2026-07-27-112803_markdown-viewer/spec.md`) always
render Markdown through Glow, a terminal renderer. Glow has no graphics
capability: a ```` ```mermaid ```` fenced code block is shown as plain
highlighted text instead of a rendered diagram, so any diagram-bearing doc is
unreadable as a diagram from either entry point.

Terminal rendering cannot draw a Mermaid diagram. The only way to render one
is a graphical Mermaid engine, which means a browser. This spec adds a
narrow, scoped exception to the original feature's "no second renderer"
constraint: a lightweight client-side HTML preview used *only* when a file
actually contains a Mermaid fence, opened *alongside* Glow rather than instead
of it. Glow remains the sole renderer for every file without a Mermaid fence,
and continues to render the rest of a Mermaid-bearing file's content in the
terminal exactly as it does today — this feature only adds the missing
diagram view, it does not replace Glow's role.

## Requirements

1. Detect whether a Markdown file contains at least one fenced code block
   whose info string is `mermaid` (case-insensitive, allowing leading
   whitespace before the fence, e.g. inside a list item).
2. When a Mermaid fence is detected, generate a self-contained HTML file that:
   - Renders the full Markdown document client-side (so surrounding prose,
     headings, etc. give the diagram context).
   - Loads a Markdown parser and Mermaid.js from a CDN (`jsdelivr`), pinned to
     a specific major version for Mermaid to avoid silent breakage.
   - Converts rendered ` ```mermaid ` code blocks into diagrams on page load.
   - Embeds the Markdown source safely (base64, decoded client-side) so
     backticks, `</script>`, and non-ASCII content in the source file cannot
     break the page.
3. Open that HTML file in the system's default browser:
   - macOS: `open`.
   - Linux: `xdg-open`.
   - If neither is available, print an actionable warning to stderr with the
     generated file's path and continue — this must not block or fail the
     existing Glow preview.
4. When no Mermaid fence is present, behavior is unchanged: only Glow runs,
   with no new process, file, or browser activity.
5. Apply this to both existing entry points:
   - `devws md [file]` (`shell/devws.zsh`).
   - `<leader>mp` in LunarVim (`config/lvim/config.lua`).
6. Both entry points must share one implementation of the detection/HTML
   generation/open logic (a single companion script), not two divergent
   copies.
7. Update `README.md`'s "Markdown preview" section to describe the new
   browser fallback and its CDN/internet-access requirement.
8. Extend `scripts/test_markdown_viewer.sh` to cover: a file with a Mermaid
   fence triggers the browser-open step with a generated HTML file, and a
   file without one does not.

## Constraints

- Do not replace Glow as the default/primary renderer; it still runs for
  every file, including ones with Mermaid fences.
- Do not vendor Mermaid.js or a Markdown parser into the repo — use CDN
  script tags (accepted trade-off: preview of Mermaid diagrams requires
  internet access; Glow itself still works fully offline).
- Do not add a new Homebrew/system dependency — `open`/`xdg-open` are assumed
  present on macOS/Linux respectively, matching the project's existing
  macOS/Linux support scope.
- The shared companion script must be resolvable from both a sourced Zsh
  function (`shell/devws.zsh`, not symlinked — sourced by absolute path from
  the clone) and from LunarVim's `config.lua` (symlinked into
  `~/.config/lvim/config.lua`, so its own path must be resolved through the
  symlink back to the repo clone to find sibling files).
- Leave the generated temp HTML file on disk after opening it (the browser
  reads it asynchronously); do not attempt synchronous cleanup.
- No changes to `install.sh`, `delete.sh`, or `Brewfile` (no new dependency
  to track/own/remove).

## Files Involved

| File | Action |
| ---- | ------ |
| `shell/render_mermaid_preview.sh` | Create — shared detection + HTML generation + browser-open script |
| `shell/devws.zsh` | Modify — call the companion script from `devws-markdown()` before running Glow |
| `config/lvim/config.lua` | Modify — resolve the repo root through the symlink and call the companion script from `preview_markdown_with_glow()` before opening the Glow terminal split |
| `scripts/test_markdown_viewer.sh` | Modify — add coverage for Mermaid-fence detection and browser-open invocation |
| `scripts/check.sh` | Modify — add syntax check + executable-bit check for the new script |
| `README.md` | Modify — document the browser fallback under "Markdown preview" |

## Success Criteria

- [ ] A Markdown file with a ` ```mermaid ` fence opens both a browser tab
      with rendered diagram(s) and the existing Glow terminal pager, from
      `devws md`.
- [ ] The same file previewed via `<leader>mp` in LunarVim also opens the
      browser tab, in addition to the existing Glow terminal split.
- [ ] A Markdown file without a Mermaid fence behaves exactly as before: only
      Glow runs, no browser activity.
- [ ] Missing `open`/`xdg-open` produces an actionable stderr warning and
      still lets Glow run.
- [ ] `make check` passes, including the new/extended tests.
- [ ] README documents the new behavior and its internet-access requirement.
