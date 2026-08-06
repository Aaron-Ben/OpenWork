use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Instant;

use tokio::sync::{Mutex, OwnedMutexGuard, mpsc};
use tokio_util::sync::CancellationToken;

use crate::backend::{AsyncFileSystem, LocalFileSystem, ProcessBackend, TokioProcessBackend};
use crate::policy::{AccessKind, PermissionProfile, lexical_normalize, path_is_within};
use crate::{ExecutionPermit, ToolExecutionError, ToolProgress};

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
    progress: Option<mpsc::Sender<ToolProgress>>,
    execution_permit: Option<ExecutionPermit>,
}

impl ToolCallContext {
    pub fn new(call_id: ToolCallId, cancel: CancellationToken) -> Self {
        Self {
            call_id,
            cancel,
            deadline: None,
            progress: None,
            execution_permit: None,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_progress_sender(mut self, progress: mpsc::Sender<ToolProgress>) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn with_execution_permit(mut self, permit: ExecutionPermit) -> Self {
        self.execution_permit = Some(permit);
        self
    }

    pub(crate) fn execution_permit(&self) -> Option<&ExecutionPermit> {
        self.execution_permit.as_ref()
    }

    /// Reports live progress without applying backpressure to tool execution.
    ///
    /// Full or disconnected channels intentionally drop the progress item. The
    /// terminal tool result remains the source of truth.
    pub fn report_progress(&self, progress: ToolProgress) {
        if let Some(sender) = &self.progress {
            let _ = sender.try_send(progress);
        }
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
    pub fn normalize_effect_path(&self, input: &str) -> PathBuf {
        let requested = Path::new(input);
        let unresolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.working_directory.join(requested)
        };
        lexical_normalize(&unresolved)
    }

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
        self.permissions.allows_baseline(path, kind)
    }

    pub(crate) async fn resolve_path(
        &self,
        input: &str,
        kind: AccessKind,
        intent: PathIntent,
    ) -> Result<CheckedPath, ToolExecutionError> {
        self.resolve_path_inner(input, kind, intent, None).await
    }

    pub(crate) async fn resolve_tool_path(
        &self,
        input: &str,
        kind: AccessKind,
        intent: PathIntent,
        call: &ToolCallContext,
    ) -> Result<CheckedPath, ToolExecutionError> {
        self.resolve_path_inner(input, kind, intent, call.execution_permit())
            .await
    }

    async fn resolve_path_inner(
        &self,
        input: &str,
        kind: AccessKind,
        intent: PathIntent,
        permit: Option<&ExecutionPermit>,
    ) -> Result<CheckedPath, ToolExecutionError> {
        let requested = Path::new(input);
        let unresolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.working_directory.join(requested)
        };
        let lexical = lexical_normalize(&unresolved);
        let baseline_allowed = self.check_path(&lexical, kind).is_ok();
        if self.permissions.hard_denies(&lexical, kind) {
            return Err(ToolExecutionError::denied(format!(
                "write access denied for protected metadata path: {}",
                lexical.display()
            )));
        }
        if !baseline_allowed && !permit.is_some_and(|permit| permit.permits_path(&lexical, kind)) {
            return Err(ToolExecutionError::denied(format!(
                "{} access denied without an execution permit: {}",
                match kind {
                    AccessKind::Read => "read",
                    AccessKind::Write => "write",
                },
                lexical.display()
            )));
        }

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

        self.check_canonical_path(&lexical, &actual, kind, baseline_allowed)
            .await?;
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
        requested: &Path,
        actual: &Path,
        kind: AccessKind,
        baseline_allowed: bool,
    ) -> Result<(), ToolExecutionError> {
        let workspace = self.permissions.workspace();
        let canonical_workspace =
            self.filesystem
                .canonicalize(workspace)
                .await
                .map_err(|error| {
                    ToolExecutionError::denied(format!(
                        "failed to resolve permitted root {}: {error}",
                        workspace.display()
                    ))
                })?;
        let mut canonical_skill_roots = Vec::new();
        for root in self.permissions.skill_roots() {
            match self.filesystem.canonicalize(root).await {
                Ok(canonical) => canonical_skill_roots.push(canonical),
                Err(error) if path_is_within(requested, root) => {
                    return Err(ToolExecutionError::denied(format!(
                        "failed to resolve permitted root {}: {error}",
                        root.display()
                    )));
                }
                Err(_) => {}
            }
        }
        if self.permissions.hard_denies(actual, kind)
            || self
                .permissions
                .hard_denies_resolved(actual, &canonical_workspace, kind)
            || (kind == AccessKind::Write
                && canonical_skill_roots
                    .iter()
                    .any(|root| path_is_within(actual, root)))
        {
            return Err(ToolExecutionError::denied(format!(
                "write access denied for protected metadata path: {}",
                actual.display()
            )));
        }

        if !baseline_allowed && !path_is_within(requested, workspace) {
            return Ok(());
        }

        if path_is_within(actual, &canonical_workspace)
            || (kind == AccessKind::Read
                && canonical_skill_roots
                    .iter()
                    .any(|root| path_is_within(actual, root)))
        {
            return Ok(());
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

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::ToolProgress;

    #[tokio::test]
    async fn tool_call_context_reports_best_effort_progress() {
        let (progress_tx, mut progress_rx) = mpsc::channel(1);
        let call = ToolCallContext::new(ToolCallId::new("progress"), CancellationToken::new())
            .with_progress_sender(progress_tx);

        call.report_progress(ToolProgress::Message {
            message: "working".to_string(),
        });

        assert_eq!(
            progress_rx.recv().await,
            Some(ToolProgress::Message {
                message: "working".to_string()
            })
        );
    }

    #[test]
    fn disconnected_progress_consumer_does_not_fail_the_tool_call() {
        let (progress_tx, progress_rx) = mpsc::channel(1);
        drop(progress_rx);
        let call = ToolCallContext::new(ToolCallId::new("disconnected"), CancellationToken::new())
            .with_progress_sender(progress_tx);

        call.report_progress(ToolProgress::Message {
            message: "ignored".to_string(),
        });
    }
}
