//! Lifecycle of spill directories (tools.md §10 "大结果落盘").
//!
//! Tools write the complete text behind bounded results into
//! `<root>/<session-id>/`. Core owns the rest of the lifecycle: a session's
//! directory goes with the session, and directories untouched for
//! [`SPILL_RETENTION`] are removed at startup.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use openwork_tools::SpillDirectory;

use crate::SessionId;

/// Spill directories older than this are removed when Core starts.
pub(crate) const SPILL_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpillRoot {
    path: PathBuf,
}

impl SpillRoot {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn session(&self, session_id: &SessionId) -> SpillDirectory {
        SpillDirectory::for_session(&self.path, session_id.as_str())
    }

    /// Removes the spill directories of deleted sessions. Failures are left
    /// for the startup sweep: a stale directory is harmless, a failed delete
    /// must not fail the session deletion.
    pub(crate) async fn remove_sessions(&self, session_ids: &[SessionId]) {
        for session_id in session_ids {
            let _ = tokio::fs::remove_dir_all(self.session(session_id).path()).await;
        }
    }

    /// Removes session directories whose last modification is older than
    /// `retention`. Returns how many were removed.
    pub(crate) async fn purge_older_than(&self, retention: Duration) -> usize {
        let root = self.path.clone();
        tokio::task::spawn_blocking(move || purge(&root, retention))
            .await
            .unwrap_or(0)
    }
}

fn purge(root: &Path, retention: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let now = SystemTime::now();
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > retention)
        })
        .filter(|entry| std::fs::remove_dir_all(entry.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "openwork-spill-root-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create root");
        root
    }

    #[tokio::test]
    async fn removing_a_session_removes_its_directory_only() {
        let root = temp_root("remove");
        let spill = SpillRoot::new(root.clone());
        let kept = spill.session(&SessionId::new("kept"));
        let removed = spill.session(&SessionId::new("removed"));
        std::fs::create_dir_all(kept.path()).expect("kept");
        std::fs::create_dir_all(removed.path()).expect("removed");
        std::fs::write(removed.path().join("call.txt"), "output").expect("spill file");

        spill.remove_sessions(&[SessionId::new("removed")]).await;

        assert!(kept.path().exists());
        assert!(!removed.path().exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn startup_sweep_removes_only_stale_directories() {
        let root = temp_root("purge");
        let spill = SpillRoot::new(root.clone());
        let fresh = spill.session(&SessionId::new("fresh"));
        let stale = spill.session(&SessionId::new("stale"));
        std::fs::create_dir_all(fresh.path()).expect("fresh");
        std::fs::create_dir_all(stale.path()).expect("stale");
        let old = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
        std::fs::File::open(stale.path())
            .expect("open stale dir")
            .set_modified(old)
            .expect("age stale dir");

        assert_eq!(spill.purge_older_than(SPILL_RETENTION).await, 1);

        assert!(fresh.path().exists());
        assert!(!stale.path().exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_missing_root_is_not_an_error() {
        let spill = SpillRoot::new(std::env::temp_dir().join("openwork-spill-root-missing-dir"));
        assert_eq!(spill.purge_older_than(SPILL_RETENTION).await, 0);
    }
}
