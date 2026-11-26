//! Containers see disposable source snapshots, never host projects or data volumes.
use crate::{checks, execution::Workflow, state::LEADER, task::Task};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const FILE_LIMIT: u64 = 16 * 1024 * 1024;
const TREE_LIMIT: u64 = 256 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 64 * 1024;
const RESULT_LIMIT: usize = 2 * 1024 * 1024;
const RESULT_PAGE: usize = 8 * 1024;
const RESULTS_PER_SESSION: usize = 32;
const PREVIEW_LIMIT: usize = 1_200;
const DIAGNOSTIC_LINE_LIMIT: usize = 180;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// A local immutable image ID. The runner never pulls arbitrary images.
    pub image: String,
    pub source_paths: Vec<String>,
    pub protected_paths: Vec<String>,
    pub timeout_seconds: u64,
    /// The rootless Docker daemon every container runs on (`unix://…`);
    /// None is the user's own, `$XDG_RUNTIME_DIR/docker.sock`. Never a shared
    /// or root daemon: a host's other containers stay out of reach.
    #[serde(default)]
    pub docker_host: Option<String>,
    /// Folders of the image mounted read-only inside the workspace: the
    /// dependencies a project keeps untracked (`node_modules`), which the
    /// tracked-source snapshot lacks and a no-network run cannot fetch.
    #[serde(default)]
    pub image_mounts: Vec<ImageMount>,
    /// Each container's memory in MiB (default and most: `MEMORY_LIMIT_MB`)
    /// and CPUs (default 1). Bounded: the runner shares the host with
    /// everything else on it.
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub cpus: Option<u32>,
    /// A lane's own image, for work the main image cannot run (a PHP
    /// backend beside a Node frontend). Left out when empty, so a task
    /// without lane images keeps its evidence signatures.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lanes: BTreeMap<String, LaneImage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneImage {
    /// A local immutable image ID, like the main one.
    pub image: String,
    #[serde(default)]
    pub image_mounts: Vec<ImageMount>,
}

impl Configuration {
    /// What a container of `lane` runs with: the lane's image and mounts when
    /// it has its own, the main ones otherwise.
    pub fn for_lane(&self, lane: Option<&str>) -> Configuration {
        let mut config = self.clone();
        config.lanes = BTreeMap::new();
        if let Some(own) = lane.and_then(|name| self.lanes.get(name)) {
            config.image = own.image.clone();
            config.image_mounts = own.image_mounts.clone();
        }
        config
    }

    /// The configuration without its lane images: what a lane-only change
    /// leaves as it was.
    fn without_lanes(&self) -> Configuration {
        let mut config = self.clone();
        config.lanes = BTreeMap::new();
        config
    }
}

/// No container gets more than 4 GB, swap included (images.rs builds with the
/// same): a runaway test cannot take the host's memory.
pub const MEMORY_LIMIT_MB: u32 = 4096;
const MEMORY_MB: std::ops::RangeInclusive<u32> = 128..=MEMORY_LIMIT_MB;
const CPUS: std::ops::RangeInclusive<u32> = 1..=4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageMount {
    /// An absolute folder in the image (`/node_modules`).
    pub path: String,
    /// Where it appears, relative to the workspace (`web/node_modules`).
    pub target: String,
}

/// The Docker endpoint of `config`: its pinned one, or the user's rootless
/// socket in `runtime_dir` (their `$XDG_RUNTIME_DIR`).
pub fn endpoint(config: &Configuration, runtime_dir: Option<&str>) -> Result<String> {
    match &config.docker_host {
        Some(host) => Ok(host.clone()),
        None => user_endpoint(runtime_dir),
    }
}

/// The user's rootless Docker socket in `runtime_dir`.
fn user_endpoint(runtime_dir: Option<&str>) -> Result<String> {
    let runtime_dir = runtime_dir
        .filter(|folder| !folder.is_empty())
        .context("no XDG_RUNTIME_DIR: name the rootless Docker socket in docker_host")?;
    Ok(format!("unix://{runtime_dir}/docker.sock"))
}

pub(crate) fn runtime_dir() -> Option<String> {
    let uid = unsafe { libc::geteuid() };
    runtime_dir_from(
        std::env::var("XDG_RUNTIME_DIR").ok(),
        Path::new(&format!("/run/user/{uid}")),
    )
}

/// `$XDG_RUNTIME_DIR`, else the standard `/run/user/<uid>` when it exists: a
/// process an agent's provider starts (Codex's MCP servers) may get only a
/// few variables, without it.
fn runtime_dir_from(variable: Option<String>, standard: &Path) -> Option<String> {
    variable.filter(|folder| !folder.is_empty()).or_else(|| {
        standard
            .is_dir()
            .then(|| standard.to_string_lossy().into_owned())
    })
}

/// The user's own rootless Docker socket, never a pinned or shared one.
pub(crate) fn user_docker_endpoint() -> Result<String> {
    user_endpoint(runtime_dir().as_deref())
}

fn configured_endpoint(config: &Configuration) -> Result<String> {
    endpoint(config, runtime_dir().as_deref())
}

/// A `docker` command bound to `endpoint`, whatever the environment or the
/// user's current Docker context would choose.
pub fn docker(endpoint: &str) -> Command {
    let mut command = Command::new("docker");
    command
        .env("DOCKER_HOST", endpoint)
        .env_remove("DOCKER_CONTEXT");
    command
}

/// Whether `docker info`'s security options name a rootless daemon.
pub fn rootless(security_options: &str) -> bool {
    security_options
        .split([' ', '[', ']'])
        .any(|option| option == "name=rootless")
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub snapshot: PathBuf,
    pub source: PathBuf,
    pub base: String,
    pub original: BTreeMap<String, String>,
    #[serde(default)]
    pub executable: BTreeMap<String, bool>,
    pub readonly: bool,
    pub order: Option<String>,
    pub revision: u64,
    pub baseline: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    #[serde(skip)]
    exit_code: Option<i32>,
}
/// A committed dotenv template (`.env.example`…) holds no secrets: it is
/// source when the runner configuration names that exact path, never through
/// a glob. Every other `.env` file stays out.
fn named_dotenv_template(path: &str, source_paths: &[String]) -> bool {
    const TEMPLATES: [&str; 4] = [".env.example", ".env.sample", ".env.dist", ".env.template"];
    let name = path.rsplit('/').next().unwrap_or(path);
    TEMPLATES.contains(&name) && source_paths.iter().any(|source| source == path)
}

/// Why a path is kept out of every snapshot (secrets, data, dependencies,
/// the user's protected paths); None when nothing keeps it out.
fn exclusion(path: &str, config: &Configuration) -> Result<Option<String>> {
    excluded_by(path, &config.source_paths, &config.protected_paths)
}

/// The built-in rules, with the source paths that may name a dotenv template
/// and the user's protected paths.
fn excluded_by(
    path: &str,
    source_paths: &[String],
    protected_paths: &[String],
) -> Result<Option<String>> {
    const FOLDERS: [&str; 15] = [
        ".git",
        ".ssh",
        ".aws",
        ".codex",
        ".claude",
        ".env",
        "node_modules",
        "vendor",
        "storage",
        "pgdata",
        "mysql",
        "mongodb",
        ".devagentics",
        ".devws",
        ".agents",
    ];
    const EXTENSIONS: [&str; 8] = [
        ".sqlite", ".sqlite3", ".db", ".pem", ".key", ".p12", ".sock", ".dump",
    ];
    let lower = path.to_ascii_lowercase();
    if let Some(part) = lower.split('/').find(|part| FOLDERS.contains(part)) {
        return Ok(Some(format!(
            "it is under or named {part}, which never enters a snapshot (secrets, data or dependencies)"
        )));
    }
    if lower.split('/').any(|part| part.starts_with(".env."))
        && !named_dotenv_template(path, source_paths)
    {
        return Ok(Some(
            "it is a dotenv file, which never enters a snapshot; a template (.env.example, .env.sample, .env.dist, .env.template) does when you name it exactly in source_paths"
                .into(),
        ));
    }
    if let Some(extension) = EXTENSIONS
        .iter()
        .find(|extension| lower.ends_with(*extension))
    {
        return Ok(Some(format!(
            "its {extension} extension marks data or a key, which never enters a snapshot"
        )));
    }
    if checks::glob_set(protected_paths)?.is_match(path) {
        return Ok(Some("it matches the runner's protected_paths".into()));
    }
    Ok(None)
}

/// Why the leader may not read `path`, or None when it may: never what a
/// snapshot always leaves out (secrets, data, the protected paths), even
/// with no runner; with one, only its source and markdown docs.
pub(crate) fn leader_unreadable(
    path: &str,
    config: Option<&Configuration>,
) -> Result<Option<String>> {
    relative(Path::new(path))?;
    let Some(config) = config else {
        return excluded_by(path, &[], &[]);
    };
    if let Some(why) = exclusion(path, config)? {
        return Ok(Some(why));
    }
    if path.ends_with(".md") || checks::glob_set(&config.source_paths)?.is_match(path) {
        return Ok(None);
    }
    Ok(Some(
        "no source_paths entry of the runner matches it".into(),
    ))
}

fn protected(path: &str, config: &Configuration) -> Result<bool> {
    Ok(exclusion(path, config)?.is_some())
}
pub(crate) fn source_path(path: &str, config: &Configuration) -> Result<bool> {
    Ok(!protected(path, config)? && checks::glob_set(&config.source_paths)?.is_match(path))
}

/// Whether `path` is in a container's copy of the project, and the rule that
/// decides it: what the leader, which cannot read this policy, is told.
pub(crate) fn snapshot_decision(path: &str, config: &Configuration) -> Result<String> {
    if let Some(why) = exclusion(path, config)? {
        return Ok(format!("{path} is left out: {why}."));
    }
    Ok(if checks::glob_set(&config.source_paths)?.is_match(path) {
        format!("{path} is in the snapshot: it matches the runner's source_paths.")
    } else {
        format!("{path} is left out: no source_paths entry matches it.")
    })
}
fn relative(path: &Path) -> Result<String> {
    ensure!(
        !path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
        "path must stay within the source snapshot"
    );
    let text = path.to_str().context("non-UTF8 source path")?;
    ensure!(
        !text.is_empty() && !text.contains('\\') && !text.contains('\n') && !text.contains('\0'),
        "invalid source path"
    );
    Ok(text.into())
}
pub(crate) fn regular(root: &Path, path: &str) -> Result<PathBuf> {
    let relative = relative(Path::new(path))?;
    let mut current = root.to_path_buf();
    ensure!(
        !fs::symlink_metadata(root)?.file_type().is_symlink(),
        "snapshot/project root cannot be a symlink"
    );
    for component in Path::new(&relative).components() {
        current.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "symlink is not a source file: {path}"
            );
        }
    }
    Ok(current)
}
pub(crate) fn bytes(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= FILE_LIMIT,
        "source must be a bounded regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(metadata.nlink() == 1, "hardlinked source is not permitted");
    }
    Ok(fs::read(path)?)
}
pub(crate) fn content_hash(content: &[u8]) -> Result<String> {
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(content)))
}

pub(crate) fn validate_image(image: &str) -> Result<()> {
    ensure!(
        image.starts_with("sha256:")
            && image.len() == 71
            && image[7..]
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
        "runner image must be an immutable local sha256 image ID"
    );
    Ok(())
}

pub(crate) fn validate_mounts(mounts: &[ImageMount]) -> Result<()> {
    for mount in mounts {
        let inside = mount
            .path
            .strip_prefix('/')
            .context("an image mount's path is absolute in the image")?;
        relative(Path::new(inside)).context("an image mount's path")?;
        relative(Path::new(&mount.target)).context("an image mount's target")?;
        ensure!(
            !mount.path.contains(',') && !mount.target.contains(','),
            "an image mount path contains a comma"
        );
    }
    Ok(())
}

pub fn validate_configuration(config: &Configuration) -> Result<()> {
    validate_image(&config.image)?;
    validate_mounts(&config.image_mounts)?;
    for (lane, own) in &config.lanes {
        validate_image(&own.image).with_context(|| format!("lane {lane}'s image"))?;
        validate_mounts(&own.image_mounts).with_context(|| format!("lane {lane}'s mounts"))?;
    }
    ensure!(
        (1..=900).contains(&config.timeout_seconds),
        "runner timeout must be 1–900 seconds"
    );
    ensure!(
        MEMORY_MB.contains(&config.memory_mb.unwrap_or(MEMORY_LIMIT_MB)),
        "runner memory must be {}–{} MiB",
        MEMORY_MB.start(),
        MEMORY_MB.end()
    );
    ensure!(
        CPUS.contains(&config.cpus.unwrap_or(1)),
        "runner CPUs must be {}–{}",
        CPUS.start(),
        CPUS.end()
    );
    if let Some(host) = &config.docker_host {
        ensure!(
            host.starts_with("unix://") && host.len() > "unix://".len(),
            "the runner's Docker daemon must be a local Unix socket"
        );
    }
    ensure!(
        !config
            .source_paths
            .iter()
            .any(|path| matches!(path.as_str(), "*" | "**" | "**/*")),
        "declare narrow source roots, not the entire project"
    );
    ensure!(
        !config.source_paths.is_empty(),
        "declare project source paths before enabling the runner"
    );
    for path in config.source_paths.iter().chain(&config.protected_paths) {
        relative(Path::new(path))?;
    }
    checks::glob_set(&config.source_paths)?;
    checks::glob_set(&config.protected_paths)?;
    Ok(())
}
pub fn probe(config: &Configuration) -> Result<()> {
    validate_configuration(config)?;
    let endpoint = configured_endpoint(config)?;
    ensure!(
        endpoint.starts_with("unix://"),
        "reviewed execution requires a local Unix-socket Docker endpoint"
    );
    // A root daemon may run other people's containers (production): reviewed
    // source never goes there.
    let info = docker(&endpoint)
        .args(["info", "--format", "{{.SecurityOptions}}"])
        .output()
        .context("Docker must be installed")?;
    ensure!(
        info.status.success(),
        "the rootless Docker daemon at {endpoint} is not running (systemctl --user start docker)"
    );
    ensure!(
        rootless(&String::from_utf8_lossy(&info.stdout)),
        "the Docker daemon at {endpoint} is not rootless: reviewed work runs only on your own rootless daemon"
    );
    for image in std::iter::once(&config.image).chain(config.lanes.values().map(|own| &own.image)) {
        let result = docker(&endpoint)
            .args(["image", "inspect", "--format", "{{.Id}}", image])
            .output()
            .context("Docker must be installed and running")?;
        ensure!(
            result.status.success() && String::from_utf8_lossy(&result.stdout).trim() == image,
            "configured local Docker image {image} is unavailable; no pull or daemon changes were attempted"
        );
    }
    Ok(())
}
pub fn available(task: &Task) -> Result<()> {
    if task.state()?.workflow == Workflow::Legacy {
        return Ok(());
    }
    let config = task
        .state()?
        .runner
        .context("configure and verify the approved container runner before execution")?;
    probe(&config)
}
/// The lanes a runner change touches, when it touches only lane images;
/// `None` when the main configuration changes too, which every live worker
/// depends on. Workers of other lanes may keep running.
fn changed_lanes(
    current: Option<&Configuration>,
    requested: &Configuration,
) -> Option<std::collections::BTreeSet<String>> {
    let current = current?;
    if current.without_lanes() != requested.without_lanes() {
        return None;
    }
    Some(
        current
            .lanes
            .keys()
            .chain(requested.lanes.keys())
            .filter(|lane| current.lanes.get(*lane) != requested.lanes.get(*lane))
            .cloned()
            .collect(),
    )
}

/// The folders the image mounts go over: Docker cannot create them in a
/// read-only workspace.
fn mount_points(workspace: &Path, config: &Configuration) -> Result<()> {
    for dependency in &config.image_mounts {
        fs::create_dir_all(workspace.join(&dependency.target))?;
    }
    Ok(())
}

pub fn configure(task: &Task, config: Configuration) -> Result<()> {
    probe(&config)?;
    let lanes = task.config()?.lanes;
    for lane in config.lanes.keys() {
        ensure!(
            lanes.iter().any(|found| found.name == *lane),
            "the task has no lane {lane}: add_lane it first"
        );
    }
    // Verify the isolation flags in a real disposable container of every
    // image before enabling.
    for lane in std::iter::once(None).chain(config.lanes.keys().map(|lane| Some(lane.as_str()))) {
        let own = config.for_lane(lane);
        let root = task.dir().join("runner-probe").join(lane.unwrap_or("main"));
        fs::create_dir_all(&root)?;
        mount_points(&root, &own)?;
        let result = run_container(
            &own,
            &root,
            true,
            // A real write: `test -w` answers yes for uid 0 whatever the mount.
            "test ! -e /var/run/docker.sock && test ! -e /host && test ! -e /root/.ssh && ! touch /etc/.devagentics-probe 2>/dev/null && test -r /proc/net/route",
        )?;
        ensure!(
            result.success,
            "container isolation preflight failed: {}",
            result.stderr
        );
    }
    let _control = task.control_lock()?;
    task.transaction(|tx| {
        if tx.state.workflow == Workflow::Reviewed {
            let changed = changed_lanes(tx.state.runner.as_ref(), &config);
            let busy = tx
                .state
                .agents
                .iter()
                .filter(|agent| agent.live)
                .find(|agent| match (&changed, &agent.lane) {
                    (None, _) => true,
                    (Some(lanes), Some(lane)) => lanes.contains(lane),
                    (Some(_), None) => false,
                });
            if let Some(agent) = busy {
                bail!(
                    "pause and close reviewed workers before changing runner policy ({} is live); \
                     a change of lane images only waits for that lane's workers",
                    agent.id
                );
            }
        }
        tx.state.runner = Some(config);
        tx.log(
            crate::log::Kind::Review,
            Some(LEADER),
            None,
            "user configured verified source-only container runner",
        );
        Ok(())
    })
}
fn token() -> Result<String> {
    let mut secret = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut secret)?;
    Ok(secret.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn write_identity(path: &Path, value: &str) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(value.as_bytes())?;
    Ok(())
}
pub fn prepare(task: &Task, agent: &str, source: &Path, readonly: bool) -> Result<PathBuf> {
    let nonce = token()?;
    let workspace = snapshot(task, agent, source, readonly, &nonce)?;
    let identity = task
        .dir()
        .join("workers")
        .join(agent)
        .join(format!("identity-{}", &nonce[..12]));
    write_identity(&identity, &nonce)?;
    task.transaction(|tx| {
        tx.state.runner_workspaces.insert(agent.into(), workspace);
        tx.state
            .runner_identities
            .insert(agent.into(), identity.clone());
        Ok(())
    })?;
    Ok(identity)
}

/// Gives a running worker a new source and keeps its identity. Its panel and
/// MCP server check that identity on every poll, so a new one, even for a
/// moment, makes them exit as a foreign session.
pub fn replace_source(task: &Task, agent: &str, source: &Path, readonly: bool) -> Result<()> {
    ensure!(
        task.state()?.runner_identities.contains_key(agent),
        "worker identity missing"
    );
    let workspace = snapshot(task, agent, source, readonly, &token()?)?;
    task.transaction(|tx| {
        tx.state.runner_workspaces.insert(agent.into(), workspace);
        Ok(())
    })
}

/// Drops a worker that will not run again: its source and identity leave the
/// state, and its source snapshots (every generation) leave the disk. Each
/// workspace holds a hash of every source file, and every state read parses
/// them all, so a long task slowed down with each finished check and agent.
/// Its `control/` folder stays: its turns say why it ended.
pub fn forget(task: &Task, agent: &str) -> Result<()> {
    task.transaction(|tx| {
        tx.state.runner_workspaces.remove(agent);
        tx.state.runner_identities.remove(agent);
        Ok(())
    })?;
    if let Err(error) = remove_snapshots(&task.dir().join("workers").join(agent)) {
        // The state no longer points at them: they only take disk space.
        task.transaction(|tx| {
            tx.log(
                crate::log::Kind::Stop,
                Some(agent),
                None,
                &format!("source snapshots not removed: {error:#}"),
            );
            Ok(())
        })?;
    }
    Ok(())
}

/// Forgets every worker source no agent can use again: finished checks
/// (`candidate-*`, `preflight-*`) and stopped agents. An agent that is live,
/// lost, unavailable or waiting to restart on another account keeps its own,
/// since resuming it binds its order to that source. Returns the forgotten.
pub fn prune(task: &Task) -> Result<Vec<String>> {
    let state = task.state()?;
    let can_run = |id: &str| {
        (id == crate::preview::WORKER && state.preview.is_some())
            || state.restart_on_next_account.contains(id)
            || state
                .agent(id)
                .is_some_and(|agent| agent.live || agent.lost || agent.unavailable)
    };
    let forgotten: Vec<String> = state
        .runner_workspaces
        .keys()
        .chain(state.runner_identities.keys())
        .filter(|id| !can_run(id))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    drop(state);
    for agent in &forgotten {
        forget(task, agent)?;
    }
    Ok(forgotten)
}

/// Prunes when a dispatcher starts, so a task from before pruning sheds what
/// it collected; what it did, or why it could not, goes in the task's log.
pub fn prune_at_start(task: &Task) -> Result<()> {
    let text = match prune(task) {
        Ok(forgotten) if forgotten.is_empty() => return Ok(()),
        Ok(forgotten) => format!("forgot the sources of {} finished workers", forgotten.len()),
        Err(error) => format!("pruning finished workers failed: {error:#}"),
    };
    task.transaction(|tx| {
        tx.log(crate::log::Kind::Stop, None, None, &text);
        Ok(())
    })
}

/// Removes a worker's source snapshots (every generation) and its scratch
/// folder; the rest of its folder stays.
fn remove_snapshots(directory: &Path) -> Result<()> {
    let entries = match fs::read_dir(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let path = entry?.path();
        let removable = path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy();
            name.starts_with("source-") || name == SCRATCH
        });
        if removable {
            remove_tree(&path)?;
        }
    }
    Ok(())
}

fn remove_tree(path: &Path) -> Result<()> {
    fn writable(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            // A read-only snapshot's folders refuse their entries' removal.
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            for entry in fs::read_dir(path)? {
                writable(&entry?.path())?;
            }
        }
        Ok(())
    }
    writable(path).with_context(|| format!("opening {}", path.display()))?;
    fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))
}

const SCRATCH: &str = "scratch";

/// A read-only worker's own writable folder, kept across its commands: each
/// command runs in a fresh container whose /tmp goes with it, so a planner
/// proving its plan in a prototype had to rebuild the prototype every time.
/// Its source stays read-only and it still has no network. None for a
/// writer, which writes its own snapshot.
fn scratch_for(task: &Task, agent: &str) -> Result<Option<PathBuf>> {
    let readonly = task
        .state()?
        .runner_workspaces
        .get(agent)
        .context("worker source missing")?
        .readonly;
    if !readonly {
        return Ok(None);
    }
    let folder = task.dir().join("workers").join(agent).join(SCRATCH);
    fs::create_dir_all(&folder)?;
    Ok(Some(folder))
}

fn mount_scratch(arguments: &mut Vec<String>, folder: &Path) -> Result<()> {
    let folder = folder.canonicalize()?;
    let path = folder.to_str().context("invalid scratch path")?;
    ensure!(!path.contains(','), "invalid Docker scratch mount");
    arguments.splice(
        2..2,
        [
            "--mount".into(),
            format!("type=bind,src={path},dst=/scratch"),
            "--env=DEVAGENTICS_SCRATCH=/scratch".into(),
        ],
    );
    Ok(())
}

fn snapshot(
    task: &Task,
    agent: &str,
    source: &Path,
    readonly: bool,
    nonce: &str,
) -> Result<Workspace> {
    let config = task.state()?.runner.context("runner not configured")?;
    let directory = task.dir().join("workers").join(agent);
    fs::create_dir_all(&directory)?;
    let snapshot = directory.join(format!("source-{}", &nonce[..12]));
    fs::create_dir(&snapshot)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["ls-files", "-z"])
        .output()?;
    ensure!(output.status.success(), "source enumeration failed");
    let paths = String::from_utf8(output.stdout)?;
    let mut original = BTreeMap::new();
    let mut executable = BTreeMap::new();
    let mut total = 0;
    for path in paths.split('\0').filter(|path| !path.is_empty()) {
        relative(Path::new(path))?;
        if !source_path(path, &config)? {
            continue;
        }
        let content = bytes(&regular(source, path)?)?;
        total += content.len() as u64;
        ensure!(
            total <= TREE_LIMIT && original.len() < 20000,
            "source snapshot exceeds its resource limit"
        );
        let destination = regular(&snapshot, path)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(regular(source, path)?)?.permissions().mode() & 0o777;
            fs::set_permissions(&destination, fs::Permissions::from_mode(mode))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            executable.insert(
                path.into(),
                fs::metadata(&destination)?.permissions().mode() & 0o111 != 0,
            );
        }
        original.insert(path.into(), content_hash(&content)?);
    }
    // Tracked source under an image mount would be hidden by it.
    for dependency in &config.image_mounts {
        let prefix = format!("{}/", dependency.target);
        ensure!(
            !original
                .keys()
                .any(|path: &String| path.starts_with(&prefix)),
            "the image mount at {} would hide tracked source under it",
            dependency.target
        );
    }
    mount_points(&snapshot, &config)?;
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        fn allow_snapshot(path: &Path) -> Result<()> {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::symlink_metadata(path)?;
            ensure!(!metadata.file_type().is_symlink(), "snapshot symlink");
            if metadata.is_dir() {
                for entry in fs::read_dir(path)? {
                    allow_snapshot(&entry?.path())?;
                }
                fs::set_permissions(path, fs::Permissions::from_mode(0o777))?;
            } else {
                fs::set_permissions(
                    path,
                    fs::Permissions::from_mode(0o666 | (metadata.permissions().mode() & 0o111)),
                )?;
            }
            Ok(())
        }
        allow_snapshot(&snapshot)?;
    }
    Ok(Workspace {
        snapshot,
        source: source.canonicalize()?,
        base: checks::head(source)?,
        original,
        executable,
        readonly,
        order: None,
        revision: 0,
        baseline: task.state()?.application.revision,
    })
}
pub fn authenticate(task: &Task, agent: &str, identity: &Path) -> Result<()> {
    let state = task.state()?;
    let expected = state
        .runner_identities
        .get(agent)
        .context("unknown worker session")?;
    ensure!(
        identity == expected && identity.starts_with(task.dir().join("workers")),
        "foreign worker session"
    );
    ensure!(
        state.agent(agent).is_some_and(|agent| agent.live),
        "worker session is no longer active"
    );
    let supplied = fs::read(identity)?;
    ensure!(
        supplied.len() == 64 && supplied.iter().all(u8::is_ascii_hexdigit),
        "invalid worker identity"
    );
    Ok(())
}
pub fn bind_order(task: &Task, agent: &str, order: &str) -> Result<()> {
    let state = task.state()?;
    let worker = state
        .runner_workspaces
        .get(agent)
        .context("worker has no isolated source")?;
    let source = worker.source.clone();
    let readonly = worker.readonly;
    drop(state);
    // A new source generation discards stale tool context without deleting
    // old artifacts, and keeps the identity the worker's panel checks.
    replace_source(task, agent, &source, readonly)?;
    // Each order starts with an empty scratch folder: the last one's
    // prototype was built on a source that is gone.
    let scratch = task.dir().join("workers").join(agent).join(SCRATCH);
    if scratch.exists() {
        remove_tree(&scratch)?;
    }
    task.transaction(|tx| {
        let revision = tx.state.revision(order);
        let workspace = tx
            .state
            .runner_workspaces
            .get_mut(agent)
            .context("missing isolated source")?;
        workspace.order = Some(order.into());
        workspace.revision = revision;
        Ok(())
    })
}
fn collect(
    root: &Path,
    current: &Path,
    result: &mut BTreeMap<String, Vec<u8>>,
    total: &mut u64,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "snapshot contains a symlink"
        );
        if metadata.is_dir() {
            collect(root, &path, result, total)?;
        } else {
            let name = relative(path.strip_prefix(root)?)?;
            let content = bytes(&path)?;
            *total += content.len() as u64;
            ensure!(
                *total <= TREE_LIMIT && result.len() < 20000,
                "snapshot exceeds resource limit"
            );
            result.insert(name, content);
        }
    }
    Ok(())
}
pub fn assert_checkpointed(task: &Task, agent: &str) -> Result<()> {
    let state = task.state()?;
    let worker = state
        .runner_workspaces
        .get(agent)
        .context("isolated source missing")?;
    ensure!(
        checks::head(&worker.source)? == worker.base,
        "host code changed since checkpoint"
    );
    for (path, expected) in &worker.original {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                worker.executable.get(path).copied()
                    == Some(
                        fs::metadata(regular(&worker.snapshot, path)?)?
                            .permissions()
                            .mode()
                            & 0o111
                            != 0
                    ),
                "source mode changed since checkpoint"
            );
        }
        ensure!(
            content_hash(&bytes(&regular(&worker.snapshot, path)?)?)? == *expected,
            "snapshot source differs from checkpoint; checkpoint before validation: {path}"
        );
    }
    Ok(())
}

/// Copies a checkpoint's files from the snapshot into the source and commits
/// them, or leaves the source as it was: a half-done checkpoint (files copied,
/// the commit refused) would block every retry with edits it did not make.
fn write_checkpoint(
    source: &Path,
    snapshot: &Path,
    changed: &[String],
    files: &BTreeMap<String, Vec<u8>>,
    subject: &str,
) -> Result<()> {
    if changed.is_empty() {
        return Ok(());
    }
    ensure!(
        safe_git(source, &["var", "GIT_AUTHOR_IDENT"]).is_ok(),
        "no git author for {}: set user.name and user.email (git config --global) on this host",
        source.display()
    );
    let written = copy_and_commit(source, snapshot, changed, files, subject);
    if written.is_err() {
        undo_copy(source, changed)?;
    }
    written
}

fn copy_and_commit(
    source: &Path,
    snapshot: &Path,
    changed: &[String],
    files: &BTreeMap<String, Vec<u8>>,
    subject: &str,
) -> Result<()> {
    for path in changed {
        let destination = regular(source, path)?;
        if let Some(content) = files.get(path) {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&destination)
                    .map(|metadata| metadata.permissions().mode() & 0o666)
                    .unwrap_or(0o644);
                let executable =
                    fs::metadata(regular(snapshot, path)?)?.permissions().mode() & 0o111 != 0;
                fs::write(&destination, content)?;
                fs::set_permissions(
                    &destination,
                    fs::Permissions::from_mode(
                        mode | if executable { (mode & 0o444) >> 2 } else { 0 },
                    ),
                )?;
            }
            #[cfg(not(unix))]
            fs::write(destination, content)?;
        } else {
            fs::remove_file(destination)?;
        }
    }
    let mut arguments = vec!["add", "--"];
    arguments.extend(changed.iter().map(String::as_str));
    safe_git(source, &arguments)?;
    let mut arguments = vec![
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "--only",
        "-m",
        subject,
        "--",
    ];
    arguments.extend(changed.iter().map(String::as_str));
    safe_git(source, &arguments)?;
    Ok(())
}

/// Puts the checkpoint's paths back to the source's HEAD (the checkpoint's
/// base): files it had are restored, files it added are removed.
fn undo_copy(source: &Path, changed: &[String]) -> Result<()> {
    let mut arguments = vec!["reset", "-q", "--"];
    arguments.extend(changed.iter().map(String::as_str));
    safe_git(source, &arguments)?;
    for path in changed {
        if safe_git(source, &["cat-file", "-e", &format!("HEAD:{path}")]).is_ok() {
            safe_git(source, &["checkout", "HEAD", "--", path])?;
        } else {
            let destination = regular(source, path)?;
            if destination.exists() {
                fs::remove_file(destination)?;
            }
        }
    }
    Ok(())
}

pub fn checkpoint(task: &Task, agent: &str, summary: &str) -> Result<Vec<String>> {
    let _control = task.control_lock()?;
    let state = task.state()?;
    ensure!(!state.paused, "task is paused");
    let worker = state
        .runner_workspaces
        .get(agent)
        .context("no source snapshot")?;
    ensure!(
        !worker.readonly,
        "reviewers and planners cannot checkpoint code"
    );
    let order = state
        .open_order_of(agent)
        .context("no current assignment")?;
    ensure!(
        worker.order.as_deref() == Some(&order.id)
            && worker.revision == state.revision(&order.id)
            && state.contract_bindings.get(&order.id) == state.application.items.get(&order.step)
            && order.delivered,
        "worker assignment is stale or undelivered"
    );
    ensure!(
        checks::head(&worker.source)? == worker.base,
        "project source changed; refresh before checkpoint"
    );
    let config = state.runner.as_ref().context("runner missing")?;
    let author = state.agent(agent).context("unknown source author")?;
    ensure!(
        author.writes && author.lane.is_some(),
        "reviewed source checkpoints require an isolated implementation lane"
    );
    ensure!(
        checks::inspect(&worker.source, "HEAD", &[], &[])?
            .uncommitted
            .is_empty(),
        "project worktree contains edits outside this checkpoint"
    );
    let allowed = checks::glob_set(&order.allowed)?;
    ensure!(
        !allowed.is_empty(),
        "reviewed source requires explicit scope"
    );
    let forbidden = checks::glob_set(&order.forbidden)?;
    let mut files = BTreeMap::new();
    collect(&worker.snapshot, &worker.snapshot, &mut files, &mut 0)?;
    let build_output = ignored(
        &worker.source,
        files
            .keys()
            .filter(|path| !worker.original.contains_key(*path)),
    )?;
    let mut changed = Vec::new();
    for path in worker
        .original
        .keys()
        .chain(files.keys())
        .filter(|path| !build_output.contains(*path))
        .collect::<std::collections::BTreeSet<_>>()
    {
        let new_hash = files
            .get(path)
            .map(|content| content_hash(content))
            .transpose()?;
        #[cfg(unix)]
        let mode_changed = {
            use std::os::unix::fs::PermissionsExt;
            files.contains_key(path)
                && worker.executable.get(path).copied().unwrap_or(false)
                    != (fs::metadata(regular(&worker.snapshot, path)?)?
                        .permissions()
                        .mode()
                        & 0o111
                        != 0)
        };
        #[cfg(not(unix))]
        let mode_changed = false;
        if new_hash.as_ref() == worker.original.get(path) && !mode_changed {
            continue;
        }
        ensure!(
            source_path(path, config)? && allowed.is_match(path) && !forbidden.is_match(path),
            "checkpoint outside approved source scope: {path}"
        );
        let destination = regular(&worker.source, path)?;
        match worker.original.get(path) {
            Some(original) => ensure!(
                content_hash(&bytes(&destination)?)? == *original,
                "source changed since snapshot: {path}"
            ),
            None => ensure!(!destination.exists(), "source file already exists: {path}"),
        }
        changed.push(path.clone());
    }
    ensure!(
        !summary.trim().is_empty() && summary.len() <= 500,
        "checkpoint summary required (maximum 500 characters)"
    );
    write_checkpoint(
        &worker.source,
        &worker.snapshot,
        &changed,
        &files,
        &format!("feat: implement {}", order.step),
    )?;
    let new_base = checks::head(&worker.source)?;
    let mut original = BTreeMap::new();
    for (path, content) in files {
        if source_path(&path, config)? && !build_output.contains(&path) {
            original.insert(path, content_hash(&content)?);
        }
    }
    task.transaction(|tx| {
        let workspace = tx
            .state
            .runner_workspaces
            .get_mut(agent)
            .context("worker disappeared")?;
        workspace.base = new_base;
        workspace.original = original;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            workspace.executable = workspace
                .original
                .keys()
                .map(|path| {
                    Ok((
                        path.clone(),
                        fs::metadata(regular(&worker.source, path)?)?
                            .permissions()
                            .mode()
                            & 0o111
                            != 0,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
        }
        tx.log(
            crate::log::Kind::Review,
            Some(agent),
            Some(&order.step),
            "isolated source checkpoint validated and committed",
        );
        Ok(())
    })?;
    Ok(changed)
}
/// A container's output within `OUTPUT_LIMIT`: its start (what ran, with
/// which setup) and its end (a test runner's failures and summary), the
/// middle left out and marked.
fn drain(mut stream: impl Read, mut capture: Option<fs::File>) -> (String, Option<bool>) {
    const HEAD: usize = OUTPUT_LIMIT / 8;
    let mut head = Vec::new();
    let mut tail = std::collections::VecDeque::new();
    let mut left_out = 0usize;
    let mut buffer = [0u8; 4096];
    let mut captured = 0;
    let mut truncated = capture.as_ref().map(|_| false);
    loop {
        let size = match stream.read(&mut buffer) {
            Ok(size) => size,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                truncated = None;
                break;
            }
        };
        if size == 0 {
            break;
        }
        let mut chunk = &buffer[..size];
        if let Some(file) = &mut capture {
            let kept = size.min(RESULT_LIMIT - captured);
            if file.write_all(&chunk[..kept]).is_err() {
                // Keep draining so an artifact failure cannot change the command.
                capture = None;
                truncated = None;
            } else {
                captured += kept;
                truncated = Some(truncated == Some(true) || kept < size);
            }
        }
        let into_head = chunk.len().min(HEAD.saturating_sub(head.len()));
        head.extend_from_slice(&chunk[..into_head]);
        chunk = &chunk[into_head..];
        tail.extend(chunk);
        let over = tail.len().saturating_sub(OUTPUT_LIMIT - HEAD);
        tail.drain(..over);
        left_out += over;
    }
    let mut text = String::from_utf8_lossy(&head).into_owned();
    if left_out > 0 {
        text.push_str(&format!("\n[… {left_out} bytes left out …]\n"));
    }
    text.push_str(&String::from_utf8_lossy(tail.make_contiguous()));
    (text, truncated)
}
fn docker_arguments(
    config: &Configuration,
    source: &Path,
    readonly: bool,
    name: &str,
    command: &str,
) -> Result<Vec<String>> {
    validate_configuration(config)?;
    let source = source.canonicalize()?;
    let source = source.to_str().context("invalid snapshot path")?;
    ensure!(!source.contains(','), "Docker mount path contains comma");
    let mount = format!(
        "type=bind,src={source},dst=/workspace{}",
        if readonly { ",readonly" } else { "" }
    );
    // The daemon is rootless (`probe` refuses any other): its container root
    // is the user who owns the snapshot, without capabilities or a way to
    // gain any. A container uid of the user's own would map to one of their
    // subordinate ids instead, which cannot write the snapshot.
    let user = "0:0".to_string();
    let memory_mb = config.memory_mb.unwrap_or(MEMORY_LIMIT_MB);
    let mut arguments: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--pull=never".into(),
        "--name".into(),
        name.into(),
        "--network=none".into(),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--pids-limit=128".into(),
        format!("--memory={memory_mb}m"),
        // Equal to the memory: no swap on top of it.
        format!("--memory-swap={memory_mb}m"),
        format!("--cpus={}", config.cpus.unwrap_or(1)),
        "--user".into(),
        user,
        "--mount".into(),
        mount,
    ];
    // Read-only image folders over the workspace mount, never under it.
    for dependency in &config.image_mounts {
        arguments.extend([
            "--mount".into(),
            format!(
                "type=image,source={},target=/workspace/{},image-subpath={}",
                config.image,
                dependency.target,
                dependency.path.trim_start_matches('/')
            ),
        ]);
    }
    arguments.extend([
        "--tmpfs=/tmp:rw,noexec,nosuid,size=64m".into(),
        "--workdir=/workspace".into(),
        "--env=HOME=/tmp".into(),
        "--entrypoint=/bin/sh".into(),
        config.image.clone(),
        "-lc".into(),
        command.into(),
    ]);
    Ok(arguments)
}
pub fn run_container(
    config: &Configuration,
    source: &Path,
    readonly: bool,
    command: &str,
) -> Result<Output> {
    run_container_artifacts(config, source, readonly, command, None)
}

pub(crate) fn run_container_artifacts(
    config: &Configuration,
    source: &Path,
    readonly: bool,
    command: &str,
    artifacts: Option<&Path>,
) -> Result<Output> {
    Ok(run_container_capture(config, source, readonly, command, artifacts, None, None)?.0)
}

/// The containers this host runs at once at `config`'s caps: as many as its
/// CPUs and three quarters of its memory hold (the rest is the host's), at
/// least one. Total memory, not free: the running containers' own use must
/// not shrink the count below the slots they hold.
pub fn host_slots(config: &Configuration) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let by_cpu = cpus / config.cpus.unwrap_or(1).max(1) as usize;
    let memory_mb = u64::from(config.memory_mb.unwrap_or(MEMORY_LIMIT_MB).max(1));
    let by_memory = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| usable_memory_mb(&text))
        .map_or(usize::MAX, |usable| {
            usize::try_from(usable / memory_mb).unwrap_or(usize::MAX)
        });
    by_cpu.min(by_memory).max(1)
}

/// Three quarters of `MemTotal` of /proc/meminfo, in MB.
fn usable_memory_mb(meminfo: &str) -> Option<u64> {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| {
            rest.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|kilobytes| kilobytes / 1024 * 3 / 4)
}

/// A container slot of this host, held until it is dropped: a lock on one of
/// a few files, which the system releases even when the holder dies.
struct Slot {
    _lock: fs::File,
}

/// A free slot of `slots` in `folder`, if any. Each slot is a file of its
/// own: nothing shared to count, so no counter to go wrong.
fn free_slot(folder: &Path, slots: usize) -> Result<Option<Slot>> {
    fs::create_dir_all(folder)?;
    for slot in 0..slots {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.join(format!("slot-{slot}")))?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(Some(Slot { _lock: file })),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {}
            Err(error) => {
                return Err(anyhow::Error::from(error).context(format!(
                    "cannot lock the container slot {}",
                    folder.join(format!("slot-{slot}")).display()
                )));
            }
        }
    }
    Ok(None)
}

/// Waits for a container slot: on a busy host a check queues rather than
/// starving the others into timeouts.
fn take_slot(config: &Configuration) -> Result<Slot> {
    let folder = crate::home::Home::from_env()?.root().join("runner-slots");
    loop {
        if let Some(slot) = free_slot(&folder, host_slots(config))? {
            return Ok(slot);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn run_container_capture(
    config: &Configuration,
    source: &Path,
    readonly: bool,
    command: &str,
    artifacts: Option<&Path>,
    scratch: Option<&Path>,
    capture: Option<(fs::File, fs::File)>,
) -> Result<(Output, Option<[bool; 2]>)> {
    ensure!(command.len() <= 64 * 1024, "command too large");
    let name = format!("devagentics-{}", &token()?[..20]);
    let mut arguments = docker_arguments(config, source, readonly, &name, command)?;
    if let Some(folder) = scratch {
        mount_scratch(&mut arguments, folder)?;
    }
    if let Some(artifacts) = artifacts {
        let artifacts = artifacts.canonicalize()?;
        let path = artifacts.to_str().context("invalid isolated result path")?;
        ensure!(!path.contains(','), "invalid Docker artifact mount");
        #[cfg(unix)]
        if unsafe { libc::geteuid() } == 0 {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&artifacts, fs::Permissions::from_mode(0o777))?;
        }
        arguments.splice(
            2..2,
            [
                "--mount".into(),
                format!("type=bind,src={path},dst=/artifacts"),
            ],
        );
    }
    let endpoint = configured_endpoint(config)?;
    // Held until the container is gone; its timeout starts once it runs.
    let _slot = take_slot(config)?;
    let mut child = docker(&endpoint)
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().context("container stdout")?;
    let stderr = child.stderr.take().context("container stderr")?;
    let (stdout_file, stderr_file) =
        capture.map_or((None, None), |(out, err)| (Some(out), Some(err)));
    let stdout = std::thread::spawn(move || drain(stdout, stdout_file));
    let stderr = std::thread::spawn(move || drain(stderr, stderr_file));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(config.timeout_seconds) {
            timed_out = true;
            let _ = child.kill();
            let _ = docker(&endpoint)
                .args(["rm", "--force", &name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (stdout, stdout_truncated) = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("container output reader failed"))?;
    let (stderr, stderr_truncated) = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("container error reader failed"))?;
    Ok((
        Output {
            success: status.success() && !timed_out,
            stdout,
            stderr,
            timed_out,
            exit_code: status.code(),
        },
        stdout_truncated
            .zip(stderr_truncated)
            .map(|(out, err)| [out, err]),
    ))
}
pub fn execute(task: &Task, agent: &str, command: &str) -> Result<Output> {
    Ok(execute_capture(task, agent, command, None)?.0)
}

fn execute_capture(
    task: &Task,
    agent: &str,
    command: &str,
    capture: Option<(fs::File, fs::File)>,
) -> Result<(Output, Option<[bool; 2]>)> {
    let state = task.state()?;
    ensure!(!state.paused, "task is paused");
    let workspace = state
        .runner_workspaces
        .get(agent)
        .context("worker source missing")?;
    let config = state
        .runner
        .as_ref()
        .context("runner missing")?
        .for_lane(container_lane(&state, agent).as_deref());
    let scratch = scratch_for(task, agent)?;
    run_container_capture(
        &config,
        &workspace.snapshot,
        workspace.readonly,
        command,
        None,
        scratch.as_deref(),
        capture,
    )
}

fn result_directory(task: &Task, agent: &str) -> Result<PathBuf> {
    let state = task.state()?;
    let generation = state
        .runner_identities
        .get(agent)
        .and_then(|path| path.file_name())
        .context("worker session identity missing")?;
    Ok(task
        .dir()
        .join("workers")
        .join(agent)
        .join("results")
        .join(generation))
}

fn preview(text: &str) -> String {
    if text.chars().count() <= PREVIEW_LIMIT {
        return text.to_owned();
    }
    let head: String = text.chars().take(PREVIEW_LIMIT / 4).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(PREVIEW_LIMIT * 3 / 4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}\n[… output omitted …]\n{tail}")
}

fn failure_diagnostics(output: &str, shown: &str) -> Vec<String> {
    let is_diagnostic = |line: &str| {
        let lowered = line.to_ascii_lowercase();
        [
            "error:",
            "fatal:",
            "fatal error:",
            "fail:",
            "fail ",
            "failed:",
            "failed ",
            "traceback ",
            "assertionerror:",
        ]
        .iter()
        .any(|prefix| lowered.starts_with(prefix))
            || lowered.contains(": error:")
            || (lowered.starts_with("thread '") && lowered.contains("panicked at"))
    };
    let mut matches = output
        .lines()
        .map(str::trim)
        .filter(|line| is_diagnostic(line) && !shown.contains(line));
    let first: Vec<_> = matches.by_ref().take(2).collect();
    let last: Vec<_> = matches.rev().take(2).collect();
    let mut selected = Vec::new();
    for line in first.into_iter().chain(last.into_iter().rev()) {
        let snippet: String = line.chars().take(DIAGNOSTIC_LINE_LIMIT).collect();
        if !selected.contains(&snippet) {
            selected.push(snippet);
        }
    }
    selected
}

fn captured_diagnostics(path: &Path, retained: &str, shown: &str) -> Vec<String> {
    match fs::read(path) {
        Ok(bytes) => failure_diagnostics(&String::from_utf8_lossy(&bytes), shown),
        Err(error) => {
            eprintln!("devagentics execution diagnostic capture: {error}");
            failure_diagnostics(retained, shown)
        }
    }
}

pub fn execute_compact(task: &Task, agent: &str, command: &str) -> Result<serde_json::Value> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let root = result_directory(task, agent)?;
    let id = token()?;
    let staging = root.join(format!("{id}-pending"));
    let reserved = {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(root.join("capture.lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        // Count unfinished captures too, so interrupted or concurrent commands
        // cannot push this worker session past its allocated disk budget.
        let count = fs::read_dir(&root)?.try_fold(0usize, |count, entry| {
            Ok::<_, std::io::Error>(count + usize::from(entry?.file_type()?.is_dir()))
        })?;
        if count < RESULTS_PER_SESSION {
            fs::DirBuilder::new().mode(0o700).create(&staging)?;
        }
        count < RESULTS_PER_SESSION
    };
    if !reserved {
        let mut value = serde_json::to_value(execute(task, agent, command)?)?;
        value["result_id"] = serde_json::Value::Null;
        value["capture_notice"] =
            "result limit reached; returning the full-mode bounded output".into();
        return Ok(value);
    }
    let open = |name| {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(staging.join(name))
    };
    let run = (|| {
        let files = (open("stdout")?, open("stderr")?);
        execute_capture(task, agent, command, Some(files))
    })();
    let publish = |limits: [bool; 2]| -> Result<()> {
        fs::write(staging.join("metadata.json"), serde_json::to_vec(&limits)?)?;
        fs::rename(&staging, root.join(&id))?;
        Ok(())
    };
    let (result, limits) = match run {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let stdout = preview(&result.stdout);
    let stderr = preview(&result.stderr);
    let diagnostics = (!result.success).then(|| {
        let find = |stream, retained, shown| {
            if limits.is_some() {
                captured_diagnostics(&staging.join(stream), retained, shown)
            } else {
                failure_diagnostics(retained, shown)
            }
        };
        serde_json::json!({
            "stdout": find("stdout", &result.stdout, &stdout),
            "stderr": find("stderr", &result.stderr, &stderr),
        })
    });
    let result_id = limits.and_then(|limits| match publish(limits) {
        Ok(()) => Some(id.as_str()),
        Err(error) => {
            eprintln!("devagentics execution artifact: {error:#}");
            None
        }
    });
    if result_id.is_none() {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(serde_json::json!({
        "success": result.success, "timed_out": result.timed_out, "exit_code": result.exit_code,
        "stdout": stdout, "stderr": stderr, "failure_diagnostics": diagnostics,
        "result_id": result_id,
        "capture_truncated": result_id.as_ref().and(limits).map(|[stdout, stderr]| serde_json::json!({"stdout":stdout, "stderr":stderr})),
    }))
}

pub fn read_result(
    task: &Task,
    agent: &str,
    id: &str,
    stream: &str,
    offset: u64,
) -> Result<serde_json::Value> {
    use std::io::{Seek, SeekFrom};

    ensure!(
        id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid result ID"
    );
    let index = match stream {
        "stdout" => 0,
        "stderr" => 1,
        _ => bail!("result stream must be stdout or stderr"),
    };
    let directory = result_directory(task, agent)?.join(id);
    let metadata = fs::read(directory.join("metadata.json"))
        .context("result is unavailable in this worker session")?;
    let limits: [bool; 2] = serde_json::from_slice(&metadata)?;
    let mut file = fs::File::open(directory.join(stream))?;
    let size = file.metadata()?.len();
    ensure!(
        offset <= size,
        "offset is past the end of the captured output"
    );
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(RESULT_PAGE as u64).read_to_end(&mut bytes)?;
    if offset + (bytes.len() as u64) < size
        && let Err(error) = std::str::from_utf8(&bytes)
        && error.error_len().is_none()
    {
        bytes.truncate(error.valid_up_to());
    }
    let next = offset + bytes.len() as u64;
    Ok(serde_json::json!({
        "result_id": id, "stream": stream, "offset": offset,
        "text": String::from_utf8_lossy(&bytes),
        "next_offset": (next < size).then_some(next),
        "captured_bytes": size,
        "truncated": limits[index],
    }))
}

/// The lane whose image `agent`'s containers use: its own, or for a reviewer
/// the lane of the work it reviews now. None for the main image.
pub fn container_lane(state: &crate::state::State, agent: &str) -> Option<String> {
    if let Some(lane) = state.agent(agent).and_then(|found| found.lane.clone()) {
        return Some(lane);
    }
    let review = state
        .reviews
        .iter()
        .rev()
        .find(|review| review.reviewer == agent && review.verdict.is_none())?;
    lane_of_order(state, &review.order)
}

/// The lane of the agent an order was given to.
pub fn lane_of_order(state: &crate::state::State, order: &str) -> Option<String> {
    let order = state.orders.iter().find(|found| found.id == order)?;
    state.agent(&order.agent)?.lane.clone()
}

/// Git plumbing must not execute repository hooks, filters or fsmonitor programs.
pub(crate) fn safe_git_output(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let configured = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "config",
            "--name-only",
            "--get-regexp",
            r"^filter\..*\.(clean|smudge|process|required)$",
        ])
        .output()?;
    ensure!(
        configured.status.success() || configured.status.code() == Some(1),
        "cannot inspect repository filter policy"
    );
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "commit.gpgsign=false",
    ]);
    for key in String::from_utf8_lossy(&configured.stdout).lines() {
        let value = if key.ends_with(".required") {
            "false"
        } else if key.ends_with(".process") {
            ""
        } else {
            "/bin/cat"
        };
        command.args(["-c", &format!("{key}={value}")]);
    }
    Ok(command.args(args).output()?)
}

pub(crate) fn safe_git(root: &Path, args: &[&str]) -> Result<String> {
    let result = safe_git_output(root, args)?;
    ensure!(
        result.status.success(),
        "controlled Git operation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().into())
}

/// The new snapshot files the source repository ignores (build output such
/// as `web/.next/`): a worker's checks make them, and they are never source.
fn ignored<'a>(
    source: &Path,
    paths: impl Iterator<Item = &'a String>,
) -> Result<std::collections::BTreeSet<String>> {
    let paths: Vec<&str> = paths.map(String::as_str).collect();
    let mut found = std::collections::BTreeSet::new();
    for chunk in paths.chunks(200) {
        let mut arguments = vec!["check-ignore", "--"];
        arguments.extend(chunk);
        let result = safe_git_output(source, &arguments)?;
        // 1: none of them is ignored.
        ensure!(
            matches!(result.status.code(), Some(0 | 1)),
            "controlled Git operation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        found.extend(
            String::from_utf8(result.stdout)?
                .lines()
                .map(str::to_string),
        );
    }
    Ok(found)
}

/// Prerequisites run before dispatch in their own source generation, never the worker's edits.
pub(crate) fn result_bytes(path: &Path) -> Result<Vec<u8>> {
    bytes(path)
}

pub fn preflight(task: &Task, item_id: &str, source: &Path) -> Result<()> {
    let state = task.state()?;
    let item = state
        .application
        .items
        .get(item_id)
        .context("unknown contract")?;
    let configuration = task.config()?;
    let definitions = &configuration
        .check_profiles
        .get(&item.repo)
        .context("named checks not configured")?
        .for_lane(Some(&item.lane));
    let fingerprint = content_hash(&serde_json::to_vec(&(
        item,
        definitions,
        &state.runner,
        checks::head(source)?,
    ))?)?;
    if let Some(previous) = state
        .environment_preflight
        .get(item_id)
        .filter(|previous| previous.fingerprint == fingerprint)
    {
        ensure!(
            previous.failure.is_none(),
            "unchanged environment remains blocked: {}",
            previous.failure.as_deref().unwrap_or("unknown")
        );
        return Ok(());
    }
    let worker = format!("preflight-{}", &fingerprint[..16]);
    prepare(task, &worker, source, false)?;
    let directory = task.dir().join("preflight-checks").join(&fingerprint);
    fs::create_dir_all(&directory)?;
    let run = || -> Result<Option<String>> {
        for (index, name) in item.profiles.iter().enumerate() {
            let mut profile = definitions
                .checks
                .get(name)
                .context("profile missing")?
                .clone();
            profile.command = "true".into();
            profile.test_evidence = false;
            let environment = definitions
                .environments
                .get(&profile.environment)
                .context("environment missing")?;
            let phase = crate::check_profiles::run_container_profile(
                task,
                crate::check_profiles::Container {
                    agent: &worker,
                    lane: Some(&item.lane),
                    targets: &[],
                },
                name,
                &profile,
                environment,
                &directory,
                index,
            )?;
            if phase.outcome != crate::check_profiles::Outcome::Passed {
                return Ok(Some(format!(
                    "{}: {}; evidence {}",
                    name,
                    phase.diagnostic,
                    phase.log.display()
                )));
            }
        }
        Ok(None)
    };
    let failure = run();
    // Its evidence is in `directory`; its source is never used again.
    forget(task, &worker)?;
    let failure = failure?;
    task.transaction(|tx| {
        tx.state.environment_preflight.insert(
            item_id.into(),
            Preflight {
                fingerprint,
                failure: failure.clone(),
            },
        );
        Ok(())
    })?;
    ensure!(
        failure.is_none(),
        "environment preflight blocked assignment: {}",
        failure.unwrap_or_default()
    );
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preflight {
    pub fingerprint: String,
    pub failure: Option<String>,
}

/// Coordinator discovery is read-only and never invokes project scripts.
pub fn images() -> Result<serde_json::Value> {
    let endpoint = user_endpoint(runtime_dir().as_deref())?;
    let output = docker(&endpoint)
        .args(["image", "ls", "--no-trunc", "--format", "{{json .}} "])
        .output()?;
    ensure!(
        output.status.success(),
        "local Docker image discovery failed"
    );
    Ok(serde_json::json!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(100)
            .map(|line| serde_json::from_str::<serde_json::Value>(line.trim()))
            .collect::<std::result::Result<Vec<_>, _>>()?
    ))
}
/// The overview fits the leader's tools (80 KB) for a repo of any size: the
/// first files and the start of each document; the leader reads more through
/// a planner.
const OVERVIEW_FILES: usize = 600;
const OVERVIEW_DOCUMENT_BYTES: usize = 8 * 1024;

pub fn overview(task: &Task) -> Result<serde_json::Value> {
    let mut repositories = Vec::new();
    for repo in task.config()?.repos {
        let files = checks::git(&repo.worktree, &["ls-files"])?;
        let mut documents = BTreeMap::new();
        for path in [
            "AGENTS.md",
            "CLAUDE.md",
            "README.md",
            "package.json",
            "composer.json",
            "Cargo.toml",
        ] {
            let source = regular(&repo.worktree, path)?;
            if source.is_file() {
                let data = bytes(&source)?;
                documents.insert(
                    path,
                    String::from_utf8_lossy(&data[..data.len().min(OVERVIEW_DOCUMENT_BYTES)])
                        .into_owned(),
                );
            }
        }
        repositories.push(serde_json::json!({"name":repo.name,"tracked_files_total":files.lines().count(),"tracked_files":files.lines().take(OVERVIEW_FILES).collect::<Vec<_>>(),"project_context":documents}));
    }
    Ok(serde_json::json!(repositories))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn a_host_runs_as_many_containers_as_its_free_memory_and_cpus_hold() {
        let meminfo = "MemTotal:       16777216 kB\nMemAvailable:    1048576 kB\n";
        assert_eq!(
            usable_memory_mb(meminfo),
            Some(12_288),
            "three quarters of the total, whatever is free while checks run"
        );
        assert_eq!(usable_memory_mb("MemAvailable: 1 kB\n"), None);
        let huge = Configuration {
            memory_mb: Some(MEMORY_LIMIT_MB),
            cpus: Some(u32::MAX),
            ..configuration()
        };
        assert_eq!(host_slots(&huge), 1, "never fewer than one");
    }

    #[test]
    fn a_container_waits_for_a_free_slot_and_gets_it_when_one_is_released() {
        let folder = tempfile::TempDir::new().unwrap();
        let first = free_slot(folder.path(), 2).unwrap().unwrap();
        let second = free_slot(folder.path(), 2).unwrap().unwrap();
        assert!(
            free_slot(folder.path(), 2).unwrap().is_none(),
            "both slots busy"
        );
        drop(first);
        // A process forked by another test while the file was open shares its
        // lock until it execs, so the slot frees a moment later; take_slot
        // polls for the same reason.
        let freed = (0..100).any(|_| {
            free_slot(folder.path(), 2).unwrap().is_some() || {
                std::thread::sleep(Duration::from_millis(20));
                false
            }
        });
        assert!(freed, "a released slot is free");
        drop(second);
    }

    #[test]
    fn compact_failure_surfaces_buried_diagnostics_and_skips_visible_duplicates() {
        let output = format!(
            "error: already visible\n{}\nFAIL: test_counts\nTraceback (most recent call last):\nAssertionError: 41 != 42\nFAILED (failures=1)\n{}\n",
            "setup noise\n".repeat(400),
            "cleanup noise\n".repeat(400),
        );
        let shown = preview(&output);
        assert!(!shown.contains("41 != 42"));
        assert_eq!(
            failure_diagnostics(&output, &shown),
            [
                "FAIL: test_counts",
                "Traceback (most recent call last):",
                "AssertionError: 41 != 42",
                "FAILED (failures=1)"
            ],
        );
    }

    #[test]
    fn capture_keeps_output_beyond_the_preview_up_to_its_disk_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("capture");
        let mut output = vec![b'a'; RESULT_LIMIT];
        output[OUTPUT_LIMIT] = b'b';
        output.extend_from_slice(b"final error");
        let (bounded, truncated) = drain(output.as_slice(), Some(fs::File::create(&path).unwrap()));
        assert_eq!(truncated, Some(true));
        let saved = fs::read(path).unwrap();
        assert_eq!(saved.len(), RESULT_LIMIT);
        assert_eq!(saved[OUTPUT_LIMIT], b'b');
        assert!(preview(&bounded).ends_with("final error"));
    }

    #[test]
    fn captured_results_are_paged_and_bound_to_the_worker_generation() {
        let directory = tempfile::tempdir().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state
                .runner_identities
                .insert("a".into(), "identity-a".into());
            tx.state
                .runner_identities
                .insert("b".into(), "identity-b".into());
            Ok(())
        })
        .unwrap();
        let id = "a".repeat(64);
        let result = result_directory(&task, "a").unwrap().join(&id);
        fs::create_dir_all(&result).unwrap();
        fs::write(result.join("metadata.json"), "[false,true]").unwrap();
        fs::write(
            result.join("stdout"),
            format!("{}€second", "a".repeat(RESULT_PAGE - 1)),
        )
        .unwrap();
        let first = read_result(&task, "a", &id, "stdout", 0).unwrap();
        assert_eq!(first["next_offset"], RESULT_PAGE - 1);
        let second = read_result(&task, "a", &id, "stdout", (RESULT_PAGE - 1) as u64).unwrap();
        assert_eq!(second["text"], "€second");
        assert!(second["next_offset"].is_null());
        assert!(read_result(&task, "b", &id, "stdout", 0).is_err());
        assert!(read_result(&task, "a", "../metadata.json", "stdout", 0).is_err());
        task.transaction(|tx| {
            tx.state
                .runner_identities
                .insert("a".into(), "identity-new".into());
            Ok(())
        })
        .unwrap();
        assert!(read_result(&task, "a", &id, "stdout", 0).is_err());
    }

    // A test runner prints its failures and summary last: a long suite's
    // output keeps its end, not only its first 64 KiB.
    #[test]
    fn long_output_keeps_its_start_and_its_end() {
        let mut text = String::from("back-check: vendor matches composer.lock\n");
        while text.len() < 5 * OUTPUT_LIMIT {
            text.push_str("  ✓ it passes a unit test                     0.01s\n");
        }
        text.push_str(
            " FAIL  Tests\\Feature\\SystemFlowRepositoryTest\nTests: 18 failed, 900 passed\n",
        );
        let (kept, _) = drain(text.as_bytes(), None);
        assert!(
            kept.starts_with("back-check: vendor matches"),
            "{}",
            &kept[..80]
        );
        assert!(
            kept.ends_with("Tests: 18 failed, 900 passed\n"),
            "the end was cut"
        );
        assert!(kept.contains("bytes left out"), "the cut is not marked");
        assert!(kept.len() <= OUTPUT_LIMIT + 100, "{}", kept.len());
        assert_eq!(drain("short\n".as_bytes(), None).0, "short\n");
    }

    #[test]
    fn the_runner_uses_the_pinned_daemon_or_the_users_rootless_one() {
        let mut config = configuration();
        assert_eq!(
            endpoint(&config, Some("/run/user/1001")).unwrap(),
            "unix:///run/user/1001/docker.sock"
        );
        assert!(
            endpoint(&config, None).is_err(),
            "no runtime folder: no rootless socket to default to"
        );
        config.docker_host = Some("unix:///home/iva/docker.sock".into());
        assert_eq!(
            endpoint(&config, Some("/run/user/1001")).unwrap(),
            "unix:///home/iva/docker.sock"
        );
        config.docker_host = Some("tcp://10.0.0.5:2375".into());
        assert!(
            validate_configuration(&config).is_err(),
            "only a local socket"
        );
    }

    fn with_dependencies() -> Configuration {
        Configuration {
            image_mounts: vec![ImageMount {
                path: "/node_modules".into(),
                target: "web/node_modules".into(),
            }],
            ..configuration()
        }
    }

    #[test]
    fn the_image_provides_dependency_folders_read_only_inside_the_workspace() {
        let root = tempfile::TempDir::new().unwrap();
        let config = with_dependencies();
        validate_configuration(&config).unwrap();
        let args = docker_arguments(&config, root.path(), false, "n", "true").unwrap();
        let workspace = args
            .iter()
            .position(|argument| argument.contains("dst=/workspace"))
            .unwrap();
        let dependencies = args
            .iter()
            .position(|argument| {
                argument
                    == &format!(
                        "type=image,source={},target=/workspace/web/node_modules,image-subpath=node_modules",
                        config.image
                    )
            })
            .expect("an image mount of the dependencies");
        assert!(
            workspace < dependencies,
            "mounted over the workspace, not under it"
        );
        assert_eq!(args[dependencies - 1], "--mount");

        for (path, target) in [
            ("node_modules", "web/node_modules"),
            ("/node_modules/../etc", "web/node_modules"),
            ("/node_modules", "../outside"),
            ("/node_modules", "/abs"),
            ("/node,modules", "web/node_modules"),
        ] {
            let config = Configuration {
                image_mounts: vec![ImageMount {
                    path: path.into(),
                    target: target.into(),
                }],
                ..configuration()
            };
            assert!(
                validate_configuration(&config).is_err(),
                "{path} -> {target}"
            );
        }
    }

    #[test]
    fn a_container_gets_the_configured_memory_and_cpus_within_bounds() {
        let root = tempfile::TempDir::new().unwrap();
        let defaults = docker_arguments(&configuration(), root.path(), false, "n", "true").unwrap();
        assert!(
            defaults.contains(&"--memory=4096m".into())
                && defaults.contains(&"--memory-swap=4096m".into())
                && defaults.contains(&"--cpus=1".into()),
            "every container is held to 4 GB, swap included: {defaults:?}"
        );
        let smaller = Configuration {
            memory_mb: Some(2048),
            cpus: Some(2),
            ..configuration()
        };
        let args = docker_arguments(&smaller, root.path(), false, "n", "true").unwrap();
        assert!(
            args.contains(&"--memory=2048m".into())
                && args.contains(&"--memory-swap=2048m".into())
                && args.contains(&"--cpus=2".into())
        );
        for (memory_mb, cpus) in [
            (Some(64), None),
            (Some(4097), None),
            (Some(16_384), None),
            (None, Some(0)),
            (None, Some(8)),
        ] {
            let config = Configuration {
                memory_mb,
                cpus,
                ..configuration()
            };
            assert!(
                validate_configuration(&config).is_err(),
                "{memory_mb:?} {cpus:?}"
            );
        }
    }

    #[test]
    fn mount_points_exist_before_a_read_only_workspace_is_mounted() {
        let root = tempfile::TempDir::new().unwrap();
        mount_points(root.path(), &with_dependencies()).unwrap();
        assert!(root.path().join("web/node_modules").is_dir());
        mount_points(root.path(), &with_dependencies()).unwrap();
    }

    #[test]
    fn a_snapshot_gets_the_mount_points_and_no_source_is_hidden_under_them() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        crate::checks::tests::commit_file(repo.path(), "src/app.rs", "app\n", "app");
        task.transaction(|tx| {
            tx.state.runner = Some(Configuration {
                image_mounts: vec![ImageMount {
                    path: "/node_modules".into(),
                    target: "src/deps".into(),
                }],
                ..configuration()
            });
            Ok(())
        })
        .unwrap();
        prepare(&task, "worker-1", repo.path(), false).unwrap();
        let snapshot = task.state().unwrap().runner_workspaces["worker-1"]
            .snapshot
            .clone();
        assert!(snapshot.join("src/deps").is_dir());
        assert!(snapshot.join("src/app.rs").is_file());

        crate::checks::tests::commit_file(
            repo.path(),
            "src/deps/left.js",
            "x\n",
            "a tracked file under the mount",
        );
        let error = prepare(&task, "worker-2", repo.path(), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("src/deps"), "{error}");
    }

    /// A task with a runner, and the folder a worker's source snapshots are
    /// written to.
    fn with_runner(directory: &tempfile::TempDir) -> (Task, tempfile::TempDir) {
        let (_home, _devws, task, repo) = crate::board::tests::started(directory);
        task.transaction(|tx| {
            tx.state.runner = Some(configuration());
            Ok(())
        })
        .unwrap();
        (task, repo)
    }

    fn snapshots(task: &Task, worker: &str) -> usize {
        fs::read_dir(task.dir().join("workers").join(worker))
            .map(|entries| {
                entries
                    .filter(|entry| {
                        entry
                            .as_ref()
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("source-")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn a_forgotten_worker_leaves_no_source_in_the_state_or_on_disk() {
        let directory = tempfile::TempDir::new().unwrap();
        let (task, repo) = with_runner(&directory);
        prepare(&task, "candidate-abc", repo.path(), false).unwrap();
        prepare(&task, "candidate-abc", repo.path(), false).unwrap();
        let control = task.dir().join("workers/candidate-abc/control");
        fs::create_dir_all(&control).unwrap();
        assert_eq!(snapshots(&task, "candidate-abc"), 2);

        forget(&task, "candidate-abc").unwrap();

        let state = task.state().unwrap();
        assert!(!state.runner_workspaces.contains_key("candidate-abc"));
        assert!(!state.runner_identities.contains_key("candidate-abc"));
        assert_eq!(
            snapshots(&task, "candidate-abc"),
            0,
            "every generation goes"
        );
        assert!(control.is_dir(), "its turns stay, for why it ended");
        forget(&task, "candidate-abc").unwrap();
    }

    #[test]
    fn pruning_keeps_the_sources_of_agents_that_can_still_run() {
        let directory = tempfile::TempDir::new().unwrap();
        let (task, repo) = with_runner(&directory);
        let agent = |id: &str, live: bool, lost: bool, unavailable: bool| crate::state::Agent {
            id: id.into(),
            role: "executor".into(),
            agent: "claude".into(),
            repo: "app".into(),
            session: String::new(),
            pane: String::new(),
            writes: true,
            group: None,
            lane: None,
            seat: 0,
            live,
            unavailable,
            ready: true,
            lost,
            usage: None,
        };
        task.transaction(|tx| {
            tx.state.agents = vec![
                agent("hammer-1", true, false, false),
                agent("hammer-2", false, true, false),
                agent("hammer-3", false, false, true),
                agent("hammer-4", false, false, false),
                agent("hammer-5", false, false, false),
                agent("hammer-6", false, false, false),
            ];
            tx.state.restart_on_next_account.insert("hammer-5".into());
            Ok(())
        })
        .unwrap();
        for worker in [
            "hammer-1",
            "hammer-2",
            "hammer-3",
            "hammer-4",
            "hammer-5",
            "hammer-6",
            "candidate-abc",
            "preflight-0123456789abcdef",
        ] {
            prepare(&task, worker, repo.path(), false).unwrap();
        }

        let forgotten = prune(&task).unwrap();

        assert_eq!(
            forgotten,
            [
                "candidate-abc",
                "hammer-4",
                "hammer-6",
                "preflight-0123456789abcdef"
            ]
        );
        let kept: Vec<_> = task
            .state()
            .unwrap()
            .runner_workspaces
            .into_keys()
            .collect();
        assert_eq!(kept, ["hammer-1", "hammer-2", "hammer-3", "hammer-5"]);
        assert!(prune(&task).unwrap().is_empty(), "nothing left to prune");
    }

    #[test]
    fn a_read_only_worker_keeps_a_scratch_folder_across_its_commands() {
        // Each command runs in a fresh container whose /tmp goes with it: a
        // planner proving its plan rebuilt its prototype copy every command.
        let directory = tempfile::TempDir::new().unwrap();
        let (task, repo) = with_runner(&directory);
        prepare(&task, "planner-1", repo.path(), true).unwrap();
        prepare(&task, "hammer-1", repo.path(), false).unwrap();

        let folder = scratch_for(&task, "planner-1").unwrap().unwrap();
        assert!(folder.is_dir());
        assert!(
            scratch_for(&task, "hammer-1").unwrap().is_none(),
            "a writer writes its own snapshot"
        );

        let mut arguments = vec!["run".to_string(), "--rm".into(), "image".into()];
        mount_scratch(&mut arguments, &folder).unwrap();
        let mount = format!(
            "type=bind,src={},dst=/scratch",
            folder.canonicalize().unwrap().display()
        );
        assert_eq!(
            arguments,
            [
                "run".to_string(),
                "--rm".into(),
                "--mount".into(),
                mount,
                "--env=DEVAGENTICS_SCRATCH=/scratch".into(),
                "image".into()
            ]
        );

        fs::write(folder.join("prototype.txt"), "kept").unwrap();
        bind_order(&task, "planner-1", "o1").unwrap();
        assert!(
            !folder.join("prototype.txt").exists(),
            "a new order starts with an empty folder"
        );
        // The order's first command makes it again.
        assert_eq!(
            scratch_for(&task, "planner-1").unwrap(),
            Some(folder.clone())
        );
        fs::write(folder.join("prototype.txt"), "kept").unwrap();
        forget(&task, "planner-1").unwrap();
        assert!(!folder.exists(), "a forgotten worker's folder goes");
    }

    #[test]
    fn binding_an_order_keeps_the_worker_identity_its_panel_checks() {
        let directory = tempfile::TempDir::new().unwrap();
        let (task, repo) = with_runner(&directory);
        let identity = prepare(&task, "hammer-1", repo.path(), false).unwrap();
        let identities = || {
            fs::read_dir(task.dir().join("workers/hammer-1"))
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("identity-")
                })
                .count()
        };

        bind_order(&task, "hammer-1", "o7").unwrap();

        let state = task.state().unwrap();
        assert_eq!(state.runner_identities["hammer-1"], identity);
        assert_eq!(identities(), 1, "no second identity is issued");
        assert_eq!(
            state.runner_workspaces["hammer-1"].order.as_deref(),
            Some("o7")
        );
    }

    #[test]
    fn a_new_source_keeps_the_worker_identity_its_panel_checks() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state.runner = Some(configuration());
            Ok(())
        })
        .unwrap();
        let identity = prepare(&task, "reviewer-1", repo.path(), true).unwrap();
        let first = task.state().unwrap().runner_workspaces["reviewer-1"]
            .snapshot
            .clone();
        let identities = |task: &Task| {
            fs::read_dir(task.dir().join("workers/reviewer-1"))
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("identity-")
                })
                .count()
        };

        replace_source(&task, "reviewer-1", repo.path(), true).unwrap();

        let state = task.state().unwrap();
        assert_eq!(state.runner_identities["reviewer-1"], identity);
        assert_eq!(identities(&task), 1, "no second identity is issued");
        assert_ne!(state.runner_workspaces["reviewer-1"].snapshot, first);
        assert!(replace_source(&task, "stranger", repo.path(), true).is_err());
    }

    #[test]
    fn without_xdg_runtime_dir_the_standard_user_folder_is_used() {
        let standard = tempfile::TempDir::new().unwrap();
        let folder = standard.path().to_string_lossy().into_owned();
        assert_eq!(
            runtime_dir_from(Some("/run/user/7".into()), standard.path()),
            Some("/run/user/7".into())
        );
        assert_eq!(
            runtime_dir_from(None, standard.path()),
            Some(folder.clone())
        );
        assert_eq!(
            runtime_dir_from(Some(String::new()), standard.path()),
            Some(folder)
        );
        assert_eq!(
            runtime_dir_from(None, &standard.path().join("missing")),
            None
        );
    }

    #[test]
    fn every_docker_command_goes_to_the_pinned_daemon() {
        let command = docker("unix:///run/user/1001/docker.sock");
        let environment: Vec<_> = command.get_envs().collect();
        assert!(environment.contains(&(
            std::ffi::OsStr::new("DOCKER_HOST"),
            Some(std::ffi::OsStr::new("unix:///run/user/1001/docker.sock"))
        )));
        assert!(
            environment.contains(&(std::ffi::OsStr::new("DOCKER_CONTEXT"), None)),
            "a selected context cannot redirect it"
        );
        assert_eq!(command.get_program(), "docker");
    }

    #[test]
    fn only_a_rootless_daemon_runs_reviewed_work() {
        assert!(rootless(
            "[name=seccomp,profile=builtin name=rootless name=cgroupns]"
        ));
        assert!(!rootless(
            "[name=apparmor name=seccomp,profile=builtin name=cgroupns]"
        ));
        assert!(!rootless(""));
    }

    pub fn configuration() -> Configuration {
        Configuration {
            image: format!("sha256:{}", "a".repeat(64)),
            source_paths: vec!["src/**".into(), "tests/**".into()],
            protected_paths: vec!["data/**".into()],
            timeout_seconds: 5,
            docker_host: None,
            image_mounts: vec![],
            memory_mb: None,
            cpus: None,
            lanes: BTreeMap::new(),
        }
    }
    fn back_lane() -> LaneImage {
        LaneImage {
            image: format!("sha256:{}", "b".repeat(64)),
            image_mounts: vec![ImageMount {
                path: "/vendor".into(),
                target: "backend/vendor".into(),
            }],
        }
    }

    #[test]
    fn a_lane_with_its_own_image_runs_in_it_and_the_others_in_the_main_one() {
        let mut config = configuration();
        config.lanes.insert("back".into(), back_lane());
        validate_configuration(&config).unwrap();
        let back = config.for_lane(Some("back"));
        assert_eq!(back.image, back_lane().image);
        assert_eq!(back.image_mounts, back_lane().image_mounts);
        assert!(back.lanes.is_empty());
        assert_eq!(config.for_lane(Some("front")).image, configuration().image);
        assert_eq!(config.for_lane(None).image, configuration().image);

        config.lanes.get_mut("back").unwrap().image = "php:8.4".into();
        let error = validate_configuration(&config).unwrap_err();
        assert!(format!("{error:#}").contains("lane back"), "{error:#}");
        assert!(
            !serde_json::to_string(&configuration())
                .unwrap()
                .contains("lanes"),
            "a task without lane images keeps its evidence signatures"
        );
    }

    #[test]
    fn a_change_of_lane_images_only_waits_for_that_lanes_workers() {
        let current = configuration();
        let mut added = current.clone();
        added.lanes.insert("back".into(), back_lane());
        assert_eq!(
            changed_lanes(Some(&current), &added),
            Some(std::collections::BTreeSet::from(["back".to_string()]))
        );
        let mut main = added.clone();
        main.timeout_seconds = 60;
        assert_eq!(
            changed_lanes(Some(&current), &main),
            None,
            "the main image changed"
        );
        assert_eq!(changed_lanes(None, &added), None, "a first configuration");
    }

    // A committed template (.env.example) that a protected test reads must
    // be in the snapshot when the runner configuration names it; a real
    // .env never is, whatever the globs say.
    // The leader cannot read the snapshot policy: it asks why a path is in
    // or out of a container's copy of the project.
    #[test]
    fn a_snapshot_decision_names_the_rule_that_made_it() {
        let mut config = configuration();
        config.source_paths = vec!["src/**".into(), "backend/**".into()];
        let decision = |path: &str| snapshot_decision(path, &config).unwrap();
        assert!(
            decision("src/a.rs").starts_with("src/a.rs is in"),
            "{}",
            decision("src/a.rs")
        );
        assert!(
            decision("backend/.env.example").contains("dotenv"),
            "{}",
            decision("backend/.env.example")
        );
        assert!(
            decision("backend/.env.example").contains("name it exactly in source_paths"),
            "a template says how to let it in"
        );
        assert!(decision("backend/vendor/x.php").contains("vendor"));
        assert!(decision("backend/db.sqlite").contains(".sqlite"));
        assert!(decision("data/a").contains("protected_paths"));
        assert!(decision("README.md").contains("no source_paths"));
    }

    // The leader reads source and docs, never secrets or data, with or
    // without a runner.
    #[test]
    fn the_leader_reads_source_and_docs_but_never_secrets() {
        let mut config = configuration();
        config.source_paths = vec!["src/**".into()];
        let unreadable =
            |path: &str, config: Option<&Configuration>| leader_unreadable(path, config).unwrap();
        assert_eq!(unreadable("src/a.rs", Some(&config)), None);
        assert_eq!(unreadable("docs/guide.md", Some(&config)), None, "docs");
        assert!(
            unreadable("README.txt", Some(&config))
                .unwrap()
                .contains("source_paths")
        );
        assert!(
            unreadable("data/a", Some(&config))
                .unwrap()
                .contains("protected_paths")
        );
        assert!(unreadable("src/.env", Some(&config)).is_some());
        assert!(
            unreadable("keys/server.pem", None)
                .unwrap()
                .contains(".pem")
        );
        assert!(
            unreadable("backend/.env.local", None)
                .unwrap()
                .contains("dotenv")
        );
        assert_eq!(unreadable("backend/app/Models/System.php", None), None);
        assert!(leader_unreadable("../other/a.rs", None).is_err());
        assert!(leader_unreadable("/etc/passwd", None).is_err());
    }

    #[test]
    fn a_dotenv_template_is_source_only_when_named_exactly() {
        let mut config = configuration();
        config.source_paths = vec![
            "backend/**".into(),
            "backend/.env.example".into(),
            "backend/.env".into(),
            "backend/.env.production".into(),
        ];
        assert!(source_path("backend/.env.example", &config).unwrap());
        for path in [
            "backend/.env",
            "backend/.env.production",
            "backend/.env.sample",
            "web/.env.example",
        ] {
            assert!(!source_path(path, &config).unwrap(), "{path}");
        }
        config.protected_paths = vec!["backend/.env.example".into()];
        assert!(
            !source_path("backend/.env.example", &config).unwrap(),
            "the user's protected paths still win"
        );
    }

    #[test]
    fn policy_distinguishes_source_deletion_from_data_and_path_escapes() {
        let config = configuration();
        assert!(source_path("src/old.rs", &config).unwrap());
        for path in [
            ".env",
            "src/production.sqlite",
            "src/storage/user.txt",
            "data/a",
            ".git/config",
        ] {
            assert!(!source_path(path, &config).unwrap());
        }
        assert!(relative(Path::new("../other")).is_err());
        assert!(relative(Path::new("/etc/passwd")).is_err());
    }
    #[test]
    fn mount_and_capability_policy_cannot_be_changed_by_command_text() {
        let root = tempfile::TempDir::new().unwrap();
        let args = docker_arguments(
            &configuration(),
            root.path(),
            true,
            "devagentics-test",
            "echo --privileged --network host",
        )
        .unwrap();
        assert!(
            args.contains(&"--network=none".into())
                && args.contains(&"--read-only".into())
                && args.contains(&"--pull=never".into())
        );
        assert!(
            args.iter()
                .any(|argument| argument.ends_with("dst=/workspace,readonly"))
        );
        assert_eq!(args.last().unwrap(), "echo --privileged --network host");
        let user = args
            .iter()
            .position(|argument| argument == "--user")
            .unwrap();
        assert_eq!(
            args[user + 1],
            "0:0",
            "on a rootless daemon the container's root is the user, unprivileged"
        );
        assert!(args.contains(&"--cap-drop=ALL".into()));
    }
    #[test]
    fn symlinks_never_cross_snapshot_or_project_roots() {
        let root = tempfile::TempDir::new().unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", root.path().join("escape")).unwrap();
            assert!(regular(root.path(), "escape/x").is_err());
        }
    }
}

#[cfg(test)]
mod docker_tests {
    use super::*;
    use crate::{execution::Workflow, orders::OrderRequest, team::Team};
    fn config() -> Configuration {
        Configuration {
            image: std::env::var("DEVAGENTICS_TEST_IMAGE")
                .expect("set DEVAGENTICS_TEST_IMAGE to an existing local sha256 image ID"),
            source_paths: vec!["src/**".into(), "tests/**".into()],
            protected_paths: vec!["data/**".into()],
            timeout_seconds: 30,
            docker_host: None,
            image_mounts: vec![],
            memory_mb: None,
            cpus: None,
            lanes: BTreeMap::new(),
        }
    }
    #[test]
    #[ignore = "requires explicit local Docker image and isolated containers"]
    fn reviewed_source_checks_independent_review_merge_and_milestone() {
        reviewed_source_lifecycle(false, false);
    }

    #[test]
    #[ignore = "requires explicit local Docker image and isolated containers"]
    fn a_light_item_merges_on_its_candidate_check_and_the_leaders_reason() {
        reviewed_source_lifecycle(false, true);
    }

    #[test]
    #[ignore = "requires explicit local Docker image and isolated containers"]
    fn reviewed_opencode_workers_checkpoint_check_and_submit_independent_review() {
        reviewed_source_lifecycle(true, false);
    }

    fn reviewed_source_lifecycle(opencode: bool, light: bool) {
        let root = tempfile::TempDir::new().unwrap();
        let (home, devws, task, repo) = crate::board::tests::started(&root);
        let devws = devws.with_kind("opencode", crate::providers::Kind::Opencode);
        if opencode {
            let group = crate::opencode::GroupSettings {
                enabled: true,
                model: Some("fixture/fixture".into()),
            };
            crate::opencode::OpencodeSettings {
                enabled: true,
                floor: group.clone(),
                senior: group,
                ..Default::default()
            }
            .save(&home)
            .unwrap();
        }
        crate::checks::tests::run(repo.path(), &["config", "user.name", "Test Developer"]);
        crate::checks::tests::run(
            repo.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        crate::checks::tests::commit_file(repo.path(), "src/old.rs", "old\n", "initial source");
        crate::checks::tests::commit_file(
            repo.path(),
            "data/users.sqlite",
            "protected fixture\n",
            "data fixture",
        );
        crate::checks::tests::commit_file(
            repo.path(),
            ".env",
            "PRIVATE_FIXTURE=not-for-workers\n",
            "environment fixture",
        );
        let repo_name = task.config().unwrap().repos[0].name.clone();
        let lane = crate::lanes::add(&task, "back", &["src/**".into()], None).unwrap();
        crate::roles::provision_specialists(&home).unwrap();
        configure(&task, config()).unwrap();
        let mut definitions = serde_json::json!({"environments":{"testing":{}},"checks":{"unit":{"command":"test -f src/new.rs && test ! -e src/old.rs && printf '{\"tests\":1,\"failures\":0,\"required_skipped\":0}' > \"$DEVAGENTICS_RESULT\"","environment":"testing","test_evidence":true}},"selected":["unit"]});
        definitions["environments"]["testing"]["values"] =
            serde_json::json!({"RUNNER_TEXT":"value with 'quote"});
        definitions["checks"]["smoke"] =
            serde_json::json!({"command":"test -f src/new.rs","environment":"testing"});
        definitions["candidate"] = serde_json::json!(["smoke"]);
        // The application starts and answers on loopback: --network=none keeps lo.
        definitions["checks"]["app"] = serde_json::json!({"command":"(nc -l -p 8080 -s 127.0.0.1 -e echo up &); sleep 0.5; nc 127.0.0.1 8080 </dev/null | grep -qx up","environment":"testing"});
        definitions["system"] = serde_json::json!(["app"]);
        let original = definitions["checks"]["unit"]["command"].as_str().unwrap();
        definitions["checks"]["unit"]["command"] = serde_json::json!(format!(
            "test \"$RUNNER_TEXT\" = \"value with 'quote\" && {original}"
        ));
        crate::check_profiles::configure(
            &task,
            &repo_name,
            serde_json::from_value(definitions).unwrap(),
        )
        .unwrap();
        let mut application = crate::execution::tests::baseline();
        let item = application.items.get_mut("S01-B01").unwrap();
        item.repo = repo_name.clone();
        item.allowed = vec!["src/**".into()];
        item.profiles = vec!["unit".into()];
        if light {
            item.kind = crate::execution::WorkKind::Change;
        }
        crate::execution::store(&task, application, "complete baseline").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            Ok(())
        })
        .unwrap();
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let author = team
            .spawn_agent_in("backend-data", None, Some("back"))
            .unwrap();
        let reviewer = team.spawn_agent_in("code-reviewer", None, None).unwrap();
        if opencode {
            assert_eq!(author.agent, "opencode");
            assert_eq!(reviewer.agent, "opencode");
        }
        let order = crate::orders::send_order(
            &task,
            OrderRequest {
                read_lane: None,
                diagnosis: None,
                read: Vec::new(),
                targets: Vec::new(),
                agent: author.id.clone(),
                step: "S01-B01".into(),
                title: "Replace source".into(),
                text: "Replace old source with new".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                profiles: vec![],
                plan: None,
                spec: Some("S01".into()),
            },
        )
        .unwrap();
        bind_order(&task, &author.id, &order).unwrap();
        task.transaction(|tx| {
            tx.state
                .orders
                .iter_mut()
                .find(|assignment| assignment.id == order)
                .unwrap()
                .delivered = true;
            Ok(())
        })
        .unwrap();
        let identity = task.state().unwrap().runner_identities[&author.id].clone();
        authenticate(&task, &author.id, &identity).unwrap();
        assert!(
            crate::worker_tools::call(
                &task,
                &author.id,
                "approve",
                &serde_json::json!({"revision":1})
            )
            .is_err()
        );
        let workspace = task.state().unwrap().runner_workspaces[&author.id].clone();
        assert!(!workspace.snapshot.join(".env").exists());
        assert!(!workspace.snapshot.join("data/users.sqlite").exists());
        assert!(!workspace.snapshot.join(".git").exists());
        let result=execute(&task,&author.id,"set -eu; test ! -e /var/run/docker.sock; test ! -e /host; test \"$(wc -l < /proc/net/route)\" -eq 1; if touch /etc/runner-escape 2>/dev/null; then exit 1; fi; rm src/old.rs; printf 'new\\n' > src/new.rs").unwrap();
        assert!(result.success, "{} {}", result.stdout, result.stderr);
        let changed = if opencode {
            crate::worker_tools::call(
                &task,
                &author.id,
                "checkpoint",
                &serde_json::json!({"summary":"Replace scoped source"}),
            )
            .unwrap()["changed"]
                .as_array()
                .unwrap()
                .len()
        } else {
            checkpoint(&task, &author.id, "Replace scoped source")
                .unwrap()
                .len()
        };
        assert_eq!(changed, 2);
        assert!(repo.path().join("src/old.rs").exists());
        assert!(!lane.worktree.join("src/old.rs").exists());
        assert_eq!(
            fs::read_to_string(lane.worktree.join("data/users.sqlite")).unwrap(),
            "protected fixture\n"
        );
        if opencode {
            crate::worker_tools::call(&task, &author.id, "check", &serde_json::json!({})).unwrap();
        }
        let checked = crate::check_profiles::run(&task, &order, Some(&author.pane)).unwrap();
        assert!(checked.passed(), "{:?}", checked.phases);
        crate::orders::report(
            &task,
            &author.pane,
            &order,
            crate::orders::Outcome::Done,
            "Implemented and checked",
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(
            task.state().unwrap().check_evidence[&order].run,
            checked.run,
            "report must reuse passing evidence for unchanged source"
        );
        let before = checks::head(repo.path()).unwrap();
        assert!(crate::orders::accept_step(&task, "S01-B01").is_err());
        assert_eq!(checks::head(repo.path()).unwrap(), before);
        if light {
            let error = crate::reviews::accept_light(&task, "S01-B01", "")
                .unwrap_err()
                .to_string();
            assert!(error.contains("accept_reason"), "{error}");
            crate::reviews::accept_light(
                &task,
                "S01-B01",
                "replaces old.rs with new.rs as ordered",
            )
            .unwrap();
            let state = task.state().unwrap();
            let review = state.reviews.last().unwrap();
            assert_eq!(review.reviewer, crate::state::LEADER);
            assert!(
                !review.candidate_evidence.is_empty(),
                "bound to the passing candidate check"
            );
            crate::orders::accept_step(&task, "S01-B01").unwrap();
            assert!(
                repo.path().join("src/new.rs").exists(),
                "merged into the task branch"
            );
            assert!(!repo.path().join("src/old.rs").exists());
            assert_eq!(
                task.state().unwrap().steps[0].status,
                crate::state::StepStatus::Accepted
            );
            assert_eq!(
                task.state().unwrap().reviews.len(),
                1,
                "no independent review was requested"
            );
            return;
        }
        let review = crate::reviews::request(&task, &order, &reviewer.id).unwrap();
        let resumed_source = crate::reviews::checkout(&task, &review);
        assert_eq!(
            checks::head(&resumed_source).unwrap(),
            review.candidate.commit
        );
        assert!(
            crate::reviews::resumption(&task.state().unwrap(), &reviewer.id)
                .unwrap()
                .contains(&review.id)
        );
        prepare(&task, &reviewer.id, &resumed_source, true).unwrap();
        assert!(
            execute(
                &task,
                &reviewer.id,
                "test -f src/new.rs && test ! -e src/old.rs"
            )
            .unwrap()
            .success
        );
        assert!(
            !execute(&task, &reviewer.id, "printf unsafe > src/new.rs")
                .unwrap()
                .success
        );
        assert!(
            crate::reviews::submit(
                &task,
                &author.pane,
                &review.id,
                crate::reviews::Verdict::Approved,
                vec![],
                "self approval"
            )
            .is_err()
        );
        if opencode {
            crate::worker_tools::call(
                &task,
                &reviewer.id,
                "review_diff",
                &serde_json::json!({"review":review.id}),
            )
            .unwrap();
            crate::worker_tools::call(&task, &reviewer.id, "review_submit", &serde_json::json!({"review":review.id,"verdict":"approved","findings":[],"summary":"Contract, source and tests reviewed"})).unwrap();
        } else {
            crate::reviews::submit(
                &task,
                &reviewer.pane,
                &review.id,
                crate::reviews::Verdict::Approved,
                vec![],
                "Contract, source and tests reviewed",
            )
            .unwrap();
        }
        crate::checks::tests::commit_file(
            repo.path(),
            "src/parallel.rs",
            "accepted parallel context\n",
            "parallel context",
        );
        let advanced = checks::head(repo.path()).unwrap();
        assert!(crate::orders::accept_step(&task, "S01-B01").is_err());
        assert_eq!(checks::head(repo.path()).unwrap(), advanced);
        let refreshed = crate::reviews::request(&task, &order, &reviewer.id).unwrap();
        let outside = crate::reviews::Finding {
            file: "src/elsewhere.rs".into(),
            line: 40,
            severity: "high".into(),
            blocking: true,
            rationale: "an old bug the change never touched".into(),
            category: Some(crate::reviews::Category::ImplementationDefect),
            ..crate::reviews::Finding::default()
        };
        let error = crate::reviews::submit(
            &task,
            &reviewer.pane,
            &refreshed.id,
            crate::reviews::Verdict::ChangesRequested,
            vec![outside],
            "Old bug",
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("every blocking finding is outside this change (src/elsewhere.rs:40)"),
            "{error}"
        );
        crate::reviews::submit(
            &task,
            &reviewer.pane,
            &refreshed.id,
            crate::reviews::Verdict::ChangesRequested,
            vec![],
            "Review correction required",
        )
        .unwrap();
        assert!(crate::orders::accept_step(&task, "S01-B01").is_err());
        let final_review = crate::reviews::request(&task, &order, &reviewer.id).unwrap();
        crate::checks::tests::commit_file(
            repo.path(),
            ".devagentics/history/feat-x/history.json",
            "{}\n",
            "chore(team): record a plan while the review is open",
        );
        crate::reviews::submit(
            &task,
            &reviewer.pane,
            &final_review.id,
            crate::reviews::Verdict::Approved,
            vec![],
            "Exact refreshed integration context reviewed",
        )
        .unwrap();
        assert!(
            task.state().unwrap().queue.iter().any(|message| {
                message.to == crate::state::LEADER
                    && message.text.contains(&final_review.id)
                    && message.text.contains("accept_step S01-B01")
            }),
            "the leader hears of the submitted review"
        );
        assert!(
            !final_review.candidate_evidence.is_empty(),
            "the merge candidate passed its candidate check before review"
        );
        task.transaction(|tx| {
            let mut other = tx
                .state
                .orders
                .iter()
                .find(|item| item.id == order)
                .unwrap()
                .clone();
            other.id = "o-unreviewed".into();
            other.step = "unreviewed".into();
            tx.state.steps.push(crate::state::Step {
                id: other.step.clone(),
                title: "Another report in the same lane".into(),
                status: crate::state::StepStatus::Reported,
                attempts: 1,
                spec: None,
                review_since: None,
            });
            tx.state.orders.push(other);
            Ok(())
        })
        .unwrap();
        crate::orders::accept_step(&task, "S01-B01").unwrap();
        let state = task.state().unwrap();
        assert!(!state.integrations.contains_key("unreviewed"));
        assert!(state.steps.iter().any(
            |step| step.id == "unreviewed" && step.status == crate::state::StepStatus::Reported
        ));
        let merged = checks::head(repo.path()).unwrap();
        task.transaction(|tx| {
            tx.state
                .steps
                .iter_mut()
                .find(|step| step.id == "S01-B01")
                .unwrap()
                .status = crate::state::StepStatus::Reported;
            tx.state.integrations.get_mut("S01-B01").unwrap().accepted = false;
            Ok(())
        })
        .unwrap();
        crate::orders::accept_step(&task, "S01-B01").unwrap();
        assert_eq!(checks::head(repo.path()).unwrap(), merged);
        assert_eq!(
            fs::read_to_string(repo.path().join("src/new.rs")).unwrap(),
            "new\n"
        );
        assert!(
            repo.path()
                .join(".devagentics/history/feat-x/history.json")
                .exists(),
            "the history recorded during the review is kept beside the reviewed code"
        );
        let final_check = crate::milestones::validate(&task, "M01").unwrap();
        assert!(
            final_check
                .phases
                .iter()
                .any(|phase| phase.profile == "app"),
            "the milestone ran the system check: {:?}",
            final_check.phases
        );
        assert!(
            final_check
                .phases
                .iter()
                .all(|phase| phase.outcome == crate::check_profiles::Outcome::Passed)
        );
        assert!(crate::milestones::ready(&task, "M01").unwrap());
        assert_eq!(
            crate::workflow::milestones(&task).unwrap()[0]["status"],
            "ready"
        );
        assert!(
            task.state().unwrap().application.milestones["M01"]
                .release_record
                .is_none()
        );
        assert_eq!(
            fs::read_to_string(repo.path().join("data/users.sqlite")).unwrap(),
            "protected fixture\n"
        );
    }
    #[test]
    #[ignore = "requires explicit local Docker image"]
    fn readonly_snapshot_and_timeout_are_enforced_by_docker() {
        let directory = tempfile::TempDir::new().unwrap();
        fs::write(directory.path().join("source.txt"), "unchanged").unwrap();
        let mut config = config();
        let output =
            run_container(&config, directory.path(), true, "echo changed > source.txt").unwrap();
        assert!(!output.success);
        assert_eq!(
            fs::read_to_string(directory.path().join("source.txt")).unwrap(),
            "unchanged"
        );
        config.timeout_seconds = 2;
        assert!(
            run_container(&config, directory.path(), false, "sleep 15")
                .unwrap()
                .timed_out
        );
    }
}

#[cfg(test)]
mod policy_regressions {
    use super::*;
    #[test]
    fn source_fingerprints_are_stable_and_do_not_need_git_subprocesses() {
        assert_eq!(
            content_hash(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn a_checkpoint_that_cannot_commit_leaves_the_source_as_it_was() {
        let source = crate::checks::tests::repo();
        let snapshot = tempfile::TempDir::new().unwrap();
        // No author: the commit must fail, as on a host without a git identity.
        crate::checks::tests::run(source.path(), &["config", "user.name", ""]);
        fs::write(snapshot.path().join("README.md"), "changed\n").unwrap();
        fs::write(snapshot.path().join("new.txt"), "new\n").unwrap();
        let changed = vec!["README.md".to_string(), "new.txt".to_string()];
        let files = BTreeMap::from([
            ("README.md".to_string(), b"changed\n".to_vec()),
            ("new.txt".to_string(), b"new\n".to_vec()),
        ]);

        let error = write_checkpoint(source.path(), snapshot.path(), &changed, &files, "feat: x")
            .unwrap_err()
            .to_string();

        assert!(error.contains("user.name"), "{error}");
        assert!(
            checks::inspect(source.path(), "HEAD", &[], &[])
                .unwrap()
                .uncommitted
                .is_empty(),
            "the failed checkpoint left edits behind"
        );
        assert_eq!(
            fs::read_to_string(source.path().join("README.md")).unwrap(),
            "hello\n"
        );
        assert!(!source.path().join("new.txt").exists());
    }

    #[test]
    fn build_output_the_repository_ignores_is_not_part_of_a_checkpoint() {
        let source = crate::checks::tests::repo();
        fs::create_dir_all(source.path().join("web")).unwrap();
        fs::write(source.path().join("web/.gitignore"), "/.next/\n").unwrap();
        let paths = [
            "web/.next/types/cache-life.d.ts".to_string(),
            "web/src/new.test.ts".to_string(),
        ];
        assert_eq!(
            ignored(source.path(), paths.iter()).unwrap(),
            std::collections::BTreeSet::from(["web/.next/types/cache-life.d.ts".to_string()])
        );
        assert!(
            ignored(source.path(), paths[1..].iter())
                .unwrap()
                .is_empty(),
            "nothing ignored is not an error"
        );
    }

    #[test]
    fn undoing_a_copied_checkpoint_restores_changed_files_and_removes_new_ones() {
        let source = crate::checks::tests::repo();
        fs::write(source.path().join("README.md"), "changed\n").unwrap();
        fs::create_dir_all(source.path().join("web/src")).unwrap();
        fs::write(source.path().join("web/src/new.ts"), "new\n").unwrap();
        let changed = vec!["README.md".to_string(), "web/src/new.ts".to_string()];
        safe_git(source.path(), &["add", "--", "README.md", "web/src/new.ts"]).unwrap();

        undo_copy(source.path(), &changed).unwrap();

        assert!(
            checks::inspect(source.path(), "HEAD", &[], &[])
                .unwrap()
                .uncommitted
                .is_empty()
        );
        assert_eq!(
            fs::read_to_string(source.path().join("README.md")).unwrap(),
            "hello\n"
        );
        assert!(!source.path().join("web/src/new.ts").exists());
    }

    #[test]
    fn checkpoints_do_not_execute_source_controlled_git_filters() {
        let repo = crate::checks::tests::repo();
        let marker = repo.path().join("unexpected-hook");
        fs::write(
            repo.path().join(".gitattributes"),
            "*.txt filter=source-probe\n",
        )
        .unwrap();
        fs::write(repo.path().join("probe.txt"), "content\n").unwrap();
        crate::checks::tests::run(
            repo.path(),
            &[
                "config",
                "filter.source-probe.clean",
                &format!("touch {}; cat", marker.display()),
            ],
        );
        safe_git(repo.path(), &["add", "--", "probe.txt"]).unwrap();
        assert!(!marker.exists());
    }
}
