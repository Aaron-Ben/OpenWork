use std::path::{Path, PathBuf};

use openwork_permissions::{AccessKind, PermissionProfile};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ExecutionContext {
    pub working_dir: PathBuf,
    pub permissions: PermissionProfile,
    pub cancel: CancellationToken,
}

impl ExecutionContext {
    pub fn new(
        working_dir: PathBuf,
        permissions: PermissionProfile,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            working_dir,
            permissions,
            cancel,
        }
    }

    pub(crate) fn check_path(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        self.permissions.allows(path, kind)
    }
}
