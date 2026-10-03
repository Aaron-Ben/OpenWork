//! Read before edit (tools.md §7 "先读后改").
//!
//! One table per Session maps each file the model has read or written to the
//! hash of the content it saw. `write` and `edit` refuse an existing file that
//! is missing from the table (the model never looked at it) or whose content
//! changed since (bash or the user modified it in between).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::ToolExecutionError;

pub(crate) type ContentHash = [u8; 32];

/// Session-scoped and in memory only: after a restart the model reads a file
/// again before its first edit.
#[derive(Debug, Clone, Default)]
pub struct FileObservations {
    seen: Arc<Mutex<HashMap<PathBuf, ContentHash>>>,
}

impl FileObservations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the content the model has now seen at `path`, a canonical
    /// path from path resolution.
    pub(crate) fn record(&self, path: &Path, hash: ContentHash) {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.to_path_buf(), hash);
    }

    /// Checks that an existing file may be replaced: the model must have seen
    /// exactly its current content. `display` names the file in the error.
    pub(crate) fn check_current(
        &self,
        path: &Path,
        display: &str,
        current: &[u8],
    ) -> Result<(), ToolExecutionError> {
        let seen = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .copied();
        match seen {
            None => Err(ToolExecutionError::execution(format!(
                "Read {display} before editing it."
            ))),
            Some(hash) if hash != content_hash(current) => {
                Err(ToolExecutionError::execution(format!(
                    "{display} changed since you last read it (by you via bash, or by the user). Read it again before editing."
                )))
            }
            Some(_) => Ok(()),
        }
    }
}

pub(crate) fn content_hash(bytes: &[u8]) -> ContentHash {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseen_and_changed_files_are_refused() {
        let observations = FileObservations::new();
        let path = Path::new("/work/a.rs");

        let unseen = observations
            .check_current(path, "a.rs", b"one")
            .expect_err("never read");
        assert_eq!(unseen.message, "Read a.rs before editing it.");

        observations.record(path, content_hash(b"one"));
        observations
            .check_current(path, "a.rs", b"one")
            .expect("unchanged since read");

        let changed = observations
            .check_current(path, "a.rs", b"two")
            .expect_err("changed since read");
        assert!(
            changed
                .message
                .starts_with("a.rs changed since you last read it")
        );
    }

    #[test]
    fn clones_share_one_table() {
        let observations = FileObservations::new();
        let clone = observations.clone();
        clone.record(Path::new("/work/a.rs"), content_hash(b"x"));
        observations
            .check_current(Path::new("/work/a.rs"), "a.rs", b"x")
            .expect("shared");
    }
}
