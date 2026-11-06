use anyhow::{Context, Result};
use std::path::PathBuf;

/// Where devAgentics keeps its tasks: `DEVAGENTICS_HOME`, else `~/.devagentics`.
#[derive(Clone, Debug)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    pub fn from_env() -> Result<Self> {
        if let Some(root) = std::env::var_os("DEVAGENTICS_HOME") {
            return Ok(Self::at(root.into()));
        }
        let user_home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(Self::at(PathBuf::from(user_home).join(".devagentics")))
    }

    /// Under test, a home starts with the sample planner and executor as its
    /// user's roles, as a real home has its own.
    pub fn at(root: PathBuf) -> Self {
        #[cfg(test)]
        for (name, text) in crate::roles::SAMPLES {
            let dir = root.join("roles");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{name}.md"));
            if !path.exists() {
                std::fs::write(path, text).unwrap();
            }
        }
        Self { root }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The user's own roles, which add to the built-in leader and can override it.
    pub fn roles_dir(&self) -> PathBuf {
        self.root.join("roles")
    }

    pub fn task_dir(&self, id: &str) -> PathBuf {
        self.root.join("tasks").join(id)
    }

    /// The ids of the tasks, in order.
    pub fn task_ids(&self) -> Result<Vec<String>> {
        let tasks = self.root.join("tasks");
        if !tasks.exists() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in
            std::fs::read_dir(&tasks).with_context(|| format!("reading {}", tasks.display()))?
        {
            let entry = entry?;
            if entry.path().join("task.toml").is_file() {
                ids.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }
}
