//! Runner images the team builds for itself: an infra agent writes the
//! Dockerfile, devAgentics builds it on the user's rootless daemon from the
//! project's own tracked files, and the leader puts the image ID in the
//! runner. A build has network (package downloads) and caps on memory, CPU
//! and time; no host folder is ever mounted into it.
use crate::runner::{self, ImageMount};
use crate::task::Task;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BUILD_MEMORY: &str = "4g";
/// Two CPUs, as a quota of the default 100 ms period.
const BUILD_CPU_QUOTA: &str = "200000";
const LOG_TAIL: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRequest {
    /// The image's name in its tag: `devws-runner/<repo>:<name>`.
    pub name: String,
    /// The repo whose files make the build context; the task's first by default.
    #[serde(default)]
    pub repo: Option<String>,
    pub dockerfile: String,
    /// Tracked files of the repo the Dockerfile copies (`backend/composer.lock`).
    #[serde(default)]
    pub context_files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Built {
    pub success: bool,
    /// The image ID to put in the runner, when it built.
    pub image: Option<String>,
    pub tag: String,
    pub timed_out: bool,
    pub log: PathBuf,
    /// The end of the build output.
    pub output: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TryRequest {
    pub image: String,
    #[serde(default)]
    pub image_mounts: Vec<ImageMount>,
    pub command: String,
}

/// Builds `request` into a local image.
pub fn build(task: &Task, request: &BuildRequest) -> Result<Built> {
    validate_name(&request.name)?;
    ensure!(
        !request.dockerfile.trim().is_empty() && request.dockerfile.len() <= 64 * 1024,
        "a Dockerfile of at most 64 KB is required"
    );
    let config = task.config()?;
    let repo = match &request.repo {
        Some(name) => config.repos.iter().find(|repo| repo.name == *name),
        None => config.repos.first(),
    }
    .context("unknown repo for the image build")?;
    let folder = task.dir().join("images").join(&request.name);
    let context = folder.join("context");
    if context.exists() {
        fs::remove_dir_all(&context)?;
    }
    fs::create_dir_all(&context)?;
    copy_context(&repo.worktree, &request.context_files, &context)?;
    fs::write(context.join("Dockerfile"), &request.dockerfile)?;

    let tag = format!("devws-runner/{}:{}", tag_part(&repo.name), request.name);
    let endpoint = runner::user_docker_endpoint()?;
    let log = folder.join("build.log");
    let file = fs::File::create(&log)?;
    let mut child = runner::docker(&endpoint)
        // The classic builder takes memory and CPU caps; BuildKit does not.
        .env("DOCKER_BUILDKIT", "0")
        .args(build_arguments(&tag, &config.id, &context))
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file)
        .spawn()
        .context("Docker must be installed and running")?;
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > BUILD_TIMEOUT {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let success = status.success() && !timed_out;
    let image = if success {
        Some(image_id(&endpoint, &tag)?)
    } else {
        None
    };
    Ok(Built {
        success,
        image,
        tag,
        timed_out,
        output: tail(&fs::read(&log)?),
        log,
    })
}

/// Runs `request.command` in `request.image` on `agent`'s read-only snapshot,
/// under the task's runner policy (no network, its memory, CPUs and timeout):
/// how an infra agent checks an image before the runner uses it.
pub fn try_image(task: &Task, agent: &str, request: &TryRequest) -> Result<runner::Output> {
    runner::validate_image(&request.image)?;
    runner::validate_mounts(&request.image_mounts)?;
    let state = task.state()?;
    let workspace = state
        .runner_workspaces
        .get(agent)
        .context("worker source missing")?;
    let mut config = state
        .runner
        .as_ref()
        .context("runner missing")?
        .for_lane(None);
    config.image = request.image.clone();
    config.image_mounts = request.image_mounts.clone();
    runner::run_container(&config, &workspace.snapshot, true, &request.command)
}

/// A repo name as a Docker repository path part: lowercase letters, digits
/// and single dashes, starting and ending with a letter or digit.
fn tag_part(name: &str) -> String {
    let mapped: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let part = mapped
        .split('-')
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if part.is_empty() { "repo".into() } else { part }
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 40
            && name.chars().all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'),
        "an image name is 1-40 lowercase letters, digits or -"
    );
    Ok(())
}

/// Copies each tracked `file` of `worktree` into `context` at the same path:
/// never a symlink, a path out of the repo, or a file git does not track.
fn copy_context(worktree: &Path, files: &[String], context: &Path) -> Result<()> {
    ensure!(files.len() <= 256, "at most 256 context files");
    for file in files {
        let source = runner::regular(worktree, file)?;
        crate::checks::git(worktree, &["ls-files", "--error-unmatch", "--", file])
            .with_context(|| format!("{file} is not a tracked file of the repo"))?;
        let target = context.join(file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, runner::bytes(&source)?)?;
    }
    Ok(())
}

/// The `docker build` arguments: the caps, the tag and the task's label.
fn build_arguments(tag: &str, task_id: &str, context: &Path) -> Vec<String> {
    vec![
        "build".into(),
        "--memory".into(),
        BUILD_MEMORY.into(),
        "--memory-swap".into(),
        BUILD_MEMORY.into(),
        "--cpu-quota".into(),
        BUILD_CPU_QUOTA.into(),
        "--label".into(),
        format!("devagentics.task={task_id}"),
        "--tag".into(),
        tag.into(),
        context.to_string_lossy().into_owned(),
    ]
}

fn image_id(endpoint: &str, tag: &str) -> Result<String> {
    let output = runner::docker(endpoint)
        .args(["image", "inspect", "--format", "{{.Id}}", tag])
        .output()?;
    ensure!(output.status.success(), "the built image {tag} is missing");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(LOG_TAIL);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::tests::{commit_file, repo};

    #[test]
    fn a_build_is_capped_tagged_and_labelled() {
        let arguments =
            build_arguments("devws-runner/app:back", "feat-x", Path::new("/tmp/context"));
        let joined = arguments.join(" ");
        assert!(
            joined.starts_with("build --memory 4g --memory-swap 4g --cpu-quota 200000"),
            "{joined}"
        );
        assert!(
            joined.contains("--label devagentics.task=feat-x"),
            "{joined}"
        );
        assert!(joined.contains("--tag devws-runner/app:back"), "{joined}");
        assert!(joined.ends_with("/tmp/context"), "{joined}");
        assert!(
            !joined.contains("--mount") && !joined.contains("-v "),
            "no host folder"
        );
    }

    #[test]
    fn the_context_holds_only_tracked_files_of_the_repo() {
        let repo = repo();
        commit_file(repo.path(), "backend/composer.lock", "{}\n", "lock");
        std::fs::write(repo.path().join("secret.env"), "TOKEN=x\n").unwrap();
        let context = tempfile::TempDir::new().unwrap();
        copy_context(
            repo.path(),
            &["backend/composer.lock".into()],
            context.path(),
        )
        .unwrap();
        assert!(context.path().join("backend/composer.lock").is_file());
        for refused in ["secret.env", "../outside", "/etc/passwd"] {
            assert!(
                copy_context(repo.path(), &[refused.into()], context.path()).is_err(),
                "{refused}"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", repo.path().join("link")).unwrap();
            assert!(copy_context(repo.path(), &["link".into()], context.path()).is_err());
        }
    }

    #[test]
    #[ignore = "requires the local rootless Docker daemon and alpine:3.20"]
    fn the_team_builds_an_image_from_its_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&dir);
        commit_file(repo.path(), "tool/version.txt", "1\n", "version");
        let built = build(
            &task,
            &BuildRequest {
                name: "probe".into(),
                repo: None,
                dockerfile: "FROM alpine:3.20\nCOPY tool/version.txt /version.txt\n".into(),
                context_files: vec!["tool/version.txt".into()],
            },
        )
        .unwrap();
        assert!(built.success, "{}", built.output);
        let image = built.image.unwrap();
        runner::validate_image(&image).unwrap();
        let endpoint = runner::user_docker_endpoint().unwrap();
        let _ = runner::docker(&endpoint)
            .args(["image", "rm", &built.tag])
            .output();
    }

    #[test]
    fn a_repo_name_becomes_a_valid_tag_part() {
        assert_eq!(tag_part("publisherProvider"), "publisherprovider");
        assert_eq!(tag_part(".tmpAB_c"), "tmpab-c");
        assert_eq!(tag_part("..."), "repo");
    }

    #[test]
    fn an_image_name_is_short_and_plain() {
        validate_name("back").unwrap();
        validate_name("php-8-4").unwrap();
        for bad in ["", "Back", "back end", "../x", &"a".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }
}
