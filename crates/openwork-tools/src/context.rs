use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::policy::{AccessKind, PermissionProfile};

#[derive(Clone)]
pub struct ToolContext {
    pub working_directory: PathBuf,
    pub permissions: PermissionProfile,
    pub cancel: CancellationToken,
}

impl ToolContext {
    pub fn new(
        working_directory: PathBuf,
        permissions: PermissionProfile,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            working_directory,
            permissions,
            cancel,
        }
    }

    pub(crate) fn check_path(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        self.permissions.allows(path, kind)
    }
}
