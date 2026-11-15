//! Containers see disposable source snapshots, never host projects or data volumes.
use crate::{checks, execution::Workflow, state::LEADER, task::Task};
use anyhow::{Context, Result, ensure};
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// A local immutable image ID. The runner never pulls arbitrary images.
    pub image: String,
    pub source_paths: Vec<String>,
    pub protected_paths: Vec<String>,
    pub timeout_seconds: u64,
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
}
fn protected(path: &str, config: &Configuration) -> Result<bool> {
    let lower = path.to_ascii_lowercase();
    let sensitive = lower.split('/').any(|part| {
        part == ".git"
            || part == ".ssh"
            || part == ".aws"
            || part == ".codex"
            || part == ".claude"
            || part == ".env"
            || part.starts_with(".env.")
            || part == "node_modules"
            || part == "vendor"
            || part == "storage"
            || part == "pgdata"
            || part == "mysql"
            || part == "mongodb"
            || part == ".devagentics"
            || part == ".devws"
            || part == ".agents"
    });
    Ok(sensitive
        || [
            ".sqlite", ".sqlite3", ".db", ".pem", ".key", ".p12", ".sock", ".dump",
        ]
        .iter()
        .any(|extension| lower.ends_with(extension))
        || checks::glob_set(&config.protected_paths)?.is_match(path))
}
fn source_path(path: &str, config: &Configuration) -> Result<bool> {
    Ok(!protected(path, config)? && checks::glob_set(&config.source_paths)?.is_match(path))
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
fn regular(root: &Path, path: &str) -> Result<PathBuf> {
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
fn bytes(path: &Path) -> Result<Vec<u8>> {
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

pub fn validate_configuration(config: &Configuration) -> Result<()> {
    ensure!(
        config.image.starts_with("sha256:")
            && config.image.len() == 71
            && config.image[7..]
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
        "runner image must be an immutable local sha256 image ID"
    );
    ensure!(
        (1..=900).contains(&config.timeout_seconds),
        "runner timeout must be 1–900 seconds"
    );
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
    if let Ok(endpoint) = std::env::var("DOCKER_HOST") {
        ensure!(
            endpoint.starts_with("unix://"),
            "reviewed execution requires a local Unix-socket Docker endpoint"
        );
    }
    let endpoint = Command::new("docker")
        .args([
            "context",
            "inspect",
            "--format",
            r#"{{(index .Endpoints "docker").Host}}"#,
        ])
        .output()?;
    ensure!(
        endpoint.status.success()
            && String::from_utf8_lossy(&endpoint.stdout)
                .trim()
                .starts_with("unix://"),
        "remote Docker contexts are not allowed for reviewed source mounts"
    );
    let result = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", &config.image])
        .output()
        .context("Docker must be installed and running")?;
    ensure!(
        result.status.success() && String::from_utf8_lossy(&result.stdout).trim() == config.image,
        "configured local Docker image is unavailable; no pull or daemon changes were attempted"
    );
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
pub fn configure(task: &Task, config: Configuration) -> Result<()> {
    probe(&config)?;
    // Verify the isolation flags in a real disposable container before enabling.
    let root = task.dir().join("runner-probe");
    fs::create_dir_all(&root)?;
    let result = run_container(
        &config,
        &root,
        true,
        "test ! -e /var/run/docker.sock && test ! -e /host && test ! -e /root/.ssh && test ! -w /etc && test -r /proc/net/route",
    )?;
    ensure!(
        result.success,
        "container isolation preflight failed: {}",
        result.stderr
    );
    let _control = task.control_lock()?;
    task.transaction(|tx| {
        ensure!(
            !tx.state
                .agents
                .iter()
                .any(|agent| agent.live && tx.state.workflow == Workflow::Reviewed),
            "pause and close reviewed workers before changing runner policy"
        );
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
    let config = task.state()?.runner.context("runner not configured")?;
    let directory = task.dir().join("workers").join(agent);
    fs::create_dir_all(&directory)?;
    let nonce = token()?;
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
    let workspace = Workspace {
        snapshot,
        source: source.canonicalize()?,
        base: checks::head(source)?,
        original,
        executable,
        readonly,
        order: None,
        revision: 0,
        baseline: task.state()?.application.revision,
    };
    let identity = directory.join(format!("identity-{}", &nonce[..12]));
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
    // A new source generation discards stale tool context without deleting old artifacts.
    let identity = state
        .runner_identities
        .get(agent)
        .cloned()
        .context("worker identity missing")?;
    prepare(task, agent, &source, readonly)?;
    task.transaction(|tx| {
        tx.state.runner_identities.insert(agent.into(), identity);
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
    let mut changed = Vec::new();
    for path in worker
        .original
        .keys()
        .chain(files.keys())
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
    for path in &changed {
        let destination = regular(&worker.source, path)?;
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
                let executable = fs::metadata(regular(&worker.snapshot, path)?)?
                    .permissions()
                    .mode()
                    & 0o111
                    != 0;
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
    if !changed.is_empty() {
        let mut arguments = vec!["add", "--"];
        arguments.extend(changed.iter().map(String::as_str));
        safe_git(&worker.source, &arguments)?;
        let subject = format!("feat: implement {}", order.step);
        let mut arguments = vec![
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--only",
            "-m",
            &subject,
            "--",
        ];
        arguments.extend(changed.iter().map(String::as_str));
        safe_git(&worker.source, &arguments)?;
    }
    let new_base = checks::head(&worker.source)?;
    let mut original = BTreeMap::new();
    for (path, content) in files {
        if source_path(&path, config)? {
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
fn drain(mut stream: impl Read) -> String {
    let mut collected = Vec::new();
    let mut buffer = [0u8; 4096];
    while let Ok(size) = stream.read(&mut buffer) {
        if size == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(collected.len());
        collected.extend_from_slice(&buffer[..size.min(remaining)]);
    }
    String::from_utf8_lossy(&collected).into_owned()
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
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    let user = if uid == 0 {
        "65534:65534".into()
    } else {
        format!("{uid}:{gid}")
    };
    Ok(vec![
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
        "--memory=512m".into(),
        "--cpus=1".into(),
        "--user".into(),
        user,
        "--mount".into(),
        mount,
        "--tmpfs=/tmp:rw,noexec,nosuid,size=64m".into(),
        "--workdir=/workspace".into(),
        "--env=HOME=/tmp".into(),
        "--entrypoint=/bin/sh".into(),
        config.image.clone(),
        "-lc".into(),
        command.into(),
    ])
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
    ensure!(command.len() <= 64 * 1024, "command too large");
    let name = format!("devagentics-{}", &token()?[..20]);
    let mut arguments = docker_arguments(config, source, readonly, &name, command)?;
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
    let mut child = Command::new("docker")
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().context("container stdout")?;
    let stderr = child.stderr.take().context("container stderr")?;
    let stdout = std::thread::spawn(move || drain(stdout));
    let stderr = std::thread::spawn(move || drain(stderr));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(config.timeout_seconds) {
            timed_out = true;
            let _ = child.kill();
            let _ = Command::new("docker")
                .args(["rm", "--force", &name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        success: status.success() && !timed_out,
        stdout: stdout
            .join()
            .map_err(|_| anyhow::anyhow!("container output reader failed"))?,
        stderr: stderr
            .join()
            .map_err(|_| anyhow::anyhow!("container error reader failed"))?,
        timed_out,
    })
}
pub fn execute(task: &Task, agent: &str, command: &str) -> Result<Output> {
    let state = task.state()?;
    ensure!(!state.paused, "task is paused");
    let workspace = state
        .runner_workspaces
        .get(agent)
        .context("worker source missing")?;
    let config = state.runner.context("runner missing")?;
    run_container(&config, &workspace.snapshot, workspace.readonly, command)
}

/// Git plumbing must not execute repository hooks, filters or fsmonitor programs.
pub(crate) fn safe_git(root: &Path, args: &[&str]) -> Result<String> {
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
    let result = command.args(args).output()?;
    ensure!(
        result.status.success(),
        "controlled Git operation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().into())
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
    let definitions = configuration
        .check_profiles
        .get(&item.repo)
        .context("named checks not configured")?;
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
    let mut failure = None;
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
            &worker,
            name,
            &profile,
            environment,
            &directory,
            index,
        )?;
        if phase.outcome != crate::check_profiles::Outcome::Passed {
            failure = Some(format!(
                "{}: {}; evidence {}",
                name,
                phase.diagnostic,
                phase.log.display()
            ));
            break;
        }
    }
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
    let output = Command::new("docker")
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
                    String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]).into_owned(),
                );
            }
        }
        repositories.push(serde_json::json!({"name":repo.name,"tracked_files":files.lines().take(2000).collect::<Vec<_>>(),"project_context":documents}));
    }
    Ok(serde_json::json!(repositories))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn configuration() -> Configuration {
        Configuration {
            image: format!("sha256:{}", "a".repeat(64)),
            source_paths: vec!["src/**".into(), "tests/**".into()],
            protected_paths: vec!["data/**".into()],
            timeout_seconds: 5,
        }
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
        }
    }
    #[test]
    #[ignore = "requires explicit local Docker image and isolated containers"]
    fn reviewed_source_checks_independent_review_merge_and_milestone() {
        let root = tempfile::TempDir::new().unwrap();
        let (home, devws, task, repo) = crate::board::tests::started(&root);
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
        let order = crate::orders::send_order(
            &task,
            OrderRequest {
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
        let changed = checkpoint(&task, &author.id, "Replace scoped source").unwrap();
        assert_eq!(changed.len(), 2);
        assert!(repo.path().join("src/old.rs").exists());
        assert!(!lane.worktree.join("src/old.rs").exists());
        assert_eq!(
            fs::read_to_string(lane.worktree.join("data/users.sqlite")).unwrap(),
            "protected fixture\n"
        );
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
        crate::reviews::submit(
            &task,
            &reviewer.pane,
            &review.id,
            crate::reviews::Verdict::Approved,
            vec![],
            "Contract, source and tests reviewed",
        )
        .unwrap();
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
        crate::reviews::submit(
            &task,
            &reviewer.pane,
            &final_review.id,
            crate::reviews::Verdict::Approved,
            vec![],
            "Exact refreshed integration context reviewed",
        )
        .unwrap();
        crate::orders::accept_step(&task, "S01-B01").unwrap();
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
        let final_check = crate::milestones::validate(&task, "M01").unwrap();
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
