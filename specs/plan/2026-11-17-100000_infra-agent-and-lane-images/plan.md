# An infra agent, image builds, and front and back side by side

Status: built and installed (2026-11-17); infra role provisioned in ~/.devagentics/roles; live run pending (the leader starts infra-1). Not done: image_try for the leader (it has no snapshot), a two-lane Docker flow test

## Context
On `feat-system-observer` the backend chain (`C5a-1-AT` and the 8 items
after it) could run beside the frontend chain, but it is blocked, and the
leader keeps asking the user for infrastructure:
- **There is no PHP image.** The runner has only the Node `web` image and
  bare Alpine. Workers get no Docker socket and no network, so no team member
  can build one. The leader asked the user to build it.
- **One runner image and one set of checks per task.** `state.runner` holds
  a single `runner::Configuration`. `check_profiles.publisherProvider` runs
  `lint`/`typecheck`/`test` as `cd web && …`, and the backend items name those
  same profiles.
- **Every runner change is a user permission.** In `mcp.rs` `call`,
  `runner_configure` goes to `authorized_call(.., Effect::External)`. 8 of the
  leader's asks were these.
- **There is no `back` lane.**

The user's direction:
- The leader should hand infrastructure to an infra agent.
- The leader decides image builds; the user isn't asked.
- Plan this together with a per-lane image and per-lane checks.

## Changes
1. **An `infra` role** (`agentic/roles/samples/infra.md`, added to
   `roles::provision_specialists` in `agentic/src/roles.rs`).
   - A read-only senior. It never writes the repo; it builds runner images
     and works out check commands.
   - Its guidance:
     - build from the project's manifests (`composer.lock`,
       `package-lock.json`) with pinned base images;
     - put dependencies the snapshot lacks (`vendor`, `node_modules`) in image
       folders, mounted read-only;
     - try every check command in the image before reporting;
     - report the image ID, the lane's `image_mounts` and the exact check
       commands.
2. **devAgentics builds images** (`agentic/src/runner.rs`, new
   `pub fn build_image(task, request) -> Result<Built>`).
   - Request: `{name, dockerfile, context_files}`. `context_files` are paths of
     tracked repo files. They are read with the existing `regular()` helper,
     which refuses symlinks and escapes, and copied into
     `task.dir()/images/<name>/context/`, beside the Dockerfile.
   - It runs `docker build` on the user's rootless daemon only (the existing
     `user_endpoint`/`docker` helpers):
     - network on for the build only;
     - `--memory 4g --cpus 2`;
     - a 30-minute timeout;
     - tag `devws-runner/<repo>:<name>`;
     - label `devagentics.task=<id>`.
   - No host folder is mounted. The log goes to `images/<name>/build.log`, and
     the result carries only its last 64 KB.
   - Returns the image ID (`sha256:…`), which `runner_images` then lists.
   - New `pub fn try_image(task, agent, image, command)`. It runs
     `run_container` with the task's configuration on that image, against the
     agent's read-only snapshot, without network. This is how the infra agent
     checks `php artisan test` before reporting.
3. **The tools.**
   - Worker actions `image_build` and `image_try`, for the `infra` role only
     (`agentic/src/worker_tools.rs`, where role-gated actions are dispatched;
     `review_submit` is reviewer-only in the same way).
   - The leader gets the same two as `application_workflow` actions
     (`agentic/src/workflow.rs`), with `Effect::System`.
4. **A runner image per lane** (`runner.rs`).
   - `Configuration.lanes: BTreeMap<String, LaneImage { image, image_mounts }>`,
     empty by default.
   - `Configuration::for_lane(lane)` returns the configuration with that
     lane's image and mounts.
   - These use the agent's lane:
     - `run_for` (worker `execute`);
     - the checkpoint;
     - `check_profiles::run_profile`;
     - the candidate check of that lane's candidate;
     - the environment preflight fingerprint.
   - Same validation as the main image: a local immutable ID, image folders
     only.
5. **Check commands per lane** (`agentic/src/check_profiles.rs`).
   - `RepositoryChecks.lanes: BTreeMap<String, BTreeMap<String, Profile>>`. A
     lane's profile replaces the repo's profile of the same name for that
     lane's orders.
   - `validate` refuses a lane the task doesn't have.
   - The evidence signature includes the lane.
6. **Runner changes are the leader's** (`agentic/src/mcp.rs` `call`).
   - `runner_configure` runs without a permission request.
   - Exception: it still needs the user when it changes `docker_host`, which
     is outside the project.
   - Validation already bounds everything else: local image IDs, memory up to
     8 GiB, up to 4 CPUs, image folders as mounts, no network.
7. **The leader's role** (`agentic/roles/leader.md`).
   - Infrastructure (images, runner, a lane's checks) goes to an `infra`
     agent.
   - Runner changes and image builds are yours to decide: tell the user what
     you did, don't ask.
   - Replace the paragraph saying `runner_configure` needs explicit consent.
   - Update `agentic/docs/usage.md` to match.

## Tests (TDD)
- `runner.rs`:
  - `for_lane` gives the lane's image and mounts, and the main ones for other
    lanes;
  - a non-local lane image is refused;
  - `build_image` copies only the listed tracked files, and refuses a symlink
    or `../` path. The Docker command line is built by a pure function, tested
    for the rootless endpoint, the caps, the tag and the label.
  - An ignored Docker test builds a two-line image from alpine.
- `check_profiles.rs`:
  - a back-lane order resolves `test` to the lane's command, and a front order
    to the repo's;
  - an unknown lane is refused;
  - front evidence doesn't satisfy a back order.
- `worker_tools.rs`: `image_build` is refused for a hammer and a planner, and
  allowed for infra.
- `mcp.rs`:
  - `runner_configure` with an unchanged `docker_host` makes no permission
    request;
  - a changed `docker_host` makes one.
- `cargo test`, `cargo clippy --all-targets`, Docker tests with
  `DEVAGENTICS_TEST_IMAGE`, install, restart the board (the user does it, or
  says when it's free).

## Live
The leader:
1. provisions roles if `infra` is missing;
2. starts `infra-1`;
3. orders it to build the backend image (PHP 8.4, pdo_sqlite, `vendor` from
   `composer.lock`) and verify Pint, `php -l` and `php artisan test` with
   `image_try`;
4. then makes one batched change on its own: `add_lane back`, `runner_configure`
   with `lanes.back` and `backend/**` sources, and the baseline revision with
   the back lane's checks;
5. starts a backend hammer on `C5a-1-AT` beside the front work.

Check: `C5a-1-AT` runs and its checks run in the back image, while B2/B3
continue in the front lane.

The plan is saved as `specs/plan/2026-11-17-*_infra-agent-and-lane-images/plan.md`.
It replaces `2026-11-17-093000_lanes-side-by-side`.
