# JungleForge, jobs and database

The menu's **JungleForge** tab manages and runs your branch's apps in Docker.
The **Tools** tab opens the database client and background job log.

## Set up jungleforge

[jungleforge](https://github.com/IvaStival/jungleForge) is a Docker control
plane: it runs each registered app as one container, beside shared
databases, Redis, RabbitMQ and MinIO. devws uses it to run your branches.

Without it, the JungleForge tab shows **⚒ Set up JungleForge** in place of the run
actions (`./install.sh` offers the same, and `./install.sh --jungleforge`
asks again after a no). Setting up:

1. clones the public jungleforge into `~/.devws/jungleforge`;
2. writes its `.env` from `.env.example`, with a generated password for each
   shared service (an existing `.env` is never touched);
3. checks Docker, saying so when it is missing or not running.

It needs Docker Desktop. The first Run of an app builds jungleforge's base
image, which takes a few minutes; later runs reuse it.

devws finds jungleforge at `DEVWS_JUNGLEFORGE` when it is set, else in
`~/.devws/jungleforge`, else in `~/Projects/jungleforge`.

## Register a repo

jungleforge only runs apps in its registry. **＋ Register in JungleForge**
adds the workspace's repo, app by app (running an unregistered repo offers
it too):

1. **Where the app is:** the repo root or one of its folders (`backend/`,
   `web/`…); folders that already have a `deploy/` are marked.
2. **Its name:** the repo's in kebab case by default (`PhiPlanner` →
   `phi-planner`).
3. **No `deploy/` there?** Pick the stack (php, node, next, frankenphp,
   rust, external): devws copies jungleforge's template into `deploy/`,
   named after the app, on the next free port from 8090. A `.docker-env` git
   ignores goes to the repo's `.shared/`, linked into every branch.
4. The app is added to jungleforge's `.env` as `<NAME>_PATH`. For a GitHub
   workspace the path is the default branch's own folder
   (`<repo>-main`, created when missing), which is what "main" runs.

Repeat for each app of a repo that has several. Then commit the new
`deploy/` (main runs it once merged; Run this branch works now) and point
the app's own `.env` at the shared services: `DB_HOST=postgres`,
`REDIS_HOST=redis`, `RABBITMQ_HOST=rabbitmq`.

## Run branches in jungleforge

A repo's apps are the registry entries whose folder is a checkout of the
same repo: PhiPlanner's `phi-planner` (`backend/`) and `phi-planner-web`
(`web/`), say. Running a branch starts the same containers, ports and shared
databases on the branch's folder, for that one command (the registry is
never edited for it). One branch of an app runs at a time.

| Menu | Key | Command | What it does |
| ---- | --- | ------- | ------------ |
| **▶ Run this branch** | `C-b e` | `devws run` | Runs the workspace's branch. With several apps, all ticked (`Tab` unticks one). Shared services start first when down |
| **⇄ Switch running branch** | `C-b E` | `devws run <branch\|main>` | Runs `main` (the registry's own checkout) or another branch folder; `▶` marks the running one |
| **■ Stop** | | `devws stop` | Stops the repo's apps |
| **⛃ Services** | | | Starts the shared services (databases, queues) |
| **⇡ Migrate** | | | Runs the app's migrations. Switching never migrates: the databases are shared |
| **⌘ Shell** | | | A shell in the app's container, in a panel below |
| **≡ Logs** | | | The app's container logs, in a panel below |
| **ⓘ Info** | | | jungleforge's table of its containers, services and apps, running or not: status, image, ports |
| **⚙ Settings** | | | Changes an app's setting (its port…) or a shared service's: [Change a setting](#change-a-setting) |

- Each runs as a background job (below), so the screen stays yours.
- Packages install in the container only when `composer.lock` or
  `package-lock.json` changed since that folder's last install: a new branch
  installs once, switching back and forth stays fast.
- The sidebar shows `· ▶ running` on the workspace whose branch runs.

## Change a setting

**⚙ Settings** changes a jungleforge setting, such as an app's port. Pick
whose settings, then the setting (shown with its value and comment), then type
the new value; Enter on an empty value keeps the old one.

- **An app's** settings (`APP_HTTP_PORT`, `VITE_PORT`, versions…) are its
  `deploy/.docker-env` in the workspace's folder: the one this branch runs
  with. When that file is in git, the change shows in the branch's diff.
- **The shared services'** (database, Redis, RabbitMQ ports…) are in
  jungleforge's `.env`. The registry (`*_PATH`) is left to Register, and
  passwords show as `••••`.
- A port must be a number from 1 to 65535 that no other registered app or
  shared service uses; the refusal says who has it, and you type again.
- A new port only applies once the container is made again: an app running
  from this branch is offered a restart, and a shared change offers
  restarting the services. An app running from another branch takes the
  change when this branch runs.
- From the command line: `jungleforge.sh settings <folder> <app>` (or
  `settings shared`) lists them, and `jungleforge.sh set <folder> <app> <KEY>
  <value>` (or `set shared <KEY> <value>`) changes one.

## Database pane

**Tools → Database** (`C-b B`) opens **devdb**, a database client, beside the
editor and connects it to the project's database. Run the action again to focus it.

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
- **Tools → Job log** (`C-b j`, or a click on the job in the header) opens it
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
