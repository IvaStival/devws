# devws

`devws` turns a project folder into a persistent terminal workspace with:

- a full-height workspace switcher and command menu;
- an editor pane (LunarVim, Neovim, Vim, VS Code, or Zed);
- a Claude Code or Codex agent pane;
- a regular terminal pane;
- Git branch and clean/dirty status in the workspace list.

Each project runs in its own tmux session. The menu can create, switch, and
close workspaces; switch or restart the agent; switch the editor; create
terminals; and open the project in Finder, Zed, or VS Code.

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
- Neovim and LunarVim
- `tree`
- Ruby (used by tmuxinator)
- one AI agent: [Claude Code](https://docs.anthropic.com/en/docs/claude-code)
  or [Codex](https://developers.openai.com/codex/)

The included tmux theme uses
`JetBrainsMono Nerd Font Mono`. The Brewfile installs the font on macOS, but
you must select it in your terminal profile.

Optional GUI integrations are Finder (macOS), Zed, and VS Code. The menu hides
no errors if Zed or VS Code is unavailable; those actions simply do nothing.

Vim, Neovim, VS Code, and Zed are optional alternate editors. Only LunarVim is
installed by the dependency installer; picking another editor requires it to
already be on `PATH`.

## Install

Clone the repository, then run:

```sh
./install.sh --deps
source ~/.zshrc
```

`--deps` installs the Brewfile, LunarVim, and tmux plugin manager. If your
dependencies are already installed, use `./install.sh`.

The installer creates symlinks into the clone. Keep the cloned folder in its
original location after installation. Existing managed files are moved to a
timestamped directory under `~/.devws-backups/` before replacement.

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
devws ~/Projects/my-app claude lvim
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

Inside LunarVim, its separate file explorer can be controlled with
`:NvimTreeOpen`, `:NvimTreeClose`, or `:NvimTreeToggle`.

## Menu indicators

- A dark gray background marks the active workspace.
- A lighter opaque gray background marks the selected workspace.
- Yellow `●` means the Git working tree has uncommitted or untracked changes.
- Green `✓` means the Git working tree is clean.

## Configuration

- Agents and editors: `config/tmuxinator/.env`
- Workspace layout: `config/tmuxinator/dev.yml`
- Workspace menu: `config/tmuxinator/session_picker.sh`
- LunarVim: `config/lvim/config.lua`
- tmux: `config/tmux/tmux.conf`
- Shell command and prompt: `shell/devws.zsh`

After changing a symlinked config, restart the relevant program. Reload shell
changes with `source ~/.zshrc`; reload tmux with
`tmux source-file ~/.tmux.conf`.

## Validate

```sh
make check
```

This checks Bash/Zsh syntax, the tmuxinator ERB/YAML template, Lua syntax when
`luac` is available, and executable permissions.

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

Files that are no longer devws-managed are left untouched. If one conflicts
with a configuration that needs to be restored, the previous configuration
remains in `~/.devws-backups/` and the script reports its location. Only empty
configuration directories are removed.

Dependencies installed with `./install.sh --deps` are also preserved because
Homebrew packages, LunarVim, and tmux plugin manager may be shared with other
tools or may have existed before devws. Remove those separately only if they
are no longer needed.

The installer and uninstaller never modify Git remotes or GitHub
configuration.
