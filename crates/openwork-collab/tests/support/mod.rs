#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use tempfile::TempDir;

pub async fn fake_opencode(directory: &TempDir) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-opencode.zsh");
    let executable = directory.path().join("fake-opencode");
    tokio::fs::copy(&source, &executable)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "copy shared fake OpenCode fixture from {}: {error}",
                source.display()
            )
        });
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();
    executable
}
