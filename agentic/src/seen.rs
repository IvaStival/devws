//! Which NEEDS YOU rows the user has already opened or answered, so the board
//! can tell them from new ones. Kept in the task folder (`board-seen.json`),
//! so a restarted board remembers; each mark is for the row's exact text, so
//! a row that changes (a new question from the leader) is new again.
use crate::board_view::{Entry, Seen};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const FILE: &str = "board-seen.json";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marks {
    opened: BTreeSet<String>,
    told: BTreeSet<String>,
    #[serde(skip)]
    path: PathBuf,
}

impl Marks {
    /// The marks of the task in `folder`; none when the file is missing.
    pub fn load(folder: &Path) -> Result<Self> {
        let path = folder.join(FILE);
        let mut marks: Self = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("reading {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        marks.path = path;
        Ok(marks)
    }

    /// The user opened `entry`'s details. Whether that is new.
    pub fn opened(&mut self, entry: &Entry) -> Result<bool> {
        self.record(entry, false)
    }

    /// The user wrote to the leader from `entry`'s details.
    pub fn told(&mut self, entry: &Entry) -> Result<bool> {
        self.record(entry, true)
    }

    fn record(&mut self, entry: &Entry, told: bool) -> Result<bool> {
        if !entry.attention {
            return Ok(false);
        }
        let key = key(entry);
        let mut added = self.opened.insert(key.clone());
        if told {
            added |= self.told.insert(key);
        }
        if added {
            self.save()?;
        }
        Ok(added)
    }

    /// Marks each NEEDS YOU row of `entries` as the user left it.
    pub fn apply(&self, entries: &mut [Entry]) {
        for entry in entries.iter_mut().filter(|entry| entry.attention) {
            let key = key(entry);
            entry.seen = if self.told.contains(&key) {
                Seen::Told
            } else if self.opened.contains(&key) {
                Seen::Opened
            } else {
                Seen::New
            };
        }
    }

    fn save(&self) -> Result<()> {
        let temporary = self.path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(self)?)?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("writing {}", self.path.display()))
    }
}

/// The row and its exact text: a changed row is a new one.
fn key(entry: &Entry) -> String {
    let digest = Sha256::digest(entry.detail.as_bytes());
    let short: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}#{short}", entry.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, detail: &str) -> Entry {
        Entry::text(id.into(), id.into(), detail.into(), true)
    }

    #[test]
    fn an_opened_then_answered_row_is_marked_and_remembered() {
        let folder = tempfile::TempDir::new().unwrap();
        let mut marks = Marks::load(folder.path()).unwrap();
        let mut entries = vec![
            row("permission:p234", "runner"),
            row("leader-asks", "A or B?"),
        ];
        assert!(marks.opened(&entries[0]).unwrap());
        assert!(!marks.opened(&entries[0]).unwrap(), "once is enough");
        assert!(marks.told(&entries[1]).unwrap());

        let reloaded = Marks::load(folder.path()).unwrap();
        reloaded.apply(&mut entries);
        assert_eq!(entries[0].seen, Seen::Opened);
        assert_eq!(entries[1].seen, Seen::Told);
    }

    #[test]
    fn a_changed_row_is_new_again_and_other_rows_are_never_marked() {
        let folder = tempfile::TempDir::new().unwrap();
        let mut marks = Marks::load(folder.path()).unwrap();
        marks.opened(&row("leader-asks", "A or B?")).unwrap();
        let mut entries = vec![row("leader-asks", "Approve revision 5?")];
        marks.apply(&mut entries);
        assert_eq!(entries[0].seen, Seen::New, "a new question");

        let agent = Entry::text(
            "agent:hammer-1".into(),
            "hammer-1".into(),
            "x".into(),
            false,
        );
        assert!(!marks.opened(&agent).unwrap(), "not a NEEDS YOU row");
        assert!(!folder.path().join("board-seen.tmp").exists());
    }
}
