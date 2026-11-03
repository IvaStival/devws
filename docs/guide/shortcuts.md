# Shortcuts

The tmux prefix is `C-b`: press Ctrl+B, let go, then the key. `C-b Space`
opens the devws menu, which lists every action with its key, so this page
is only needed to look a key up.

## devws

| Keys | Action |
| ---- | ------ |
| `C-b Space` | Open the devws menu (every action, in tabs) |
| `C-b e` | Run this branch in jungleforge ([App](app.md)) |
| `C-b E` | Switch the running branch |
| `C-b j` | Open the latest background job's log, a failure first |
| `C-b g` | Commit, by the workspace's agent ([Git](git.md)) |
| `C-b G` | Push the branch and open or create its pull request |
| `C-b \` | Toggle the layout ([Layout](layout.md)) |
| `C-b _` | Minimize or restore the agent and terminal |
| `C-b C-s` | Hide or show the header's rows |
| `C-b N` | New panel: an agent, a terminal or a Neovim |
| `C-b S-Right` `C-b S-Left` `C-b S-Up` `C-b S-Down` | Move the pane one place that way (Shift+arrow; keep pressing the arrows to go further) |
| `C-b B` | Open the database pane, or focus it |
| `C-b a` | Go to the agent that needs you; again for the next one |
| `C-b F` | Choose how the editor follows the agent |
| `C-b d` | Detach: leave devws, every workspace keeps running |

The mouse works too: click a pane group's header (`▾ Agent`, `▾ Database`)
to minimize or restore it, click the header's rows to hide them, click a
job in the header to open its log, and click a workspace in the sidebar to
go to it.

## In the menu and the pickers

| Keys | Action |
| ---- | ------ |
| `←` `→` or `Tab` `Shift+Tab` | Change tab |
| `↑` `↓` | Move |
| Typing | Search every tab's actions (`push` finds Push / PR) |
| `Enter` | Run the action, or choose |
| `Tab` | In a list of ticked choices (apps, files): untick one |
| `Esc` | Go back one question, or close |

## In the job log panel

| Keys | Action |
| ---- | ------ |
| `q` | Close the panel (the job carries on) |

## tmux's own keys

devws keeps tmux's default keys (`config/tmux/default-keys.conf` resets
them, then devws adds the ones above and below). The ones used most:

| Keys | Action |
| ---- | ------ |
| `C-b \|` or `C-b %` | Split the pane side by side (`C-b \|` opens in the pane's folder) |
| `C-b "` | Split the pane one above the other |
| `C-b ←` `C-b →` `C-b ↑` `C-b ↓` | Go to the pane that way |
| `C-b o` `C-b ;` | Next pane / the pane you were in before |
| `C-b z` | Zoom the pane in and out |
| `C-b x` | Close the pane (asks first) |
| `C-b s` | Choose a session (any tmux session, devws or not) |
| `C-b [` | Scroll back (copy mode): move, `Space` selects, `y` or `Enter` copies to the system clipboard, `q` leaves |
| Mouse drag | Copy to the system clipboard (a click clears the selection) |
| `C-b ]` | Paste |
| `C-b R` | Rename the workspace's session |
| `C-b $` | After re-attaching (over SSH, say): refresh `SSH_AUTH_SOCK` and the like in the shells |
| `C-b C-r` | Reload `~/.tmux.conf` |
| `C-b ?` | Every key tmux has, with what it does |

devws takes `C-b Space` (tmux's next-layout), `C-b E` (spread the panes
evenly), `C-b $` (rename the session, now `C-b R`) and `C-b S-Right`…
(scroll a client smaller than the window) for its own actions.
