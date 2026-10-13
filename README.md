# devws

`devws` turns a project folder into a persistent terminal workspace with:

- a full-height workspace switcher and command menu;
- an editor pane (Neovim with LazyVim, Vim, VS Code, or Zed);
- a Claude Code or Codex agent pane;
- a regular terminal pane;
- two layouts to switch between, and agent/terminal panes that minimize to
  small bars while you code;
- Git branch and clean/dirty status in the workspace list.

Each project runs in its own tmux session. The menu can create, switch, and
close workspaces; switch or restart the agent; switch the editor; create
terminals; toggle the layout; initialize project instructions for the active
agent; and open the project in Finder, Zed, or VS Code.

![devws workspace panel with multi-line workspace details](docs/images/application-panel.png)

Keyboard selection is limited to workspace rows. The command header is
mouse-driven: click a command once to run it.

## Requirements

The automatic dependency installer supports macOS and Linux systems with
[Homebrew](https://brew.sh).

Core dependencies:

- Bash and Zsh
- Git
- tmux 3.2 or newer
- tmuxinator
- fzf
- Glow
- Neovim 0.11.2 or newer, set up with [LazyVim](https://www.lazyvim.org)
- ripgrep, fd, and the tree-sitter CLI (used by LazyVim)
- `tree`
- Ruby (used by tmuxinator)
- one AI agent: [Claude Code](https://docs.anthropic.com/en/docs/claude-code)
  or [Codex](https://developers.openai.com/codex/)

The included tmux theme uses
`JetBrainsMono Nerd Font Mono`. The Brewfile installs the font on macOS, but
you must select it in your terminal profile.

Optional GUI integrations are Finder (macOS), Zed, and VS Code. The menu hides
no errors if Zed or VS Code is unavailable; those actions simply do nothing.

Vim, VS Code, and Zed are optional alternate editors. Only Neovim is installed
by the dependency installer; picking another editor requires it to already be
on `PATH`.

LazyVim needs no separate install: `~/.config/nvim` links to `config/nvim`,
and the first `nvim` start downloads LazyVim, its plugins, language servers and
parsers. Plugin versions are pinned by `config/nvim/lazy-lock.json`; update them
with `:Lazy update` and commit the lock file to move every machine forward.
devws no longer uses LunarVim. Installing removes the old devws links in
`~/.config/lvim` and restores what was there before; uninstall LunarVim
yourself if you no longer need it.

### Shift+Enter in iTerm2

iTerm2 sends a plain carriage return for both Enter and Shift+Enter, so nothing
downstream — shell, tmux, or agent — can tell them apart, and Shift+Enter
submits instead of inserting a newline. The installer offers to fix this, or
run it on its own:

```sh
./install.sh --iterm-keys   # or: make install-iterm-keys
```

**Quit iTerm2 first.** It keeps its preferences in memory and rewrites them on
quit, which would discard the change. Reopen iTerm2 afterwards to pick it up.

The mapping sends `ESC CR` (hex `0x1b 0x0d`, the same bytes as Option+Enter) for
Shift+Enter, added to iTerm2's global key map alongside its built-in bindings. If
an agent ignores it, `scripts/setup_iterm_keys.sh` documents two alternatives at
the top: `0x0a` (Ctrl-J) and the CSI-u sequence `[13;2u`. `delete.sh` removes the
mapping again.

## Install

Clone the repository, then run:

```sh
./install.sh --deps
source ~/.zshrc
```

`fzf` is required for the workspace picker and is checked on every install. If
it is missing, the installer asks permission before installing it with
Homebrew and stops without changing configuration when permission is denied.

Every install upgrades an outdated Homebrew-managed tmuxinator automatically.
A tmuxinator installed by another package manager is left unchanged. `--deps`
installs the Brewfile and tmux plugin manager. If your other
dependencies are already installed, use `./install.sh`.

The installer creates symlinks into the clone. Keep the cloned folder in its
original location after installation. Existing managed files are moved to a
timestamped directory under `~/.devws-backups/` before replacement.

When installation finishes, run the highlighted reload command it prints:

```sh
source ~/.zshrc
```

Install either Codex or Claude Code separately, authenticate it, and edit
`config/tmuxinator/.env` if you want to change the available/default agent or
editor.

## Use

Start a workspace with the interactive agent and editor picker:

```sh
devws ~/Projects/my-app
```

Choose an agent and editor directly:

```sh
devws ~/Projects/my-app codex nvim
devws ~/Projects/my-app claude nvim
```

Control the left workspace menu from any pane:

```sh
devws menu open
devws menu close
devws menu toggle
```

Target another open workspace by folder/session name:

```sh
devws menu open my-app
```

### Layouts

Each workspace can switch between two arrangements at any time. Choose
**▥ Toggle layout** in the workspace menu or press `C-b \`:

```
default                       full-height editor
┌──┬───────────────┐          ┌──┬─────────┬───────┐
│m │    editor     │          │m │         │ agent │
│e ├───────┬───────┤    ⇄     │e │ editor  ├───────┤
│n │ agent │ term  │          │n │         │ term  │
└──┴───────┴───────┘          └──┴─────────┴───────┘
```

Panes are moved, not restarted, so the editor, agent and shell keep running,
and focus stays where it was. Extra terminals move with the terminal. New
workspaces open in the default layout; the current one is saved with the
workspace.

### Minimize the agent and terminal

The agent and terminal panes have a header: `▾ 󰚩 Agent · claude` and
`▾  Terminal`. Click either header, or press `C-b _`, to minimize both so the
editor gets the space; do it again to bring them back at their previous size.

```
default layout, minimized
├──▸ Agent──────────────────┬──▸ Term─────────────────
│ Agent · claude · minimized — click the header or press C-b _

full-height editor, minimized: a 9-column strip on the right
┬──▸ Agent
│
├──▸ Term
│
```

- In the default layout the bottom row shrinks to one line; in the full-height
  editor layout the right column shrinks to a strip just wide enough for
  `▸ Agent`. Minimized headers are plain text, without icons, to keep the
  bars small.
- The real agent and terminal keep running in a background window
  (`devws-minimized`, hidden from the status bar); placeholders stand in for
  them until you restore.
- Focus moves to the editor if it was on a pane being minimized.
- Toggling the layout, and the menu's agent and terminal commands, restore the
  panes first.
- A click toggles only when the mouse is released without dragging, so
  dragging a header still resizes the panes.
- The icons need a Nerd Font (see Requirements). The editor gets a blank
  header line too, because tmux enables headers for a whole window.

Workspaces opened before this feature have no headers; turn them on with
`tmux set -w -t <session>: pane-border-status top`, or reopen the workspace.

### Editor: Neovim with LazyVim

The default editor is Neovim configured with [LazyVim](https://www.lazyvim.org)
(`config/nvim/`). devws adds a few things for working next to an agent:

- Uncommitted changes are highlighted with a dark-green background, and the
  changed words more strongly, so the agent's edits stand out.
- Files the agent changes on disk reload automatically within a second.
- `<leader>mp` previews Markdown (see [Markdown preview](#markdown-preview)).
- PHP uses intelephense, plus phpactor for "go to implementation".

The leader key is `Space`; press it and wait to see every shortcut. The ones
used most:

| Keys | Action |
| ---- | ------ |
| `Space e` | Show or hide the file explorer (opens on its own for `nvim .`) |
| `Space f f` | Find a file by name |
| `Space /` | Search text in the project |
| `gd` / `gr` / `gI` | Go to definition / references / implementation |
| `K` | Show documentation for the symbol under the cursor |
| `Ctrl-o` / `Ctrl-i` | Jump back / forward |
| `Space g v` | Turn the change highlights off and on |
| `Space m p` | Toggle the Markdown preview |

### tmux shortcuts added by devws

The tmux prefix is `C-b` (tmux's default key set is restored by
`config/tmux/default-keys.conf`).

| Keys | Action |
| ---- | ------ |
| `C-b \` | Toggle the workspace layout |
| `C-b _` | Minimize or restore the agent and terminal |
| Click a header | Minimize or restore the agent and terminal |

### Session persistence

Open workspaces are saved automatically, so a reboot or an iTerm2/tmux crash
does not cost you the arrangement. Saved per workspace:

- the workspace folder, agent, and editor
- the pane layout (including the full-height editor layout) and each pane's
  working directory
- extra terminals created from the workspace menu
- whether the workspace menu was open or closed

State lives in `~/.devws-state/sessions.tsv` and is rewritten whenever a
workspace is created or closed, a client detaches, the agent or editor is
swapped, a terminal is added, or the menu is toggled.

The first `devws` run that finds no tmux server reopens the saved workspaces
before starting the one you asked for, then attaches you to it. Restore and
save can also be driven by hand:

```sh
devws restore              # reopen the saved workspaces and attach
devws restore --no-attach  # reopen them in the background
devws save                 # snapshot the current workspaces now
devws fresh                # forget the saved workspaces
```

Set `DEVWS_AUTO_RESTORE=0` in `config/tmuxinator/.env` to turn the automatic
restore off and keep only the explicit commands.

Three limits are worth knowing: a workspace whose folder no longer exists is
skipped with a message; an agent or editor that is no longer listed in
`.env` falls back to the configured default; and programs other than the agent
and the editor are not restarted — restored terminals come back at a shell
prompt in their saved directory. A workspace saved while minimized comes back
with small agent and terminal panes; click a header or press `C-b _` to expand
them.

### Initialize an agent

Click **Initialize agent** in the workspace menu to seed the current project
for the workspace's active agent:

- Claude creates `CLAUDE.md` and `.claude/skills/`.
- Codex creates `AGENTS.md` and `.agents/skills/`.

The shared template includes reusable TDD, SOLID, naming, simplicity, testing,
and planning rules. Existing instruction files and skills are never
overwritten.

## Markdown preview

Open a Markdown file with Glow:

```sh
devws md README.md
```

Run the command without a path to choose a `.md` or `.markdown` file below the
current directory with `fzf`:

```sh
devws md
```

Inside Neovim, press `<leader>mp` while editing a Markdown file. Modified
files are saved, then the rendered Markdown replaces the source in the same
window; scroll it with the usual Vim motions and press `<leader>mp` again to
return to the source.

Glow is installed by `./install.sh --deps` through Homebrew on macOS and
Linux. Both entry points report how to install it when it is unavailable.

Glow renders Markdown as terminal text, so it cannot draw Mermaid diagrams —
a ```` ```mermaid ```` fence shows up as plain code. When a file contains one,
both entry points additionally open a rendered preview in your default
browser (`open` on macOS, `xdg-open` on Linux) alongside the Glow preview.
That preview loads its Markdown/Mermaid renderer from a CDN, so it requires
internet access; files without a Mermaid fence are unaffected and only use
Glow, exactly as before.

## Menu indicators

- A dark gray background marks the active workspace.
- A lighter opaque gray background marks the selected workspace.
- Yellow `●` means the Git working tree has uncommitted or untracked changes.
- Green `✓` means the Git working tree is clean.

## Configuration

- Agents and editors, automatic restore: `config/tmuxinator/.env`
- Saved workspaces: `~/.devws-state/sessions.tsv`
- iTerm2 Shift+Enter mapping: `scripts/setup_iterm_keys.sh`
- Workspace layout: `config/tmuxinator/dev.yml`
- Layout toggle: `config/tmuxinator/toggle_layout.sh`
- Minimize agent and terminal: `config/tmuxinator/minimize_panes.sh` (placeholder: `minimized_placeholder.sh`)
- Pane headers and devws key bindings: end of `config/tmux/tmux.conf`
- Workspace menu: `config/tmuxinator/session_picker.sh`
- Neovim (LazyVim): `config/nvim/`
- tmux: `config/tmux/tmux.conf`
- Shell command and prompt: `shell/devws.zsh`
- Mermaid diagram preview: `shell/render_mermaid_preview.sh`

After changing a symlinked config, restart the relevant program. Reload shell
changes with `source ~/.zshrc`; reload tmux with
`tmux source-file ~/.tmux.conf`.

## Validate

```sh
make check
```

This checks Bash/Zsh syntax, the tmuxinator ERB/YAML template, Lua syntax of
the Neovim config when `luac` is available, and executable permissions, then
runs the test suites in `scripts/`: install/uninstall, Markdown viewer,
workspace save/restore, layout toggle, minimize (including real mouse clicks
on an isolated tmux server), and the iTerm2 key map.

## Uninstall

Run:

```sh
./delete.sh
source ~/.zshrc
```

During installation, devws records its backup state before replacing any
configuration. The uninstall script removes the configuration symlinks
created by this checkout, restores the files that were in their place before
installation, and removes the shell integration from `~/.zshrc`.

The saved workspace state in `~/.devws-state/` is removed, and the iTerm2
Shift+Enter mapping is deleted when devws is the one that added it. Quit iTerm2
before uninstalling so that removal can take effect.

Files that are no longer devws-managed are left untouched. If one conflicts
with a configuration that needs to be restored, the previous configuration
remains in `~/.devws-backups/` and the script reports its location. Only empty
configuration directories are removed.

If devws installed `fzf` after receiving permission or installed Glow through
`./install.sh --deps`, `delete.sh` removes those owned packages. Pre-existing
installations are never removed. Other dependencies installed with `--deps`
are preserved because Homebrew packages and tmux plugin manager may
be shared with other tools or may have existed before devws. Remove those
separately only if they are no longer needed.

The installer and uninstaller never modify Git remotes or GitHub
configuration.
