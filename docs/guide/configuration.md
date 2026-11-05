# Configuration

devws reads its settings from `config/tmuxinator/.env` in the checkout
(linked as `~/.tmuxinator/.env`). Edit it, then start a new workspace or
run `devws reload`; the menu's **Register agent** and **Remove agent** edit
the agent lines for you.

## Settings

| Setting | Default | What it does |
| ------- | ------- | ------------ |
| `DEVWS_AGENTS` | `claude codex` | The agents devws offers, in the header's order. Each name is a program in a folder ([Agents](agents.md)) |
| `DEVWS_AGENT_<name>` | — | `"<claude or codex> <folder>"` for an agent that is not plain `claude` or `codex`, e.g. `DEVWS_AGENT_work="claude ~/.claude-work"` |
| `DEVWS_DEFAULT_AGENT` | the first agent | The agent when none is chosen |
| `DEVWS_EDITORS` | `nvim vim code zed` | The editors devws offers |
| `DEVWS_DEFAULT_EDITOR` | the first editor | The editor when none is chosen |
| `DEVWS_HOME` | `~/.devws` | Where GitHub workspaces live: a hidden clone per repo and a folder per branch ([Workspaces](workspaces.md)) |
| `DEVWS_PROJECTS` | `~/Projects` | Where your own checkouts are: a repo's first GitHub workspace offers to import their untracked files (`.env`…) |
| `DEVWS_CLONE_BASE` | GitHub, as gh is set up | The address repos are cloned from, for GitHub Enterprise |
| `DEVWS_JUNGLEFORGE` | `~/.devws/jungleforge`, else `~/Projects/jungleforge` | The jungleforge checkout the App tab runs branches with ([App](app.md)); unset, the first of those that exists |
| `DEVWS_AGENTIC_COMMAND` | `devagentics` | The program that runs an [agent team](agentic.md): `devws agentic` asks it for the task and its log |
| `DEVWS_TEAM_GROUP_SIZE` | `4` | How many agents an [agent team](agentic.md)'s group workspace holds before the next one opens (`Floor 2`) |
| `DEVWS_JUNGLEFORGE_REPO` | `https://github.com/IvaStival/jungleForge.git` | Where **Set up jungleforge** clones jungleforge from (a fork of your own) |
| `DEVWS_AUTO_RESTORE` | `1` | `0`: the first `devws` after a restart does not reopen the saved workspaces (`devws restore` still does) |
| `DEVWS_SUSPEND_AFTER` | `15` | Minutes a workspace stays unseen, its agent idle or done, before it is paused; `0` never pauses |
| `DEVWS_NOTIFY` | `1` | macOS notifications: `1` all, `waiting` only agents that need you, `0` none (agents and jobs) |
| `DEVWS_JOB_NOTIFY_AFTER` | `30` | Seconds a job must run before its success is notified (a failure always is) |
| `DEVWS_JOB_PANEL_AFTER` | `3` | Seconds a job runs before its log panel opens |
| `DEVWS_JOB_PANEL_LINGER` | `5` | Seconds the log panel stays after a success (a failure stays until `q`) |
| `DEVWS_WEATHER_LOCATION` | your IP's | The header's weather place, e.g. `"Lisbon"` |
| `DEVWS_HEADER_PET` | `1` | `0` hides the header's ghost (and saves its CPU) |

Two more are read by `./install.sh` and the checks, not by `.env`:

| Variable | Default | What it does |
| -------- | ------- | ------------ |
| `DEVWS_DEVDB_SOURCE` | `~/Projects/devdb` | The devdb checkout the installer installs the database pane from |
| `DEVWS_CHECK_JOBS` | `4` | How many test suites `scripts/check.sh` runs at once |

## Where devws keeps things

| Path | What |
| ---- | ---- |
| `~/.devws/` | GitHub workspaces (`DEVWS_HOME`): `<owner>/<repo>/.bare`, a folder per branch, `.shared/`; `jungleforge/` when devws set it up |
| `~/.devws-state/sessions.tsv` | The open workspaces, to restore after a restart |
| `~/.devws-state/recent.tsv` | Folders opened before, with their agent and editor (the start screen) |
| `~/.devws-state/jobs/` | Background jobs and their logs, kept 7 days |
| `~/.devws-backups/` | What the installer replaced, to put back on uninstall |
| `~/.tmuxinator/` | Links to the scripts in `config/tmuxinator/`, made on every `devws` run |
| `~/.local/bin/devws` | The `devws` command, a link to `bin/devws` |

## Where the code is

| Part | Files |
| ---- | ----- |
| The command | `bin/devws`; prompt and `EDITOR` in `shell/devws.zsh` |
| Workspace layout | `config/tmuxinator/dev.yml` |
| Menu | `devws_menu.sh` (tabs and items), `workspace_actions.sh` (what they do), `picker.sh` (the list look) |
| Sidebar | `session_picker.sh` |
| Header | `header_ticker.sh`, `header_layout.sh`, `header_*.sh` |
| GitHub workspaces | `github_workspace.sh`, `github_repo.sh`, `worktree.sh`, `shared_files.sh`, `recent.sh`, `tidy.sh` |
| Commit and push | `git_actions.sh` |
| jungleforge | `jungleforge.sh` |
| Background jobs | `job.sh`, `job_panel.sh` |
| Agents | `agent_runner.sh`, `agents.sh`, `register_agent.sh`, `agent_state.sh`, `agent_status.sh`, `jump_agent.sh` |
| Editor link | `editor_open.sh`, `agent_follow_hook.sh`, `follow_mode.sh`, `config/nvim/lua/devws/follow.lua` |
| ai-memory | `memory.sh` |
| Database pane | `open_database.sh`, `database_runner.sh`, `database_url.sh` |
| Restore | `save_state.sh`, `restore_state.sh`; pausing: `suspend_workspace.sh` |
| Keys and pane headers | the end of `config/tmux/tmux.conf` |

The `config/tmuxinator/` files are in the checkout; each is linked into
`~/.tmuxinator`.
