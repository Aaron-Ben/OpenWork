//! Pieces shared by the two traversing tools, `grep` and `glob`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use globset::{Glob, GlobMatcher};
use tokio_util::sync::CancellationToken;

use crate::ToolExecutionError;

/// A traversal returns what it has found after this long (tools.md §9).
pub(super) const SCAN_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a traversal stopped before visiting every file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Interrupt {
    Cancelled,
    TimedOut,
}

/// Cancellation and the 30-second budget, checked from blocking code.
#[derive(Debug, Clone)]
pub(super) struct ScanBudget {
    cancel: CancellationToken,
    deadline: Instant,
}

impl ScanBudget {
    pub(super) fn new(cancel: CancellationToken, timeout: Duration) -> Self {
        Self {
            cancel,
            deadline: Instant::now() + timeout,
        }
    }

    pub(super) fn check(&self) -> Option<Interrupt> {
        if self.cancel.is_cancelled() {
            Some(Interrupt::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(Interrupt::TimedOut)
        } else {
            None
        }
    }
}

pub(super) fn compile_glob(pattern: &str) -> Result<GlobMatcher, ToolExecutionError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|error| {
            ToolExecutionError::invalid_arguments(format!(
                "invalid glob {pattern:?}: {error}. Use patterns like \"*.rs\" or \"src/**/*.ts\"."
            ))
        })
}

/// The path a glob is matched against: relative to the search root, as in
/// ripgrep, so `src/**/*.ts` under `path: "desktop"` means `desktop/src/…`.
pub(super) fn relative_to_root(root: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        path.file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        relative.to_path_buf()
    }
}

/// The path shown to the model: relative to the working directory when
/// inside it, so it can be passed straight to `read` or `edit`.
pub(super) use crate::path::display_path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_paths_are_workspace_relative_when_possible() {
        let workspace = Path::new("/work");
        assert_eq!(
            display_path(workspace, Path::new("/work/src/a.rs")),
            "src/a.rs"
        );
        assert_eq!(
            display_path(workspace, Path::new("/elsewhere/b.rs")),
            "/elsewhere/b.rs"
        );
    }

    #[test]
    fn globs_match_relative_to_the_search_root() {
        let root = Path::new("/work/desktop");
        let relative = relative_to_root(root, Path::new("/work/desktop/src/app.ts"));
        assert!(
            compile_glob("src/**/*.ts")
                .expect("glob")
                .is_match(&relative)
        );
        assert!(compile_glob("*.ts").expect("glob").is_match(&relative));
    }

    #[test]
    fn a_cancelled_budget_reports_cancellation_before_timeout() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            ScanBudget::new(cancel, Duration::ZERO).check(),
            Some(Interrupt::Cancelled)
        );
        assert_eq!(
            ScanBudget::new(CancellationToken::new(), Duration::ZERO).check(),
            Some(Interrupt::TimedOut)
        );
    }
}
