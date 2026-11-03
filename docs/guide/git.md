# Git

The menu's **Git** tab commits and pushes the workspace's branch. Each
workspace is one branch in its own folder ([Workspaces](workspaces.md)), so
there is nothing to stash or switch.

## Commit

- **✓ Commit (agent)** (`C-b g`, `devws commit`) asks the workspace's agent
  to commit: its `/commit` skill when the project has one, otherwise a
  request for a conventional commit message with no AI or co-author credits.
  The commit happens in the agent's pane, where you can follow it.
- An agent that is working or waiting on you is not interrupted: devws says
  "The agent is busy" and you commit by hand instead.
- **✎ Commit manually** (`devws commit --manual`) lists the changed files,
  all ticked (`Tab` unticks one), then asks for the message, suggested from
  the branch (`feat/login` → `feat: `). Nothing ticked, or an empty
  message, makes no commit.

## Push and pull requests

**↑ Push / PR** (`C-b G`, `devws push`):

1. When the branch has no pull request yet, asks `Open a pull request after
   pushing? [Y/n, d = draft]`.
2. Pushes as a background job (the header shows it), setting the branch's
   upstream. A push is never forced.
3. Shows the existing pull request, or creates one (`gh pr create --fill`,
   a draft for `d`) and opens it in the browser.

The sidebar then shows its state next to the branch (`· PR open`), checked
again every few minutes. A failed push (rejected, no network) shows as a
failed job: its log panel says what git answered. Without gh, devws pushes
and says to install gh for the PR.

## Done with a branch

- **× Close workspace** offers to remove the branch folder (kept unless you
  type `y`, never with changes).
- **⌫ Tidy up merged branches** (`devws tidy`) removes, after asking, every
  branch folder whose PR is merged or closed and that is clean and not open.

The branches themselves are never deleted; `git branch -d` them once merged.
