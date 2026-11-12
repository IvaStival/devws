# Spec: Complete fresh installs without a separate dependency pass

- **Date:** 11-11-2026
- **Branch:** fix/fresh-install
- **Status:** In progress
- **Authorization:** User requested installing fzf and the --deps dependencies in the normal installer, moving GitHub login to startup or the menu, and resolving missing devdb on a fresh server.

## Requirements

1. `./install.sh` installs the Brewfile dependencies and tmux plugin manager by default, before requiring fzf or changing configuration. Keep `--deps` compatible and offer `--no-deps` for systems where dependencies are managed separately.
2. Install missing devdb through uv from an available local checkout or its public GitHub repository. Respect an explicit `DEVWS_DEVDB_SOURCE`, include the default tool bin directory on PATH, and report an installation failure as a failure rather than claiming completion.
3. Remove interactive GitHub authentication from installation. Add a GitHub login action to the menu's Git tab, usable over SSH. Local workspaces remain available without GitHub authentication.
4. Preserve pre-existing programs and dependency ownership tracking; verify default fzf installation, devdb source choices and failure handling, and menu authentication with isolated tests.
5. Update installation, configuration, and menu documentation; run relevant tests, full checks and whitespace checks.
6. Include the user's subsequent server report: prepare Rust and build its programs during a normal install, fail clearly on build errors, and ensure the board/menu find binaries in `~/.cargo/bin` when tmux has an older PATH. The database pane must likewise find uv's tools in `~/.local/bin`.

## Scope

Use the existing Homebrew-based macOS/Linux installer. Do not add a new package manager or install Homebrew silently. Keep optional agent-memory and application setup separate from required dependencies.

## Files

`install.sh`, `Brewfile`, `config/tmuxinator/agentic.yml`, `config/tmuxinator/devws_menu.sh`, `config/tmuxinator/workspace_actions.sh`, lifecycle/menu/agent-team tests, README and existing user guide pages.
