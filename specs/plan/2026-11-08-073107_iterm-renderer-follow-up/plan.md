# Plan: Reduce iTerm2 rendering cost without restarting agents

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Solved on 09-11-2026 by switching the terminal to Ghostty; Steps 2–5
  were overtaken by that switch (see [measurements](measurements.md#resolution))
- **Specification:** [spec.md](spec.md), approved by the user
- **Previous measurements:** [devws performance report](../2026-11-08-000752_multi-agent-performance/measurements.md)

## Approach and decisions

Compare three conditions in order: the existing client, a fresh client on the
installed version, and a fresh client on an updated version if still needed.
A fresh client can change both terminal state and negotiated capabilities; treat
those together and avoid attributing that improvement to an upgrade.

Keep running agents in their current tmux panes. Do not invoke workspace restore,
kill the tmux server, respawn panes, clear histories, or reinstall key mappings.
The previous devws code changes remain active throughout the comparison.

An iTerm2 quit/relaunch occurs only in Step 4, after the state-preservation and
backup checks pass. A successful fresh-client comparison may make that step
unnecessary. Old and new iTerm2 builds must not run concurrently against the
same preferences domain.

## Step 1 — Capture a current baseline and preservation inventory

**Status:** Baseline and private preferences backup complete; GUI state unverified

**Resources:** Existing tmux server, iTerm2, private backup directory, and
`scripts/measure_load.sh`.

**Actions:**
- Record the installed and running app versions, macOS version/architecture,
  iTerm2 PID, tmux server PID/socket, session IDs, pane IDs/PIDs, agent descendants,
  current client, selected window/pane, focus, and terminal rows/columns.
- Inspect live `destroy-unattached`, `exit-unattached`, and related server/session
  survival settings before detaching any client. If a last-client gap could end
  a session/server, retain a temporary control client without altering the
  measured visible-client count. Record its identity and remove only that client
  after the final working client is attached. Match dimensions to avoid resizing
  existing panes. Do not silently change persistent survival settings.
- Inventory non-tmux terminal jobs and terminal-local notes, scrollback, saved
  windows, and unsaved state. Keep the original tab/window available. Determine
  the supported way to preserve terminal-local state before any app restart.
- Store personal backups under a private directory outside Git, such as
  `~/.devws-state/terminal-upgrade/<timestamp>/` with `0700` directory permissions.
  Use task-specific variables such as `ITERM_BACKUP_ROOT`; never repurpose `HOME`.
  Reports in this spec folder contain metrics and findings only.
- Collect at least three ten-second CPU/query samples while the issue is present.
  Keep focus, visible workload, dimensions, and header settings fixed. Use the
  existing profiler rather than creating another measurement implementation.
- Capture a short stack sample if current CPU is still elevated. Restrict the
  report to relevant stack symbols and aggregate counts. If the old symptom is
  absent, record the changed conditions and seek a representative normal-use
  sample rather than declaring it fixed or starting paid agent work.

**Read-only command examples:**

```bash
tmux display-message -p '#{pid} #{socket_path}'
tmux list-sessions -F '#{session_id} #{session_name}'
tmux list-clients -F '#{client_name}|#{client_session}|#{client_width}|#{client_height}|#{client_flags}'
tmux list-panes -a -F '#{session_id}|#{window_id}|#{pane_id}|#{pane_pid}|#{@devws_role}'
ps -axo pid=,ppid=,stat=,comm=
bash scripts/measure_load.sh live --seconds 10
sample "$ITERM_PROCESS_ID" 2 10 -file "$ITERM_BACKUP_ROOT/baseline.sample"
```

Resolve IDs from live metadata; do not reuse historical PIDs from earlier notes.

**Verification:**
- A repeated baseline, identity inventory, and private preservation manifest
  exist. No terminal content, preference values containing secrets, or process
  command arguments appear in the repository report.
- The original app/client is still available, and detaching a client is confirmed
  to preserve the server and sessions. Any control client is explicitly tracked.

## Step 2 — Compare a fresh client on the existing version

**Status:** Attempted 09-11-2026: attaching froze iTerm2, which had to be force-quit

**Resources:** A fresh iTerm2 window/tab and the existing tmux session.

**Actions:**
- Open a fresh terminal client using the same profile and dimensions. Attach
  directly to the existing server/session, avoiding the devws startup picker and
  restore path. For a named session, the attachment takes the form below; include
  the recorded socket if it is not the default server.

```bash
tmux attach-session -t "=$ITERM_TEST_SESSION"
```

- Compare clients sequentially, with one visible measured client at a time. Use
  `tmux detach-client -t "$ITERM_ORIGINAL_CLIENT"` only after survival checks and
  the replacement/control connection are ready. The original tab remains intact
  and available for reattachment. Do not close a window to achieve detachment.
- Match the selected pane/window, focus, profile, dimensions, and live output
  activity. Record unavoidable differences in workload or client count.
- Collect three ten-second samples per condition, alternating original/fresh
  where practical. Check typing, scrolling, menu overlays, and pane switching.
  Do not type test commands or prompts into running agents.
- Recheck pane and agent process identities after switching clients.

**Decision:**
- A useful CPU improvement should be sustained across repeated samples, with
  the reduction larger than the observed within-condition variation and no
  interaction regression. Report medians and ranges, including any workload
  difference; do not infer screen latency from tmux query latency.
- If the fresh client restores responsiveness and substantially reduces CPU,
  retain it, preserve the old client state, and proceed to Step 6. Record the
  update as optional future maintenance rather than an established performance fix.
- Otherwise proceed to staging the update.

**Verification:**
- Fresh-client results are recorded separately from any version change.
- Existing session/pane identities and agent processes are preserved, and the
  original client can still be reattached.

## Step 3 — Stage the update and a private rollback

**Status:** Not needed — terminal replaced by Ghostty

**Resources:** Official iTerm2 release, installed app bundle, live/custom settings,
and private backup directory.

**Actions:**
- Recheck the [official stable release](https://iterm2.com/downloads.html) and its
  requirements at execution time. The candidate verified during specification
  was 3.7.7 for macOS 13+. Record the actual selected version and official URL.
- Download to the private staging directory, extract there, and verify release
  version, architecture/OS compatibility, bundle identity, code signature, and
  Gatekeeper assessment. Verify any published release checksum/signature using
  the official release information. Resolve a failed identity/assessment before
  launching the candidate; do not disable platform checks.
- Copy the installed app bundle with metadata preserved and record its version
  and signature. Keep the existing installation in place while staging.
- Back up the live preferences domain and its plist, any selected custom
  preferences plist, relevant dynamic profiles and saved-state resources, plus
  the terminal-local state identified in Step 1. Record original paths and
  whether each resource existed, so rollback does not replace unrelated files.
- Record relevant preference comparisons privately: profile identities, font/
  color and renderer settings, global/profile key maps, custom preferences mode,
  and the Shift+Enter mapping. Do not write preferences while the old app runs.
- Prepare a control terminal outside the app being restarted. Its purpose is
  to retain access to the existing tmux server while iTerm2 is closed, not to
  restart workspaces. Inventory any other work that a GUI quit could interrupt.

**Verification command examples:**

```bash
codesign --verify --deep --strict --verbose=2 "$ITERM_CANDIDATE_APP"
codesign -dv --verbose=4 "$ITERM_CANDIDATE_APP"
spctl --assess --type execute --verbose=2 "$ITERM_CANDIDATE_APP"
```

Inspect identity details privately and compare the signer with official release
provenance; a successful generic signature check alone is not publisher identity.

**Verification:**
- Candidate verification passed, the old app and configuration backups are
  present, and a concrete rollback manifest is ready before the app is quit.
- Terminal-local state is preserved or a specific unresolved limitation is
  identified. If preservation cannot be verified, leave the old app running and
  report that limitation before any dependent cutover.

## Step 4 — Cut over the terminal app and compare versions

**Status:** Not needed — terminal replaced by Ghostty

**This is the planned app-restart point.** Agent processes must remain running.

**Actions:**
- Detach the original iTerm2 client after confirming the control connection and
  server/session survival. Quit iTerm2 normally through the GUI. Do not force-kill
  it or its terminal child processes.
- After the app has fully exited, capture a second settings/saved-state backup
  before launching the new version. This captures preference changes that the
  old app held in memory until quit. Keep both backup generations.
- Install the verified candidate at the normal app path using a metadata-
  preserving copy and retain the old bundle privately for rollback. Filesystem
  permission prompts may be required by the platform; execution approval covers
  this planned operation, but does not bypass those prompts.
- Launch only the selected app, verify its running version/path, then attach a
  fresh client directly to the existing tmux session. Restore the measured
  geometry and profile. Decline unrelated integration/setup offers during the
  comparison.
- Compare fresh-old-version samples from Step 2 with at least three fresh-new-
  version samples under the same conditions. Report the version effect separately
  from the original-client versus fresh-client effect. Repeat a controlled
  disposable output workload if live agent activity differs too much.
- Verify typing, scrolling, menus while output is streaming, pane switching,
  focus events, fonts/colors, and profile selection. Test Shift+Enter in a
  disposable input-capture shell, expecting ESC CR (`1b 0d`), rather than sending
  it to an agent. Verify editor-follow behavior using an isolated fixture if
  further testing is needed.
- Compare recorded tmux server/session/pane and agent identities before/after.
  Distinguish unrelated natural process exits from a cutover-induced restart.

**Verification:**
- The expected app version is running; original agents remain in their panes.
- Settings and keyboard behavior match the preserved configuration.
- CPU ranges, query latencies, and observed interaction behavior are recorded.
  Retain the update only if compatibility checks pass; document whether it
  actually improves performance.

## Step 5 — Investigate a remaining renderer cost or roll back

**Status:** Not needed — terminal replaced by Ghostty

**Actions:**
- If CPU remains excessive, collect a new short stack profile. Determine whether
  annotation-range traversal still dominates or a different path is now visible.
- Select at most one relevant documented renderer setting at a time, record its
  original value, run matched samples, and restore it before the next comparison.
  Do not invent preference keys, clear annotations/history, change agent workloads,
  or install shell integrations to manufacture an improvement.
- If a setting offers a repeatable benefit without display/input regressions,
  document the exact setting and retain it only within the approved scope.
  Otherwise restore the original setting and report the measured result.
- If the new version regresses compatibility, use the control connection to
  keep tmux access, detach the candidate client, and quit the candidate normally.
  Preserve its current settings separately before rollback. Restore the old app
  and its matching configuration snapshot while iTerm2 is closed, respecting
  the custom preferences path and the preference-domain cache. Relaunch the old
  app, reattach, and verify identities, mapping, profiles, and state again.
- Use targeted file/domain restoration from the manifest. Do not restore a whole
  home directory or overwrite unrelated user data. Restore only app-owned changes
  introduced by this trial.

**Verification:**
- The selected version/settings are explicit; no diagnostic override remains
  accidentally enabled.
- Rollback, if used, preserves agents and restores compatible settings. If the
  slowdown remains, the report contains the new profile evidence and a specific
  next investigation rather than a claim that the issue was resolved.

## Step 6 — Save findings and verify the final state

**Status:** Done 09-11-2026 — results in [measurements](measurements.md#resolution)

**Files:** `measurements.md` in this folder, this plan/spec, and
`docs/architecture/agent-performance.md` as needed.

**Actions and verification:**
1. Record the selected app/client/settings, exact measured conditions, repeated
   CPU/query samples, observed interaction results, preserved process identities,
   backup location description, and rollback instructions. Keep personal backup
   contents and raw terminal state outside Git.
2. Verify no extra diagnostic/control client remains attached after the final
   working client is established, and preserve the old client state as promised.
3. Update the performance documentation with confirmed findings. Distinguish
   fresh-client improvement from version improvement and note unresolved limits.
4. For documentation-only repository changes, verify links and run
   `git diff --check`. An external app update alone does not justify repeating
   the full devws code suite. If code changes become necessary, write a regression,
   run affected tests, and run `make check` with tmux access before completion.
5. Mark completed/conditional steps accurately. A step skipped because an earlier
   comparison solved the problem is recorded as unnecessary, not silently marked
   executed. Do not claim improved screen response without observation.

## Approval and scope

The user approved the specification and execution on 08-11-2026. Execution is in progress.
Execution approval will cover the described fresh-client trials and conditional
verified iTerm2 update/restart with backups and rollback. No agent restart,
history deletion, preference reset, or unrelated integration installation is
included. No commit is requested.

## Execution record

- The user approved execution. The computer-control tool then denied iTerm2
  access for safety reasons; no alternate UI automation was attempted.
- Current baseline: iTerm2 92.3%, 94.2%, 92.1% of one core across three ten-second
  samples. Four sessions, eleven panes, and one focused 238×65 client were recorded.
- Effective exit-unattached/destroy-unattached settings are off. Preferences were
  backed up privately, and the Shift+Enter mapping matches its existing contract.
- The process inventory found terminal leaf programs tmux and two zsh shells;
  this is not a substitute for checking GUI-local unsaved state. No saved-state
  directory was found at the standard path, so terminal-local preservation has
  not been established for an app restart.
- A manual fresh-client attachment was requested. The last metadata check still
  showed only the original /dev/ttys000 client. Steps requiring the replacement
  client, or GUI app switching/restart, have not been performed.
- No app update, client detach, preferences mutation, or agent restart occurred.
