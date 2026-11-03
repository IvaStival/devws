# App

The menu's **App** tab runs the branch you work on: its apps in
jungleforge (Docker), its database, and the
background jobs that do the long work.

## Run branches in jungleforge

jungleforge (`DEVWS_JUNGLEFORGE`, `~/Projects/jungleforge`) runs the apps
in its registry. A repo's apps are the entries whose folder is a checkout of
the same repo: PhiPlanner's `phi-planner` (`backend/`) and
`phi-planner-web` (`web/`), say. Running a branch starts the same
containers, ports and shared databases on the branch's folder, for that one
command (jungleforge's `.env` is never edited). One branch of an app runs at
a time.

| Menu | Key | Command | What it does |
| ---- | --- | ------- | ------------ |
| **▶ Run this branch** | `C-b e` | `devws run` | Runs the workspace's branch. With several apps, all ticked (`Tab` unticks one). Shared services start first when down |
| **⇄ Switch running branch** | `C-b E` | `devws run <branch\|main>` | Runs `main` (the registry's own checkout) or another branch folder; `▶` marks the running one |
| **■ Stop** | | `devws stop` | Stops the repo's apps |
| **⛃ Services** | | | Starts the shared services (databases, queues) |
| **⇡ Migrate** | | | Runs the app's migrations. Switching never migrates: the databases are shared |
| **⌘ Shell** | | | A shell in the app's container, in a panel below |
| **≡ Logs** | | | The app's container logs, in a panel below |

- Each runs as a background job (below), so the screen stays yours.
- Packages install in the container only when `composer.lock` or
  `package-lock.json` changed since that folder's last install: a new branch
  installs once, switching back and forth stays fast.
- The sidebar shows `· ▶ running` on the workspace whose branch runs.
- A repo jungleforge does not know says so: register it in jungleforge's
  `.env`.

## Database pane

**⛁ Database** (`C-b B`) opens **devdb**, a database client, beside the
editor, connected to the project's database; again focuses it.

- **Keys:** `Enter` edits a cell, `+` adds a row, `d` marks one for
  deletion, `Ctrl+S` saves the changes, `Ctrl+R` reverts them; `/`
  filters, `[` `]` page, `v` shows a long value; the **Query** tab runs SQL
  (or MongoDB JSON) with `Ctrl+J`; `F2` the connections; `Ctrl+Q` quits;
  `F1` lists every key.
- **The connection** comes from the project's `.env`: `DATABASE_URL`,
  `DB_URL`, `MONGODB_URI`, `MONGODB_URL` or `MONGO_URL`, otherwise
  Laravel's `DB_CONNECTION` (`mysql`, `mariadb`, `pgsql`, `sqlite`,
  `mongodb`) with `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME`,
  `DB_PASSWORD`. SQLite works from a file path too.
- **⛁ Database folder** chooses which `.env`: every folder with one, up to
  three levels down, plus **Automatic** (the root's, else a direct
  subfolder's, asking when several qualify). Remembered per project.
- Without database settings devdb opens its connection screen; what you set
  up there is saved for the project, the `.env` untouched.
- A host that only exists inside Docker (a Sail `mysql`) is not reachable:
  point `DB_HOST` at a published port. A missing SQLite file is reported,
  not created.
- Its header `▾ Database` hides it to a bar and back.

## Background jobs

Long work never holds the screen: Run, Switch, Stop, Services, Migrate,
Push, Tidy and closing a running branch's workspace ask their questions,
close the menu and carry on as a background job.

- **The header** shows the jobs where the network and weather are, each
  with its state, progress and time:
  `⏵ phi-planner → feat/login ▓▓▓▓░░░░░░ 4/9 0:42`. Jobs on the same app
  wait in line (`queued`). A success shows `✓` for 10 seconds; a failure
  stays `✗ … failed` until you open its log.
- **The log panel** opens below the terminal of the workspace that started
  a job still running after 3 seconds: the live log, in color. It never
  takes the focus. A success closes it after 5 seconds; a failure stays.
  `q` closes it at any time; the job carries on.
- **◷ Job log** (`C-b j`, or a click on the job in the header) opens it
  again: the latest job, a failure first.
- **A failure says why** when its exit code tells, at the end of its log and
  in its notification:

  | Exit | Hint |
  | ---- | ---- |
  | 137 (`Error 137`) | killed — usually out of memory: give Docker Desktop more (Settings → Resources) |
  | 127 | command not found |
  | 130, 143 | interrupted |

- **Notifications:** when a job ends while you are elsewhere, macOS tells
  you: a failure always, a success when it took over 30 seconds. Clicking
  opens its log. `DEVWS_NOTIFY=0` turns them off.
- **Errors in the background** (the log panel cannot open, the sidebar's
  refresh fails) show in the header as an error note instead of nothing
  happening.

Any command can be a job:

```sh
devws job "Tests" -- make test          # [--lock <name>] to queue with others
devws note "Deployed" [--error]         # a message in the same place
devws jobs                              # recent jobs
devws jobs log [id]                     # page a log (the failed one first)
```

Jobs and their logs are kept 7 days in `~/.devws-state/jobs/`.
