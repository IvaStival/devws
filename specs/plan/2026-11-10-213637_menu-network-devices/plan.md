# Implement the JungleForge menu and network device discovery

- **Date** 10-11-2026
- **Branch** `feat/menu-network-devices`
- **Status** Executed

## 1. Reorganize the menu

- Update `scripts/test_devws_menu.sh` to cover the new tab order and membership in normal, team, and group workspaces. Cover setup and configured JungleForge, editor-only database actions, tab clicks, keyboard navigation, search, and a saved App selection.
- Update the shared action table and tab order in `config/tmuxinator/devws_menu.sh`. Keep action IDs, shortcuts, and visibility predicates. Use the existing capitalization behavior in both renderers to display JungleForge and normalize a saved App selection before passing it to either renderer.
- Size the main popup to fit the full tab bar, including Team. Check the rows passed to the Rust picker and the fzf header and click coordinates.

## 2. Add the network command

- Add `scripts/test_network.sh` with a temporary home and command fixtures. Exercise the public `bin/devws` entry point and assert the Nmap arguments. Cover macOS and Linux, non-default subnet lengths, unavailable or unusable IPv4 routes, missing Nmap, help, invalid arguments, and Nmap failures.
- Add the CLI dispatch and executable `config/tmuxinator/network.sh` using the existing script linking mechanism. On macOS, read the default interface from `route` and its IPv4 address and mask from `ifconfig`. On Linux, read the default interface and its IPv4 address and prefix using `ip`.
- Validate the IPv4 address and mask and derive the network address using the existing Ruby dependency. Reject missing or unusable subnet information before invoking Nmap. Print the interface and subnet, then stream the output of `nmap -sn` and propagate its exit status.
- Include the fixture suite in `scripts/check.sh --quick`.

## 3. Connect the menu and update the guide

- Add **Tools → List network devices** through `workspace_actions.sh`. Reuse the discovery script and keep its output or error visible using the existing popup pause behavior. Extend the menu integration test to assert that this action invokes discovery.
- Update the App guide, command reference, configuration guide, and install messages to reflect the new tab names and where to find each action. Document the optional Nmap dependency and what discovery can report.

## Final verification

1. Run the network and menu suites directly.
2. Run `make check` for shell syntax, Rust formatting, Clippy, and the full test suite. Investigate and report any failures.
3. Mark the spec and plan complete once the acceptance criteria pass.

## Verification results

- `bash scripts/test_network.sh` passed.
- `bash scripts/test_devws_menu.sh` passed.
- `bash scripts/test_guide.sh` passed.
- `make check` passed, including Rust formatting, Clippy, both crates' tests, and all shell suites. Its built-in retries passed the menu suite after an overlapping test-file edit and the guide suite after correcting a line-wrapped action label.
- `git diff --check` passed.
- Device discovery used fixtures. The actual macOS default-route lookup and interface netmask format were also checked.

## Follow-up for incomplete discovery

The user's live scan found two hosts. The original unprivileged Nmap command uses TCP connection probes, which can miss local devices that ignore those ports. Discovery now explicitly uses ARP on the selected interface and requests administrator access for Nmap. The menu runs discovery before starting its pager so the password prompt has terminal access.

A fixture that returns a local device only to ARP discovery reproduced the omission before the fix and passed afterward. Coverage includes root execution and denied or unavailable sudo. A live administrator scan was attempted, but sudo required the user's password. Live device coverage still needs confirmation through the updated menu.

Follow-up verification passed with `bash scripts/test_network.sh`, `bash scripts/test_devws_menu.sh`, `bash scripts/check.sh --quick`, and `git diff --check`.
