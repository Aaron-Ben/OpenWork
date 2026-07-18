use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::backend::{AsyncFileSystem, LocalFileSystem, ProcessBackend, TokioProcessBackend};
use crate::policy::{AccessKind, PermissionProfile};

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
        }
    }

    pub(crate) fn check_path(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        self.permissions.allows(path, kind)
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
