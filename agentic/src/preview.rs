//! A preview: an application image the team built, started for the user to
//! look at in their browser. The container keeps every check container's
//! bounds (no network, read-only, capped), and a forwarder on the host opens
//! `127.0.0.1:<port>` only, passing each connection to a unix socket the
//! application listens on. It ends at its time, on `preview_stop`, or when
//! the task is cancelled; the user approves each start (mcp.rs).
use crate::log::Kind;
use crate::task::Task;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The runner workspace a preview's snapshot is kept under.
pub const WORKER: &str = "preview";
/// Where the application finds its socket folder inside the container.
pub const SOCKETS_INSIDE: &str = "/run/preview";
/// The longest a preview may run.
pub const MAX_HOURS: u64 = 8;

/// What the leader asks to start.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// A local immutable image ID (`sha256:…`), as the runner's.
    pub image: String,
    /// The command the image runs, as separate arguments.
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// A socket file the application creates in `/run/preview`, and the
    /// port on the user's `127.0.0.1` that reaches it.
    pub sockets: BTreeMap<String, u16>,
    pub hours: u64,
    /// Where the task branch's snapshot is mounted, read-only.
    #[serde(default = "default_workdir")]
    pub workdir: String,
}

fn default_workdir() -> String {
    "/workspace".into()
}

/// A running preview, in the task's state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub container: String,
    pub image: String,
    pub sockets: BTreeMap<String, u16>,
    /// Epoch seconds it ends at.
    pub until: u64,
    /// The forwarder's process, once it runs.
    pub forwarder: Option<u32>,
}

impl Preview {
    /// The addresses the user opens.
    pub fn urls(&self) -> Vec<String> {
        let mut ports: Vec<_> = self.sockets.values().collect();
        ports.sort();
        ports
            .into_iter()
            .map(|port| format!("http://127.0.0.1:{port}"))
            .collect()
    }
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        crate::runner::validate_image(&self.image)?;
        ensure!(
            !self.command.is_empty() && self.command.iter().all(|part| !part.contains('\0')),
            "preview command must be a non-empty list of arguments"
        );
        ensure!(
            (1..=MAX_HOURS).contains(&self.hours),
            "a preview runs from 1 to {MAX_HOURS} hours"
        );
        ensure!(
            !self.sockets.is_empty(),
            "a preview needs at least one socket"
        );
        let mut ports: Vec<u16> = self.sockets.values().copied().collect();
        ports.sort();
        ports.dedup();
        ensure!(
            ports.len() == self.sockets.len(),
            "each socket needs its own port"
        );
        for (name, port) in &self.sockets {
            ensure!(
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
                    && !name.starts_with('.'),
                "socket {name:?} must be a plain file name"
            );
            ensure!(*port >= 1024, "port {port} is below 1024");
        }
        for (key, value) in &self.env {
            ensure!(
                !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && !key.starts_with(|c: char| c.is_ascii_digit()),
                "environment name {key:?} must be upper case letters, digits and _"
            );
            ensure!(
                !key.starts_with("DOCKER_") && key != "PATH" && key != "PREVIEW_SOCKET_DIR",
                "environment name {key} is not allowed"
            );
            ensure!(
                !value.contains(['\n', '\0']),
                "environment value of {key} has a line break"
            );
        }
        let workdir = &self.workdir;
        ensure!(
            workdir.starts_with('/')
                && workdir != "/"
                && !workdir.contains(',')
                && !workdir.split('/').any(|part| part == ".." || part == "."),
            "workdir {workdir:?} must be an absolute folder"
        );
        Ok(())
    }
}

/// The container's name for a task.
pub fn container_name(task_id: &str) -> String {
    format!("devagentics-preview-{task_id}")
}

/// The `docker` arguments that start a preview: a check container's bounds,
/// detached, with the snapshot read-only and the socket folder. Never a
/// published port or a network: the forwarder is the only way in.
pub fn docker_arguments(
    request: &Request,
    config: &crate::runner::Configuration,
    name: &str,
    snapshot: &Path,
    sockets: &Path,
) -> Result<Vec<String>> {
    request.validate()?;
    let path = |folder: &Path| -> Result<String> {
        let text = folder.to_str().context("invalid preview path")?.to_string();
        ensure!(!text.contains(','), "Docker mount path contains comma");
        Ok(text)
    };
    let memory_mb = config.memory_mb.unwrap_or(crate::runner::MEMORY_LIMIT_MB);
    let mut arguments: Vec<String> = vec![
        "run".into(),
        "--detach".into(),
        "--rm".into(),
        "--init".into(),
        "--pull=never".into(),
        "--name".into(),
        name.into(),
        "--network=none".into(),
        "--read-only".into(),
        "--tmpfs=/tmp:rw,nosuid,nodev,size=256m".into(),
        "--shm-size=128m".into(),
        "--pids-limit=256".into(),
        format!("--memory={memory_mb}m"),
        format!("--memory-swap={memory_mb}m"),
        format!("--cpus={}", config.cpus.unwrap_or(1)),
        "--mount".into(),
        format!(
            "type=bind,src={},dst={},readonly",
            path(snapshot)?,
            request.workdir
        ),
        "--mount".into(),
        format!("type=bind,src={},dst={SOCKETS_INSIDE}", path(sockets)?),
        format!("--workdir={}", request.workdir),
        format!("--env=PREVIEW_SOCKET_DIR={SOCKETS_INSIDE}"),
    ];
    for (key, value) in &request.env {
        arguments.push(format!("--env={key}={value}"));
    }
    arguments.push(request.image.clone());
    arguments.extend(request.command.iter().cloned());
    Ok(arguments)
}

fn folder(task: &Task) -> PathBuf {
    task.dir().join("preview")
}

fn sockets_folder(task: &Task) -> PathBuf {
    folder(task).join("sockets")
}

fn docker_endpoint(task: &Task) -> Result<String> {
    let config = task
        .state()?
        .runner
        .context("configure the runner before a preview")?;
    crate::runner::endpoint(&config, crate::runner::runtime_dir().as_deref())
}

/// Starts a preview and its forwarder; `command` is this devagentics.
pub fn start(task: &Task, request: &Request, command: &str) -> Result<Preview> {
    request.validate()?;
    expire(task)?;
    let state = task.state()?;
    if let Some(running) = &state.preview {
        bail!(
            "a preview already runs at {}: stop it first (preview_stop)",
            running.urls().join(", ")
        );
    }
    let config = state
        .runner
        .clone()
        .context("configure the runner before a preview")?;
    drop(state);
    for port in request.sockets.values() {
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, *port))
            .with_context(|| format!("port {port} on 127.0.0.1 is taken"))?;
    }
    let worktree = task
        .config()?
        .repos
        .first()
        .context("the task has no repo")?
        .worktree
        .clone();
    crate::runner::prepare(task, WORKER, &worktree, true)?;
    let snapshot = task
        .state()?
        .runner_workspaces
        .get(WORKER)
        .context("preview snapshot missing")?
        .snapshot
        .clone();
    let sockets = sockets_folder(task);
    match std::fs::remove_dir_all(&sockets) {
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    std::fs::create_dir_all(&sockets)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&sockets, std::fs::Permissions::from_mode(0o700))?;
    let name = container_name(&task.config()?.id);
    let arguments = docker_arguments(request, &config, &name, &snapshot, &sockets)?;
    let endpoint = crate::runner::endpoint(&config, crate::runner::runtime_dir().as_deref())?;
    let output = crate::runner::docker(&endpoint).args(&arguments).output()?;
    if !output.status.success() {
        crate::runner::forget(task, WORKER)?;
        bail!(
            "the preview did not start: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let preview = Preview {
        container: name,
        image: request.image.clone(),
        sockets: request.sockets.clone(),
        until: crate::log::now() + request.hours * 3600,
        forwarder: None,
    };
    task.transaction(|tx| {
        tx.state.preview = Some(preview.clone());
        tx.log(
            Kind::Start,
            None,
            None,
            &format!(
                "preview started at {} for {} h ({})",
                preview.urls().join(", "),
                request.hours,
                preview.image
            ),
        );
        Ok(())
    })?;
    let forwarder = spawn_forwarder(task, command)?;
    task.transaction(|tx| {
        if let Some(running) = tx.state.preview.as_mut() {
            running.forwarder = Some(forwarder);
        }
        Ok(())
    })?;
    Ok(Preview {
        forwarder: Some(forwarder),
        ..preview
    })
}

/// The forwarder runs on its own, outside the leader's tool call.
fn spawn_forwarder(task: &Task, command: &str) -> Result<u32> {
    use std::os::unix::process::CommandExt;
    let log = std::fs::File::create(folder(task).join("forward.log"))?;
    let mut process = std::process::Command::new(command);
    process
        .args(["preview-forward", &task.config()?.id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log);
    // Its own session: it outlives the leader's turn and its pane.
    unsafe {
        process.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    Ok(process.spawn()?.id())
}

/// Stops the preview, if one runs: its container, its forwarder (unless it
/// is the caller) and its files. Safe to call twice.
pub fn stop(task: &Task, why: &str) -> Result<bool> {
    let Some(preview) = task.state()?.preview else {
        return Ok(false);
    };
    if let Ok(endpoint) = docker_endpoint(task) {
        // Already gone is fine: --rm removed it when it exited.
        let _ = crate::runner::docker(&endpoint)
            .args(["stop", "--time=10", &preview.container])
            .output();
    }
    if let Some(pid) = preview.forwarder.filter(|pid| *pid != std::process::id()) {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    match std::fs::remove_dir_all(sockets_folder(task)) {
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    crate::runner::forget(task, WORKER)?;
    task.transaction(|tx| {
        tx.state.preview = None;
        tx.log(Kind::Stop, None, None, &format!("preview stopped: {why}"));
        Ok(())
    })?;
    Ok(true)
}

/// Stops a preview whose time is up (a dispatcher calls it when it starts:
/// after a reboot the forwarder is gone too).
pub fn expire(task: &Task) -> Result<()> {
    if task
        .state()?
        .preview
        .is_some_and(|preview| preview.until <= crate::log::now())
    {
        stop(task, "its time is up")?;
    }
    Ok(())
}

/// What the leader and the board read.
pub fn status(task: &Task) -> Result<serde_json::Value> {
    Ok(match task.state()?.preview {
        Some(preview) => serde_json::json!({
            "running": true,
            "urls": preview.urls(),
            "sockets": preview.sockets,
            "image": preview.image,
            "minutes_left": preview.until.saturating_sub(crate::log::now()) / 60,
        }),
        None => serde_json::json!({"running": false}),
    })
}

/// The forwarder (`devagentics preview-forward`): listens on 127.0.0.1 for
/// each socket until the preview's time is up or its container is gone,
/// then stops it.
pub fn forward(task: &Task) -> Result<()> {
    let preview = task.state()?.preview.context("no preview runs")?;
    let sockets = sockets_folder(task);
    let mut listeners = Vec::new();
    for (name, port) in &preview.sockets {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, *port))
            .with_context(|| format!("port {port} on 127.0.0.1"))?;
        listener.set_nonblocking(true)?;
        listeners.push((listener, sockets.join(name)));
    }
    let endpoint = docker_endpoint(task)?;
    let mut checked = std::time::Instant::now();
    let mut is_running = true;
    let why = loop {
        for (listener, socket) in &listeners {
            accept(listener, socket);
        }
        // Docker is asked now and then, not on every pass.
        if checked.elapsed() >= Duration::from_secs(5) {
            checked = std::time::Instant::now();
            is_running = running(&endpoint, &preview.container);
        }
        if let Some(why) = reason_to_stop(crate::log::now(), preview.until, is_running) {
            break why;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    drop(listeners);
    if !stop(task, why)? {
        // The state lost its record: this forwarder still knows the container.
        let _ = crate::runner::docker(&endpoint)
            .args(["stop", "--time=10", &preview.container])
            .output();
        match std::fs::remove_dir_all(&sockets) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    Ok(())
}

/// The forwarder follows its container, not the task state: preview_stop and
/// a cancel stop the container, and a state saved without the preview (by an
/// older build) must not leave a running application with nothing listening.
fn reason_to_stop(now: u64, until: u64, is_running: bool) -> Option<&'static str> {
    if now >= until {
        Some("its time is up")
    } else if !is_running {
        Some("its container exited")
    } else {
        None
    }
}

fn running(endpoint: &str, container: &str) -> bool {
    crate::runner::docker(endpoint)
        .args(["inspect", "--format={{.State.Running}}", container])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
}

/// Takes the connections waiting on `listener`, each to its own thread.
fn accept(listener: &TcpListener, socket: &Path) {
    while let Ok((client, _)) = listener.accept() {
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            // The application may not be up yet: the browser then sees a
            // closed connection and tries again.
            if let Err(error) = relay(client, &socket) {
                eprintln!("preview connection: {error:#}");
            }
        });
    }
}

/// Copies bytes both ways between a browser connection and the socket.
fn relay(client: TcpStream, socket: &Path) -> Result<()> {
    client.set_nonblocking(false)?;
    let server = UnixStream::connect(socket)
        .with_context(|| format!("{} is not up yet", socket.display()))?;
    let (mut client_read, mut server_write) = (client.try_clone()?, server.try_clone()?);
    let upstream = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_read, &mut server_write);
        let _ = server_write.shutdown(std::net::Shutdown::Write);
    });
    let (mut server_read, mut client_write) = (server, client);
    let _ = std::io::copy(&mut server_read, &mut client_write);
    let _ = client_write.shutdown(std::net::Shutdown::Write);
    let _ = upstream.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn request() -> Request {
        serde_json::from_value(serde_json::json!({
            "image": format!("sha256:{}", "e".repeat(64)),
            "command": ["/opt/v1/bin/v1-target", "run", "/opt/v1/bin/v1-hold"],
            "env": {"V1_SOCKET_DIR": "/run/preview", "V1_PUBLIC_WEB_URL": "http://127.0.0.1:13000"},
            "sockets": {"web.sock": 13000, "api.sock": 18000},
            "hours": 4,
            "workdir": "/src",
        }))
        .unwrap()
    }

    fn refused(change: impl FnOnce(&mut Request)) -> String {
        let mut request = request();
        change(&mut request);
        format!("{:#}", request.validate().unwrap_err())
    }

    #[test]
    fn a_preview_request_is_bounded() {
        request().validate().unwrap();
        assert!(refused(|r| r.image = "v1-target:latest".into()).contains("sha256"));
        assert!(refused(|r| r.hours = 0).contains("1 to 8"));
        assert!(refused(|r| r.hours = 9).contains("1 to 8"));
        assert!(
            refused(|r| {
                r.sockets.insert("web.sock".into(), 80);
            })
            .contains("below 1024")
        );
        assert!(
            refused(|r| {
                r.sockets.insert("api.sock".into(), 13000);
            })
            .contains("its own port")
        );
        assert!(
            refused(|r| {
                r.sockets.insert("../x".into(), 14000);
            })
            .contains("plain file name")
        );
        assert!(
            refused(|r| {
                r.env.insert("DOCKER_HOST".into(), "tcp://x".into());
            })
            .contains("not allowed")
        );
        assert!(
            refused(|r| {
                r.env.insert("lower".into(), "x".into());
            })
            .contains("upper case")
        );
        assert!(refused(|r| r.workdir = "/src/../etc".into()).contains("absolute folder"));
        assert!(refused(|r| r.command.clear()).contains("non-empty"));
    }

    #[test]
    fn a_preview_keeps_a_check_containers_bounds_and_publishes_nothing() {
        let snapshot = tempfile::TempDir::new().unwrap();
        let sockets = tempfile::TempDir::new().unwrap();
        let args = docker_arguments(
            &request(),
            &crate::runner::tests::configuration(),
            "devagentics-preview-feat-x",
            snapshot.path(),
            sockets.path(),
        )
        .unwrap();
        for flag in ["--network=none", "--read-only", "--pull=never", "--rm"] {
            assert!(args.contains(&flag.to_string()), "{flag}: {args:?}");
        }
        assert!(args.contains(&format!(
            "type=bind,src={},dst=/src,readonly",
            snapshot.path().display()
        )));
        assert!(args.contains(&format!(
            "type=bind,src={},dst=/run/preview",
            sockets.path().display()
        )));
        assert!(
            !args.iter().any(|arg| arg.starts_with("-p")
                || arg.starts_with("--publish")
                || arg.contains("host")
                || arg.starts_with("--privileged")
                || arg.starts_with("--cap-add")),
            "{args:?}"
        );
        let image = args
            .iter()
            .position(|arg| arg.starts_with("sha256:"))
            .unwrap();
        assert_eq!(
            &args[image + 1..],
            ["/opt/v1/bin/v1-target", "run", "/opt/v1/bin/v1-hold"],
            "the command comes after the image, as its arguments"
        );
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("--env=V1_"))
                .count(),
            2
        );
    }

    #[test]
    fn a_preview_names_its_addresses_on_loopback() {
        let preview = Preview {
            container: "c".into(),
            image: "i".into(),
            sockets: request().sockets,
            until: 0,
            forwarder: None,
        };
        assert_eq!(
            preview.urls(),
            ["http://127.0.0.1:13000", "http://127.0.0.1:18000"]
        );
    }

    #[test]
    fn the_forwarder_relays_a_browser_connection_to_the_socket() {
        let folder = tempfile::TempDir::new().unwrap();
        let socket = folder.path().join("web.sock");
        let server = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut asked = [0; 4];
            stream.read_exact(&mut asked).unwrap();
            stream.write_all(b"pong").unwrap();
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut browser = TcpStream::connect(address).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        accept(&listener, &socket);
        browser.write_all(b"ping").unwrap();
        let mut answer = [0; 4];
        browser.read_exact(&mut answer).unwrap();
        assert_eq!(&answer, b"pong");
    }

    #[test]
    fn a_second_preview_is_refused_while_one_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        task.transaction(|tx| {
            tx.state.preview = Some(Preview {
                container: "c".into(),
                image: "i".into(),
                sockets: request().sockets,
                until: crate::log::now() + 3600,
                forwarder: None,
            });
            Ok(())
        })
        .unwrap();
        let error = format!("{:#}", start(&task, &request(), "d").unwrap_err());
        assert!(
            error.contains("already runs at http://127.0.0.1:13000"),
            "{error}"
        );
    }

    #[test]
    fn the_forwarder_stops_with_its_container_or_its_time_only() {
        assert_eq!(reason_to_stop(10, 20, true), None);
        assert_eq!(reason_to_stop(20, 20, true), Some("its time is up"));
        assert_eq!(reason_to_stop(10, 20, false), Some("its container exited"));
    }

    #[test]
    fn stopping_without_a_preview_does_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        assert!(!stop(&task, "asked").unwrap());
        assert!(!stop(&task, "asked").unwrap(), "twice is fine");
    }

    #[test]
    fn a_preview_whose_time_is_up_is_stopped_and_forgotten() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        task.transaction(|tx| {
            tx.state.preview = Some(Preview {
                container: "devagentics-preview-none".into(),
                image: "i".into(),
                sockets: request().sockets,
                until: 1,
                forwarder: None,
            });
            Ok(())
        })
        .unwrap();
        expire(&task).unwrap();
        assert!(task.state().unwrap().preview.is_none());
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("preview stopped: its time is up"))
        );
    }
}
