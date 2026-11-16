//! The path to this devagentics binary, for the commands it hands to others.
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// What Linux appends to a running binary's /proc path once its file was
/// replaced, as `cargo install` does.
const DELETED: &str = " (deleted)";

/// The binary to start for a worker server, panel or leader. A long-running
/// panel outlives a reinstall: the replacement is used, never the removed
/// path, which no child could start (a Codex turn then ran without its worker
/// tools, silently).
pub fn own() -> Result<PathBuf> {
    usable(&std::env::current_exe()?)
}

fn usable(running: &Path) -> Result<PathBuf> {
    let path = installed(running);
    if !path.exists() {
        bail!(
            "devagentics at {} was removed; reinstall it or restart the task",
            path.display()
        );
    }
    Ok(path)
}

fn installed(running: &Path) -> PathBuf {
    let text = running.to_string_lossy();
    match text.strip_suffix(DELETED) {
        Some(original) => PathBuf::from(original),
        None => running.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_binary_resolves_to_its_replacement() {
        let folder = tempfile::tempdir().unwrap();
        let binary = folder.path().join("devagentics");
        std::fs::write(&binary, "").unwrap();
        let running = PathBuf::from(format!("{}{DELETED}", binary.display()));
        assert_eq!(usable(&running).unwrap(), binary);
    }

    #[test]
    fn a_present_binary_is_used_as_it_is() {
        let folder = tempfile::tempdir().unwrap();
        let binary = folder.path().join("devagentics");
        std::fs::write(&binary, "").unwrap();
        assert_eq!(usable(&binary).unwrap(), binary);
    }

    #[test]
    fn a_removed_binary_fails_with_what_to_do() {
        let folder = tempfile::tempdir().unwrap();
        let running = PathBuf::from(format!(
            "{}{DELETED}",
            folder.path().join("devagentics").display()
        ));
        let error = usable(&running).unwrap_err().to_string();
        assert!(
            error.contains("was removed; reinstall it or restart the task"),
            "{error}"
        );
    }
}
