# Architecture: Team navigation

The team UI has three surfaces with distinct responsibilities. The persistent
fzf sidebar navigates workspaces and agent groups. The popup resolves contextual
actions. The Rust board presents task state and selectable details while its
existing dispatcher continues to deliver work.

## Board data and interaction

`agentic/src/board.rs` builds the board from task state, role definitions, and
devws pane readings. Display rows have stable identities (`agent:<id>`,
`step:<id>`, and question IDs), while full order and repository context lives
in a detail map. Neither opening details nor switching views mutates task state.

`agentic/src/board_view.rs` owns pure view construction, identity-based selection,
and rendering. Refresh reconciles the selected identity, falling back to the
nearest row when it disappears. Small layouts reserve space for attention counts
and a scrollable list instead of substituting a reduced data set.

`agentic/src/board_tui.rs` owns input and external effects. It caches derived rows
between changes, reads activity only when that view is active, and skips reading
an unchanged log using its size and modification time. Usage keeps its existing
30-second cache. The Ratatui rendered-line-info feature supplies exact wrapped
line counts so detail scrolling stops at the content boundary.

## Existing-pane requests

```text
Team popup action
    │ resolve @devws_team_of to the owning workspace
    ▼
Existing pane with @devws_role=team-log
    │ set @devagentics_board_view; focus pane
    ▼
Board refresh loop
    │ consume and clear request; reset view navigation
    ▼
Render selected view (same dispatcher)
```

The `@devagentics_board_view` pane option accepts `overview`, `agents`, `steps`,
`activity`, or `usage`. The popup fails visibly if no board pane exists; it does
not spawn a replacement. Requests are checked on the existing two-second board
refresh cycle. `--view` selects the initial view for interactive and `--once`
commands. Leader navigation locates its role identity rather than assuming an
adjacent pane.

## Sidebar state and search

The picker stores expansion and query state on its own tmux pane because fzf
reload callbacks execute in fresh shell processes. Entering another workspace
opens its current path; manual choices survive subsequent refreshes. Collapsed
rows aggregate working and waiting pane counts.

Each internal record carries a stable key, a visible label, and descendant
search text. Query filtering searches complete records, retaining matching
ancestors. Before records reach fzf, the search field is removed entirely, so
older running pickers cannot expose it. Agent-team entries reserve the first line for name and status, with model
metadata on separate dim lines. Models wrap rather than sharing the name budget.
At narrow widths the tree uses less indentation and compact status icons, and
team display names omit the internal dev- prefix. Stable keys and search retain
full identities. Normal development workspaces retain branch, repository state,
agent, and editor details, followed by dim model lines when known. Classification uses
the agentic template, group ownership, or a board pane rather than task membership
alone, so another repository attached to a task can retain normal details. Search temporarily exposes collapsed descendants.
Clearing it restores saved expansion. fzf tracks record keys through reloads so
insertions do not move selection to unrelated workspaces.

## Model metadata

The sidebar reads `@devws_model` from the shared pane snapshot, falling back to
`@devws_model_configured`. The runner captures an explicit launch model and
clears metadata when a pane is reused. Hooks and the Claude status line record
runtime models through `config/tmuxinator/agent_model.sh`; unknown models are
omitted rather than inferred from a program name.

The header's existing token update backfills models from compact transcript
metadata, with a global 30-second throttle. `header_tokens.sh` reuses the token
cache and exposes model identifiers and observation times without conversation
text. Direct runtime reports take precedence over older transcript evidence;
newer transcript observations and changed transcript pointers can update an
in-session model. Only model-value changes redraw sidebars. Sidebar redraws do
not read transcripts.

## Extending navigation

Add board views through `View`, row construction, and rendering; update the CLI,
Team menu actions, guide, and regression coverage together. Keep navigation
requests separate from team execution controls. Dispatcher lifetime remains
owned by the board: quitting it stops that dispatcher.

| Source | Responsibility |
| --- | --- |
| `agentic/src/board.rs` | Task and pane data with full details |
| `agentic/src/board_view.rs` | Views, selection, pure rendering |
| `agentic/src/board_tui.rs` | Input, caching, navigation effects |
| `config/tmuxinator/session_picker.sh` | Sidebar tree, model labels, and search |
| `config/tmuxinator/agent_model.sh` | Runtime model observations and backfill |
| `config/tmuxinator/devws_menu.sh` | Contextual Team tab |
| `config/tmuxinator/workspace_actions.sh` | Existing-pane targeting |
