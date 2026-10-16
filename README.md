# devws

`devws` turns a project folder into a persistent terminal workspace with:

- a full-height workspace switcher and command menu;
- an editor pane (Neovim with LazyVim, Vim, VS Code, or Zed);
- a Claude Code or Codex agent pane;
- a regular terminal pane;
- two layouts to switch between, and agent/terminal panes that minimize to
  small bars while you code;
- a database pane connected to the project's database from its `.env`;
- Git branch and clean/dirty status in the workspace list.

Each project runs in its own tmux session. The menu can create, switch, and
close workspaces; switch or restart the agent; switch the editor; create
terminals; toggle the layout; open the project's database; initialize project
instructions for the active agent; and open the project in Finder, Zed, or VS
Code.

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
- [devdb](../devdb) for the database pane, installed with
  [uv](https://docs.astral.sh/uv/) from its checkout (`~/Projects/devdb`, or
  `DEVWS_DEVDB_SOURCE`)
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

Control the left sidebar (the list of open workspaces) from any pane:

```sh
devws menu open
devws menu close
devws menu toggle
```

Target another open workspace by folder/session name:

```sh
devws menu open my-app
```

### The devws menu

Every workspace action is in one menu: press **`C-b Space`** in any pane, or
click **☰ Menu** at the top of the sidebar. It opens as a popup with a tab per
group, and shows each action's own shortcut:

| Tab | Actions |
| --- | ------- |
| Workspace | New workspace · Open folder · Toggle layout `C-b \` · Minimize agent and terminal `C-b _` · New terminal · Close workspace |
| Agent | Switch agent · Restart agent · Initialize agent · Register agent · Remove agent · Agent follow `C-b F` |
| Editor | Switch editor · Agent follow `C-b F` |
| Database | Database `C-b B` · Database folder |

`←`/`→` or `Tab` change tabs, `↑`/`↓` move, `Enter` runs the action and `Esc`
closes; typing filters the tab. Clicking a tab or an action works too. The
menu reopens on the tab you used last in that workspace. Actions that ask
something (which agent, which folder…) ask inside the popup.

### Layouts

Each workspace can switch between two arrangements at any time. Choose
**▥ Toggle layout** in the devws menu or press `C-b \`:

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
- An agent started by hand in the terminal works too: without an agent pane
  the terminal leads, and shells split off by hand (panes with no devws role)
  go along as extra terminals. Toggling the layout and opening the database
  follow the same rule.
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

devws also makes Neovim the default text editor for tools that open one (such
as `crontab -e` or the tmux config shortcut) by setting `EDITOR` and `VISUAL`
to `nvim`, unless you set them yourself before `devws.zsh` is sourced. Git
uses its own `core.editor` setting when one is configured.

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

### Following the agent in the editor

The agent and Neovim are linked:

- **The editor follows the agent's edits.** Each time the agent changes a file,
  the editor opens it at the change and flashes the changed line. Your focus
  stays in the agent pane. While you are typing in the editor (insert, replace,
  command or terminal mode), or when the changed file has unsaved changes of
  yours, it doesn't jump: a notification names the file and line instead.
  Unsaved changes to other files don't stop it. When edits come quickly, the
  editor always ends on the latest one.
- **The agent can show you things.** Ask it to "show me the login controller"
  or "open where the total is computed", and it opens that place in the
  editor. It runs `~/.tmuxinator/editor_open.sh <path>[:<line>]`, which you
  can also run yourself from the agent or terminal pane.
- **Choose how it follows** with `C-b F` or **◎ Agent follow** in the
  devws menu. The menu marks the current mode, and the agent's header
  names it:

  | Mode | What the editor does | Header |
  | ---- | -------------------- | ------ |
  | Follow edits (default) | Opens each changed file at the change | `· following` |
  | Notify only | Stays put; a notification names each change | `· notify` |
  | Follow edits and focus the editor | Follows, and moves the focus to the editor pane | `· following + focus` |
  | Follow edits and reads | Also opens each file the agent reads (Claude Code; Codex reads through shell commands, which aren't seen) | `· following reads` |
  | Off | Never moves by itself | — |

  In every mode the agent can still open files when you ask.

How it works: Neovim listens on a socket (`/tmp/devws-nvim-*.sock`), and devws
starts the agent with an edit hook and a short instruction about
`editor_open.sh`, passed on its command line, so your own Claude Code and
Codex settings are not changed. Claude Code follows every edit and may run
`editor_open.sh` without asking. **Codex asks once to review the new hook**
(it shows "Hooks need review"); approve it, and it follows from then on.
Only Neovim is linked; with another editor the agent just names the file.

### Agent status

devws shows what the agent is doing, so you can tell at a glance whether it
still works, needs you, or is done, even while it is minimized or you are in
another workspace:

| State | Shown as | When |
| ----- | -------- | ---- |
| Working | `⠋` (animated) | From your prompt until the agent's turn ends |
| Needs you | yellow `● needs you` | A permission prompt or a question is waiting for you |
| Done | green `✓ done` | The agent finished its turn; stays until your next prompt |
| Idle | nothing | The agent just started, or was restarted |

It appears in the agent's header, where the spinner replaces the agent icon
(`⠋ Agent · claude · following`) and the other states follow the name
(`Agent · claude ● needs you`), as the icon alone in the minimized agent strip
(`▸ Agent ⠋`) and in the status bar (`[project] ⠋`), and in the sidebar under
each workspace (`claude ⠋ · nvim`). There are no alerts or sounds.

How it works: the same hooks that link the editor report each agent event to
`agent_state.sh`, which records the state on the workspace (tmux option
`@devws_agent_state`). Switching or restarting the agent resets it. Codex asks
once to review the new hooks, as for following.

### The header

The status bar at the top is tmux's own line (workspace, windows, the current
conversation, the clock), a blank line, then three rows of groups, two spaces
each side of the `│` between them (five lines, the most tmux allows):

```
 [project] 1:workspace                                          conv 1.9M · ctx 115k   19:25 Thu 15 Oct

 ▀█  ▀▀█ ▄ ▀▀█ ▀▀█  │  CPU ▓▓▓░░░░░░░  27%  ▂▃▅▂▁    │  claude 5h ▓▓▓▓░░░░░░  42% wk ▓▓▓▓▓▓░░░░  63% $552  │  ↓ 2.4 MB/s  ↑ 120 kB/s     │  +18°C  Light rain   ▗▟██▙▖
  █  █▀▀ ▄ ▀▀█ █▀▀  │  MEM ▓▓▓▓▓▓▓▓░░  80%  6.4/8.0G │  work   5h ░░░░░░░░░░   2% wk ░░░░░░░░░░   1% $36   │  Wi-Fi ▂▄▆_ -60 dBm · 5 GHz │  Curitiba            ▐▗██▗▌
 ▀▀▀ ▀▀▀   ▀▀▀ ▀▀▀  │  DSK ▓▓▓▓▓▓▓▓▓▓  98%  BAT 64%  │  month $588 at API prices                         │  192.168.1.17 · ping 7 ms   │  humidity 94% · wind ↓6km/h  ▐▜▜▛▛▌
```

- **Clock:** the time, `HH:MM`, in block digits three rows tall
  (`header_clock.sh`). It is left out, not cut, when the other groups need its
  room.
- **System:** CPU with its last minute of history, memory, disk, battery (when
  there is one) and load. The bars are drawn like Claude Code's status line:
  green, yellow from 60%, red from 85%.
- **The agents**, one row each for the agents in `DEVWS_AGENTS` (see
  [Agents and accounts](#agents-and-accounts)), in that order, leaving out
  any whose folder doesn't exist; with none to show, it says so and points to
  **☰ Menu › Agent › Register**. For each: how much of
  the plan's limits it has used — the 5-hour session (`5h`) and the week (`wk`)
  — and what this month's tokens would cost on the API, then the month's
  total. Codex logs its limits; Claude Code passes them only to a status line,
  so devws starts Claude Code with its own (`agent_statusline.sh`), which
  records them per agent and then prints your status line from the account's
  `settings.json` unchanged. Until a Claude agent started that way has drawn
  its status line, its bars are empty (`—`). The cost prices each response in
  each agent's own logs (`<folder>/projects`, `<folder>/sessions`) at the rates in
  `config/tmuxinator/agent_prices.tsv` — update them there when they change.
  Plans aren't billed per token, so it is an indication; a model with no price
  is left out and marks the cost `+`. Worked out every 10 minutes.
- **Network:** download and upload speed of the default interface, the Wi-Fi
  signal (macOS hides the network's name from apps without Location access),
  the local address and a ping to 1.1.1.1 once a minute.
- **Weather** from [wttr.in](https://wttr.in), located by your IP every 30
  minutes; set `DEVWS_WEATHER_LOCATION` in `config/tmuxinator/.env` to pick a
  place.
- **The ghost**, pixel art in quarter blocks (body, white eyes, pupils),
  shows all the workspaces' agents together, the same in every workspace:
  yellow, wide-eyed and gasping with a `!` when any agent needs you; else
  orange, drifting and looking where it goes while any works, blinking now and
  then; else green and smiling with a heart when one is done; else grey,
  asleep and breathing. Set `DEVWS_HEADER_PET=0` to hide it.
- **The conversation** of this workspace's agent, on tmux's own line: its
  tokens and context (in tokens for Claude Code, as a share of the window for
  Codex).

Each group is as wide as its content; narrow terminals cut the widest first,
and under 110 columns the ghost goes. One background loop (`header_ticker.sh`)
collects everything, lays the rows out at each client's width
(`header_layout.sh`) once a second and restarts whenever tmux.conf is reloaded;
the CPU, memory, disk and battery readings are macOS's. The ghost moves 20
times a second from frames drawn once at start; that costs about 10% of a core
(tmux redrawing, the ticker and its tmux calls), which `DEVWS_HEADER_PET=0`
saves.

The header hides like the panes: click its rows (or the `▾` at the end of
tmux's line), press `C-b C-s`, or choose **▔ Hide or show the header** in the
devws menu, and only tmux's own line stays, with a `▸` to click to bring the
rows back. It stays hidden across tmux.conf reloads, and the ticker rests
meanwhile. Clicks elsewhere on the status bar still choose windows.

### Database pane

Choose **⛁ Database** in the devws menu, or press `C-b B`, to open
**devdb** beside the editor, connected to the project's database. devdb is a
DataGrip-style client for SQLite, MySQL, PostgreSQL and MongoDB that lets you
edit data in place. It goes where there is room for it:

```
default layout                    full-height editor
┌──┬─────────┬──────────┐         ┌──┬─────────────┬───────┐
│m │ editor  │ database │         │m │   editor    │ agent │
│e ├─────────┴──┬───────┤         │e ├─────────────┼───────┤
│n │   agent    │ term  │         │n │  database   │ term  │
└──┴────────────┴───────┘         └──┴─────────────┴───────┘
```

Running it again focuses the open pane. devdb lists the tables (or MongoDB
collections) on the left and shows the first one's rows straight away, in the
same Tokyo Night colours as LazyVim:

- `Enter` (or clicking the selected cell) edits a cell, `+` adds a row, `d`
  marks a row for deletion; changes stay highlighted until **`Ctrl+S`**
  submits them all together, or **`Ctrl+R`** reverts them.
- `/` filters (a SQL `WHERE` condition, or a JSON query for MongoDB),
  clicking a header sorts, `[` / `]` page, `v` shows a long value in full.
- The **Query** tab runs SQL, or a MongoDB command as JSON, with `Ctrl+J`.
- `F2` goes back to the connections: set up a new one, or open another saved
  one. `Ctrl+O` opens another database on the same server.
- `Ctrl+Q` quits and closes the pane, asking first when changes are pending.
- **`F1`** (or `?`) shows all the shortcuts; the footer only fits the first
  few in a narrow pane.

See devdb's README for everything it does.

The connection comes from a `.env` file, read when the pane opens. devws uses
the project root's `.env` when it has database settings, otherwise one in a
direct subfolder (such as `backend/.env`, skipping `node_modules` and
`vendor`), and asks when several qualify. To choose the folder yourself, use
**⛁ Database folder** in the devws menu: it lists every folder with a
`.env` (up to three levels down, database settings first) plus **Automatic**.
The choice is remembered per project, and an open database pane reconnects to
it straight away.

The settings read from that `.env`:

- a connection URL in `DATABASE_URL`, `DB_URL`, `MONGODB_URI`, `MONGODB_URL`
  or `MONGO_URL`, when set;
- otherwise Laravel's `DB_CONNECTION` (`mysql`, `mariadb`, `pgsql`, `sqlite`,
  or `mongodb` from Laravel's MongoDB package) with `DB_HOST` (default
  `127.0.0.1`), `DB_PORT` (default 3306 / 5432 / 27017), `DB_DATABASE`,
  `DB_USERNAME` and `DB_PASSWORD`; for MongoDB also
  `DB_AUTHENTICATION_DATABASE`.

SQLite works from `DB_CONNECTION=sqlite` (the file in `DB_DATABASE`, or
`database/database.sqlite`) and from `sqlite:///absolute/path`,
`sqlite://relative/path` or `file:` URLs; relative paths are resolved in the
project. If the file does not exist the pane says so instead of letting
devdb create an empty database there.

Without database settings the pane opens devdb's **connection screen**: choose
SQLite, MySQL / MariaDB, PostgreSQL or MongoDB, fill in the file or the host,
port, database, user and password, **Test connection**, then **Connect**. devdb
saves it for the project (`~/.config/devdb/connections.json`, password in the
macOS Keychain), so the pane opens it directly next time; the project's `.env`
is never changed. **⛁ Database folder** still picks a `.env` in a subfolder. The connection, password included, is handed to devdb in the
`DEVDB_URL` environment variable, so it never appears in the process list or
on disk. Hosts that only exist inside Docker, such as a Sail service called
`mysql`, are not reachable from the pane; point `DB_HOST` at a published port
instead.

The database pane has its own header (`▾ 󰆼 Database · mysql`). Clicking it
hides the database to a bar (`▸ DB`) and back, independently of the agent and
terminal. Toggling the layout moves it to its place in the other layout, and
it is reopened when a saved workspace is restored.

### tmux shortcuts added by devws

The tmux prefix is `C-b` (tmux's default key set is restored by
`config/tmux/default-keys.conf`).

| Keys | Action |
| ---- | ------ |
| `C-b Space` | Open the devws menu (every workspace action, in tabs) |
| `C-b \` | Toggle the workspace layout |
| `C-b _` | Minimize or restore the agent and terminal |
| `C-b B` | Open the database pane, or focus it |
| `C-b F` | Choose how the editor follows the agent (menu) |
| Click a header | Minimize or restore that pane group (agent and terminal, or database) |

### Session persistence

Open workspaces are saved automatically, so a reboot or an iTerm2/tmux crash
does not cost you the arrangement. Saved per workspace:

- the workspace folder, agent, and editor
- the pane layout (including the full-height editor layout) and each pane's
  working directory
- extra terminals created from the devws menu
- whether the database pane was open
- whether the sidebar was open or closed

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

### Agents and accounts

The agents devws offers are listed in `DEVWS_AGENTS` in
`config/tmuxinator/.env`, and each one is a program in a config folder. That
way one program can run several accounts, such as a personal Claude in
`~/.claude` and a company one in `~/.claude-work`:

```sh
DEVWS_AGENTS="claude work"
DEVWS_AGENT_work="claude ~/.claude-work"
```

`claude` and `codex` need no line of their own; they run in `~/.claude` and
`~/.codex`. Choose **＋ Register agent** in the devws menu's Agent tab to add
one. It asks for the program, offers the `~/.claude*` or `~/.codex*` folders
not registered yet (or takes a typed path) and suggests a name (`work` for
`~/.claude-work`). **− Remove agent** takes one off the list, and its folder is
kept. Both edit `.env` and update the header at once. Log the account in once
in its folder first, for example `CLAUDE_CONFIG_DIR=~/.claude-work claude`.

An agent in its own folder starts with `CLAUDE_CONFIG_DIR` (or `CODEX_HOME`)
set to it. One in the default folder starts with neither, as the plain command
would, so it keeps the login it has. Start a workspace with an account by
name, for example `devws ~/Projects/my-app work`, or switch to it with **Switch
agent**.

### Initialize an agent

Choose **Initialize agent** in the devws menu to seed the current project
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
- Database pane (devdb): `config/tmuxinator/open_database.sh`,
  `database_runner.sh`, and `.env` parsing in `database_url.sh`
- Sidebar: `config/tmuxinator/session_picker.sh`
- devws menu: `config/tmuxinator/devws_menu.sh` (tabs and items), with the
  actions in `config/tmuxinator/workspace_actions.sh`
- Agent ↔ editor link: `config/tmuxinator/editor_open.sh`,
  `agent_follow_hook.sh`, `agent_settings.json` (Claude Code hook),
  `agent_editor_prompt.md`, `follow_mode.sh` (the modes), and
  `config/nvim/lua/devws/follow.lua`; wired up in `agent_runner.sh` and
  `editor_runner.sh`
- Agent status: `config/tmuxinator/agent_state.sh` (hook: event → state),
  `agent_status.sh` (records it, runs the spinner), the formats
  `@devws_agent_icon` and `@devws_agent_badge` in `config/tmux/tmux.conf`;
  wired up in `agent_runner.sh` and `agent_settings.json`
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
workspace save/restore, layout toggle, minimize and the database pane
(including real mouse clicks on an isolated tmux server), and the iTerm2 key
map.

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
