# Layout

The menu's **Layout** tab arranges the screen. Nothing it does restarts a
program: panes move, and the editor, agent and shells keep running.

## The two layouts

**▥ Toggle layout** (`C-b \`) switches between:

```
default                       full-height editor
┌──┬───────────────┐          ┌──┬─────────┬───────┐
│s │    editor     │          │s │         │ agent │
│i ├───────┬───────┤    ⇄     │i │ editor  ├───────┤
│d │ agent │ term  │          │d │         │ term  │
└──┴───────┴───────┘          └──┴─────────┴───────┘
```

Focus stays where it was, extra terminals move with the terminal, and the
layout is saved with the workspace.

## Minimize the agent and terminal

**▁ Minimize agent and terminal** (`C-b _`), or a click on the `▾ Agent` or
`▾ Terminal` header, shrinks both so the editor gets the room: to one line
in the default layout, to a narrow strip on the right in the other. Again
brings them back at their size.

- They keep running in a hidden window; the strip still shows the agent's
  state (`▸ Agent ⠋`).
- Focus moves to the editor if it was on them.
- Toggling the layout, and the menu's agent actions, bring them back first.
- Dragging a header still resizes; only a click toggles.

## New panels

**▣ New panel** (`C-b N`) opens another **Agent** (any registered agent: a
second account, or a second Claude on another task), a **Terminal** or a
**Neovim**, **Right**, **Left**, **Below** or **Above** the pane you are in.

- A panel next to the agent or terminal minimizes and moves with them; one
  next to the editor stays where it is.
- An extra agent starts like the main one (its account, devws's hooks) and
  has its own state in its header. The workspace's state sums all its agents
  up; **Go to waiting agent** finds the one waiting.
- An extra Neovim is on its own: the agent keeps opening files in the main
  editor.
- After a restore an extra agent comes back in a new conversation
  (`/resume` brings the old one back), a terminal at a prompt in its folder.

An [agent team](agentic.md) has no editor panel: Toggle layout, Minimize and
Switch editor are not offered there, and New panel opens beside the focused
pane without expanding anything.

## Move panel

**⇆ Move panel** (`C-b S-Right`, `C-b S-Left`, `C-b S-Up`, `C-b S-Down`)
swaps the pane you are in with its neighbour on that side. Each place keeps
its size; keep pressing the arrows after one `C-b` to go further. The
sidebar stays put, and minimized panes are expanded first.

## The header

**▔ Hide or show the header** (`C-b C-s`, or a click on its rows) hides the
three rows under tmux's own line; a `▸` on that line brings them back.

```
 [project] 1:workspace                                  conv 1.9M · ctx 115k   19:25 Thu 15 Oct

 ▀█  ▀▀█ ▄ ▀▀█ ▀▀█  │  CPU ▓▓▓░░░  27%  │  claude 5h ▓▓▓▓░░ 42% wk ▓▓▓▓▓▓ 63% $552  │  ↓ 2.4 MB/s   │  +18°C  Light rain
  █  █▀▀ ▄ ▀▀█ █▀▀  │  MEM ▓▓▓▓▓▓  80%  │  work   5h ░░░░░░  2% wk ░░░░░░  1% $36   │  Wi-Fi -60 dBm │  Curitiba
 ▀▀▀ ▀▀▀   ▀▀▀ ▀▀▀  │  DSK ▓▓▓▓▓▓  98%  │  month $588 at API prices  mem ●          │  ping 7 ms     │  humidity 94%
```

- **Clock** in block digits (left out when the others need its room).
- **System:** CPU with its last minute, memory, disk, battery, load: green,
  yellow from 60%, red from 85%.
- **Agents:** one row per agent in `DEVWS_AGENTS`: how much of its plan's
  5-hour (`5h`) and weekly (`wk`) limits it used, and what this month's
  tokens would cost at API prices (an indication: plans are not billed per
  token), then the month's total. With ai-memory, `mem ●` ends the column:
  green while its service answers, red when not.
- **Network:** speed, Wi-Fi signal, local address, ping.
- **Weather** from wttr.in (`DEVWS_WEATHER_LOCATION`).
- **The ghost** shows every workspace's agents together: yellow with `!`
  when one needs you, orange and drifting while one works, green with a
  heart when one is done, grey and asleep otherwise. `DEVWS_HEADER_PET=0`
  hides it.
- **Background jobs** take the network and weather's place while they run
  ([App](app.md#background-jobs)).
- On tmux's own line: this workspace's conversation (tokens and context).

Narrow terminals cut the widest group first; under 110 columns the ghost
goes.

## The editor

**⇄ Switch editor** restarts the editor pane with another editor from
`DEVWS_EDITORS` (Neovim, Vim, VS Code, Zed); the workspace remembers it.

The default is Neovim with LazyVim (`config/nvim/`). `Space` is the leader:
press it and wait to see every key. devws adds:

- uncommitted changes highlighted in dark green (`Space g v` turns it off);
- files the agent changes reload by themselves;
- `Space m p` previews Markdown in place;
- PHP with intelephense, and phpactor for go to implementation.

| Keys | Action |
| ---- | ------ |
| `Space e` | File explorer |
| `Space f f` | Find a file |
| `Space /` | Search the project |
| `gd` `gr` `gI` | Definition, references, implementation |
| `K` | Documentation of the symbol |
| `Ctrl-o` `Ctrl-i` | Jump back, forward |

devws sets `EDITOR` and `VISUAL` to `nvim` unless you set them.

## Following the agent

The agent and Neovim are linked. **◎ Agent follow** (`C-b F`) chooses how:

| Mode | What the editor does | Agent header |
| ---- | -------------------- | ------------ |
| Follow edits (default) | Opens each file the agent changes, at the change | `· following` |
| Notify only | Stays put; a notification names each change | `· notify` |
| Follow edits and focus the editor | Follows, and moves the focus to the editor | `· following + focus` |
| Follow edits and reads | Also opens what the agent reads (Claude Code) | `· following reads` |
| Off | Never moves by itself | — |

- While you type in the editor, or the file has unsaved changes of yours, it
  does not jump: a notification names the file and line.
- Every line the agent changed is marked `▍` until your next prompt (in a
  git project; your staging is never touched).
- Ask the agent to "show me the login controller": it opens it with
  `editor_open.sh`, which you can run too.
- Codex asks once to review devws's hooks ("Hooks need review"): approve it.
  Only Neovim is linked.
