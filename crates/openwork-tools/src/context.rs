use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Instant;

use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

use crate::ToolExecutionError;
use crate::backend::{AsyncFileSystem, LocalFileSystem, ProcessBackend, TokioProcessBackend};
use crate::policy::{
    AccessKind, FileSystemMode, PermissionProfile, lexical_normalize, path_is_within,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(String);

impl ToolCallId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct ToolCallContext {
    pub call_id: ToolCallId,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
}

impl ToolCallContext {
    pub fn new(call_id: ToolCallId, cancel: CancellationToken) -> Self {
        Self {
            call_id,
            cancel,
            deadline: None,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[derive(Clone)]
pub struct ToolSessionContext {
    pub working_directory: PathBuf,
    pub permissions: PermissionProfile,
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
    write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathIntent {
    MustExist,
    MayCreate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedPath {
    actual: PathBuf,
}

impl CheckedPath {
    pub(crate) fn as_path(&self) -> &Path {
        &self.actual
    }
}

impl ToolSessionContext {
    pub fn local(working_directory: PathBuf, permissions: PermissionProfile) -> Self {
        Self::new(
            working_directory,
            permissions,
            Arc::new(session_environment()),
            Arc::new(LocalFileSystem),
            Arc::new(TokioProcessBackend),
        )
    }

    pub fn new(
        working_directory: PathBuf,
        permissions: PermissionProfile,
        environment: Arc<HashMap<String, String>>,
        filesystem: Arc<dyn AsyncFileSystem>,
        process_backend: Arc<dyn ProcessBackend>,
    ) -> Self {
        Self {
            working_directory,
            permissions,
            environment,
            filesystem,
            process_backend,
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn check_path(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        self.permissions.allows(path, kind)
    }

    pub(crate) async fn resolve_path(
        &self,
        input: &str,
        kind: AccessKind,
        intent: PathIntent,
    ) -> Result<CheckedPath, ToolExecutionError> {
        let requested = Path::new(input);
        let unresolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.working_directory.join(requested)
        };
        let lexical = lexical_normalize(&unresolved);
        self.check_path(&lexical, kind)
            .map_err(ToolExecutionError::denied)?;

        let actual = match intent {
            PathIntent::MustExist => {
                self.filesystem
                    .canonicalize(&lexical)
                    .await
                    .map_err(|error| {
                        ToolExecutionError::execution(format!(
                            "failed to resolve {}: {error}",
                            lexical.display()
                        ))
                    })?
            }
            PathIntent::MayCreate => self.resolve_creatable_path(&lexical).await?,
        };

        self.check_canonical_path(&actual, kind).await?;
        Ok(CheckedPath { actual })
    }

    pub(crate) async fn lock_for_write(&self, path: &CheckedPath) -> OwnedMutexGuard<()> {
        let path_lock = {
            let mut locks = self.write_locks.lock().await;
            if let Some(lock) = locks.get(path.as_path()).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(path.actual.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        path_lock.lock_owned().await
    }

    async fn resolve_creatable_path(&self, lexical: &Path) -> Result<PathBuf, ToolExecutionError> {
        let mut anchor = lexical.to_path_buf();
        let mut suffix = Vec::<OsString>::new();

        loop {
            match self.filesystem.canonicalize(&anchor).await {
                Ok(canonical) => {
                    suffix.reverse();
                    return Ok(suffix
                        .into_iter()
                        .fold(canonical, |path, component| path.join(component)));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match self.filesystem.is_symlink(&anchor).await {
                        Ok(true) => {
                            return Err(ToolExecutionError::denied(format!(
                                "access denied through dangling symlink: {}",
                                anchor.display()
                            )));
                        }
                        Ok(false) => {
                            return Err(ToolExecutionError::execution(format!(
                                "failed to resolve existing path component: {}",
                                anchor.display()
                            )));
                        }
                        Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {
                        }
                        Err(metadata_error) => {
                            return Err(ToolExecutionError::execution(format!(
                                "failed to inspect {}: {metadata_error}",
                                anchor.display()
                            )));
                        }
                    }
                    let component = anchor.file_name().map(ToOwned::to_owned).ok_or_else(|| {
                        ToolExecutionError::execution(format!(
                            "failed to find an existing parent for {}",
                            lexical.display()
                        ))
                    })?;
                    suffix.push(component);
                    if !anchor.pop() {
                        return Err(ToolExecutionError::execution(format!(
                            "failed to find an existing parent for {}",
                            lexical.display()
                        )));
                    }
                }
                Err(error) => {
                    return Err(ToolExecutionError::execution(format!(
                        "failed to resolve {}: {error}",
                        anchor.display()
                    )));
                }
            }
        }
    }

    async fn check_canonical_path(
        &self,
        actual: &Path,
        kind: AccessKind,
    ) -> Result<(), ToolExecutionError> {
        if self.permissions.filesystem.mode == FileSystemMode::DangerFullAccess {
            return Ok(());
        }
        if kind == AccessKind::Write && self.permissions.is_protected(actual) {
            return Err(ToolExecutionError::denied(format!(
                "write access denied for protected metadata path: {}",
                actual.display()
            )));
        }

        let roots = match kind {
            AccessKind::Read => &self.permissions.filesystem.read_roots,
            AccessKind::Write => &self.permissions.filesystem.write_roots,
        };
        for root in roots {
            let canonical_root = self.filesystem.canonicalize(root).await.map_err(|error| {
                ToolExecutionError::denied(format!(
                    "failed to resolve permitted root {}: {error}",
                    root.display()
                ))
            })?;
            if path_is_within(actual, &canonical_root) {
                return Ok(());
            }
        }

        Err(ToolExecutionError::denied(format!(
            "{} access denied outside permitted roots after resolving symlinks: {}",
            match kind {
                AccessKind::Read => "read",
                AccessKind::Write => "write",
            },
            actual.display()
        )))
    }
}

fn session_environment() -> HashMap<String, String> {
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TMPDIR"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}
