use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::WalkBuilder;

/// One file found by a traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    pub path: PathBuf,
    /// Last modification time; `None` when the platform cannot report it.
    pub modified: Option<SystemTime>,
}

/// Blocking traversal of the files under `root` that respects `.gitignore`
/// and skips hidden entries. Symlinks are not followed, so the walk stays
/// inside `root`. Consume it inside `spawn_blocking`.
pub(super) fn local_file_walk(
    root: &Path,
) -> impl Iterator<Item = io::Result<WalkEntry>> + Send + use<> {
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .build()
        .filter_map(|entry| match entry {
            Err(error) => Some(Err(io::Error::other(error.to_string()))),
            Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                Some(Ok(WalkEntry {
                    path: entry.into_path(),
                    modified,
                }))
            }
            Ok(_) => None,
        })
}
