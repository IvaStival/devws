# devws-ui: the devws menu and pickers in Rust (Zellij later)

## Context

The devws menu (C-b Space) and every question in a popup flicker. I measured the cause:
- Any `tmux set-option`, even an `@devws_*` value nothing displays, makes tmux redraw the whole
  client, popup included. devws scripts do this all the time.
- Every redraw ends by **re-showing the cursor and resetting its blink** (`\e[?12l\e[?25h`) when the
  popup's program has a visible cursor. fzf does.
- **Measured on a test server, 10 set-option calls:**

  | Popup program | Cursor re-shows |
  | --- | --- |
  | fzf | 16 |
  | a program that hides its cursor | 3 |

**Decided with the user:**
- Write a small Rust program, **`devws-ui`**, that draws the menu and every picker with the terminal
  cursor hidden and a caret of its own.
- It is a plain TUI on a tty, so it runs in a tmux popup now and in a **Zellij floating pane later**.
  Moving to Zellij is a separate project (step 2 below), not part of this plan.
- It goes in a new crate **`devws-ui/`**, separate from `agentic/`.

## Scope (v1)
- **`devws-ui menu`:** replaces the fzf menu in `devws_menu.sh` `show_menu`. That covers tabs,
  ←/→/Tab to switch, typing to search every tab, Enter to run, Esc to close, mouse click on a tab or
  an item, and remembering the current tab.
- **`devws-ui pick`:** replaces fzf inside `config/tmuxinator/picker.sh`, the ~32 questions:
  branches, repos, apps, settings and files. Callers don't change.
- **Not in v1:**
  - the sidebar `session_picker.sh` (fzf in a pane, not a popup, so it doesn't flicker);
  - `database_runner.sh`'s direct fzf.

## Design

### Crate `devws-ui/` (new, next to `agentic/`)
- `Cargo.toml`: edition 2024, `rust-version = "1.85"`. Dependencies: `ratatui` (same version as
  `agentic/`, 0.30), `crossterm` (through ratatui), `nucleo-matcher` for fuzzy matching, `clap`,
  `anyhow`.
- **Terminal:** it draws on and reads keys from **`/dev/tty`**, like fzf, so stdin carries the items
  and stdout carries the answer. The terminal cursor is hidden the whole time, and the caret is a
  reversed cell drawn in the prompt. It doesn't use the alternate screen; it fills the popup.
- **Modules, one job each:**
  - `items.rs`: reads lines, splits them on the delimiter, gives the displayed field (`--with-nth`),
    and keeps the original line for output. It strips ANSI codes for matching but keeps them for
    display.
  - `filter.rs`: filters with nucleo by the query, keeping input order when the query is empty.
  - `pick.rs`: picker state, which covers the cursor row, ticked rows for `--multi`, and the query.
  - `menu.rs`: menu state, which covers tabs, the current tab, and the search across every tab with
    rows tagged "Tab · ".
  - `view.rs`: ratatui rendering of the header, the tab bar, the list, the prompt with its caret,
    and the footer.
  - `tty.rs`: opens `/dev/tty`, sets raw mode, hides the cursor, and always restores the terminal on
    exit or panic.
  - `main.rs`: the CLI.
- **`devws-ui pick`:**
  - Options: `--prompt <p>`, `--header <h>`, `--footer <f>`, `--multi`, `--select-all`,
    `--delimiter <d>`, `--with-nth <n>`, `--print-query`.
  - It returns the chosen line, or one line per tick with `--multi`. With `--print-query`, the query
    comes first.
  - Exit codes are fzf's: 0 for a choice, 1 for no match, and **130 for Esc**.
- **`devws-ui menu`:**
  - Input on stdin, one row per action: `tab<TAB>action<TAB>label<TAB>shortcut`. Tab order follows
    first appearance, and `--tab <current>` picks the starting tab.
  - Output on Enter: `tab<TAB>action`. On Esc it prints `tab`, so the shell can still remember the
    tab, and exits 130.

### Shell side
- **`config/tmuxinator/picker.sh`:**
  - It uses `"${DEVWS_UI:-$HOME/.cargo/bin/devws-ui}" pick` when that binary exists, and fzf
    otherwise.
  - fzf options are translated: `--bind load:select-all` → `--select-all`; `--height`, `--info`,
    `--border` and similar are ignored.
  - Any option it doesn't know makes **that call fall back to fzf**, so no caller breaks.
  - The path is tied to `$HOME` on purpose. The test suites set their own `HOME`, so they keep using
    their fzf stubs and never pick up a real devws-ui.
- **`config/tmuxinator/devws_menu.sh` `show_menu`:**
  - With devws-ui present, it pipes every visible action of every tab as
    `tab<TAB>action<TAB>label<TAB>shortcut` into `devws-ui menu --tab "$(current_tab)"`. The rows
    are built from the existing `items` / `shown` / `tabs` logic, so there are no callbacks while
    the menu is open.
  - It stores the returned tab in `@devws_menu_tab` (the option `switch_tab` sets today) and runs
    the action through `run_action`.
  - The fzf path stays as the fallback. `--items`, `--switch` and the other callbacks stay too,
    because the fallback and the tests use them.
- **Install:** rename `config/tmuxinator/agentic_install.sh` to **`rust_tools.sh`**, which handles
  both programs:
  - `missing` prints `rust`, `rust-update`, `build devagentics` and `build devws-ui`.
  - The needed version is the highest `rust-version` of the two crates.
  - Each program is built with `cargo install --path <crate> --locked`, and each is recorded only
    if devws installed it.

  Update `agentic.sh`, `install.sh` `ensure_agentic` (renamed `ensure_rust_tools`), `delete.sh`
  (remove a devws-installed `devws-ui` too), and `test_agentic_install.sh` (renamed
  `test_rust_tools.sh`, with cases for the two programs).

## Tests first
- **Rust (`cargo test` in `devws-ui/`):**
  - items: delimiter/with-nth return the whole line, and ANSI is stripped for matching.
  - filter: empty query keeps input order, and fuzzy matching orders by score.
  - pick: ↑/↓ wrap, Tab ticks, select-all, the output order, `--print-query`, Esc → 130, no match → 1.
  - menu: ←/→ wrap across tabs, search spans every tab with tags, Enter returns `tab\taction`, and
    Esc returns the tab.
  - view (ratatui `TestBackend`): it renders the tab bar with the current tab marked and the caret
    cell, and **never asks to show the cursor**.
- **Shell:**
  - `scripts/test_picker.sh`: with a stub devws-ui at `$HOME/.cargo/bin`, options are translated and
    the answer and exit code pass through. An unknown option falls back to the fzf stub. Without
    devws-ui, behaviour is as today.
  - `scripts/test_devws_menu.sh`: with a stub `devws-ui menu`, the input rows match the visible
    actions (respecting `when` / `shown`), the returned tab is stored, and the action runs.
  - `scripts/test_rust_tools.sh`: the existing six cases, plus both programs built and recorded
    separately.
- **Add both Rust crates to `scripts/check.sh`'s cargo step.** It already runs cargo for `agentic/`,
  and devws-ui follows the same pattern.

## Critical files
- **New:** `devws-ui/` (`Cargo.toml`, `src/*.rs`), `config/tmuxinator/rust_tools.sh` (renamed from
  `agentic_install.sh`), `scripts/test_rust_tools.sh`.
- **Changed:**
  - `config/tmuxinator/picker.sh`, `config/tmuxinator/devws_menu.sh`, `config/tmuxinator/agentic.sh`
  - `install.sh`, `delete.sh`, `scripts/check.sh`
  - `scripts/test_picker.sh`, `scripts/test_devws_menu.sh`, `scripts/test_install_lifecycle.sh`
  - `docs/guide/` (menu, configuration), `README.md`

## Verification
1. `cargo test` in `devws-ui/`, plus `bash scripts/check.sh`. The 3 suites that already fail on main
   (`test_agent_model`, `test_git_actions`, `test_workspace_minimize`) stay out of scope.
2. **Flicker, measured:** re-run the scratchpad harness (`setopt.sh`: a popup while `set-option`
   runs every 0.3 s) with `devws-ui menu`. Expect at most 3 cursor re-shows, against 16 for fzf, and
   no `\e[?25h` inside the redraw frames.
3. **Live:** build with `cargo install --path devws-ui --locked` and open C-b Space while agents are
   working. The user confirms there's no flicker. Then try one picker of each kind: a branch (Run
   this branch), a multi-select (Commit files), and a typed answer (`--print-query`, database
   folder).
4. Run in a plain Zellij floating pane, after `brew install zellij`, to confirm it doesn't depend on
   tmux: `zellij run --floating -- devws-ui menu < rows.tsv`.

## Process
- Make branch `feat/devws-ui` off `main`, and run `/vault-update` after approval.
- Use the spec-plan skill: copy this into `specs/plan/2026-11-10-<time>_devws-ui/{spec,plan}.md`.
- Merge into `local/all-fixes` for live testing, and commit with the commit skill (no AI credits).

## Step 2, later (its own plan): the Zellij spike
Install Zellij and rebuild devws's workspace layout as a Zellij KDL layout: the editor, agent,
terminal and sidebar panes. Then list what maps and what doesn't across devws's ~450 tmux calls:
- popups → floating panes;
- `@devws_*` options → ?
- hooks;
- send-keys;
- the status/header bars.

That list is the input to a migration decision. No code moves in the spike.
