# JungleForge menu and network device discovery

- **Date** 10-11-2026
- **Branch** `feat/menu-network-devices`
- **Status** Executed

## Context

The App tab groups JungleForge deployment controls with the standalone database client and background job log. Network information in the header describes the connection. Users need a command that lists devices discoverable on the local network.

## Menu

| Tab | Actions |
| --- | --- |
| Workspace | Open or create workspaces and agent teams, open a folder, close or detach |
| Layout | Arrange panels and switch editors |
| Agent | Manage agents, configure team roles before launch, and use memory |
| Team | Navigate the current team's views and manage its leader. Available in team workspaces and their groups |
| Git | Commit, push or open a PR, and tidy merged branches |
| JungleForge | Set up JungleForge, register apps, run or switch branches, stop apps, start services, migrate, and open the app shell, logs, info, and settings |
| Tools | Open the database client, choose its database folder, open the job log, and list network devices |
| Help | Open the guide and command reference |

The App tab becomes JungleForge. A workspace that last selected App opens the menu on JungleForge. Existing shortcuts and action IDs continue to work. JungleForge setup is available before configuration, and its other actions appear once JungleForge is available. Database actions require an editor workspace. Search, keyboard navigation, and header clicks cover the same tabs in the Rust and fzf menus.

## Network discovery

Run `devws network devices` from any terminal or select **Tools → List network devices** in the menu. Both entry points use the same discovery command. Menu results stay readable until dismissed.

- On macOS and Linux, derive the connected IPv4 subnet from the default-route interface and its address and prefix. Report the selected interface and subnet. If the route or subnet cannot be determined, report the problem without starting discovery.
- Use [Nmap host discovery](https://nmap.org/book/man-host-discovery.html) with `-sn`. List discovered IP addresses and show names or MAC addresses when Nmap provides them. Sleeping, isolated, and nonresponding devices can be absent.
- Nmap is an optional dependency. If it is missing, explain how to install it. Run discovery on demand using ARP on the selected interface, requesting administrator access for Nmap through sudo when required.
- `devws network --help` shows usage. Invalid arguments produce a usage error, and discovery failures are visible to the caller.

## Files involved

| Area | Changes |
| --- | --- |
| `config/tmuxinator/devws_menu.sh` | Shared tab membership and naming, remembered selection, and room for the tab bar |
| `bin/devws`, `config/tmuxinator/network.sh`, `config/tmuxinator/workspace_actions.sh` | CLI dispatch, discovery, and menu action |
| `scripts/test_devws_menu.sh`, `scripts/test_network.sh`, `scripts/check.sh` | Menu and command regression coverage |
| `docs/guide/`, `install.sh` | Update menu references and document discovery |

## Acceptance criteria

- [x] JungleForge actions are in JungleForge, independent utilities are in Tools, and branch cleanup is in Git. Context-sensitive actions and keyboard shortcuts work as before.
- [x] Both menu renderers show JungleForge, support keyboard and mouse selection, search across tabs, and recognize the saved App selection.
- [x] Fixture-based command tests cover macOS and Linux subnet detection, including a prefix other than `/24`, the resulting host discovery invocation, usage errors, missing route or dependency, and scan failures.
- [x] The menu invokes the same discovery command and displays its result or error. Automated checks use fixtures and make no live network probes.
