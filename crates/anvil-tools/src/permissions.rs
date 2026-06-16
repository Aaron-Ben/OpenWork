use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    Restricted,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSystemPermissions {
    pub mode: FileSystemMode,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub protected_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionProfile {
    pub filesystem: FileSystemPermissions,
    pub network: NetworkMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Write,
}

impl PermissionProfile {
    pub fn workspace_write(working_dir: impl Into<PathBuf>) -> Self {
        let working_dir = working_dir.into();
        Self {
            filesystem: FileSystemPermissions {
                mode: FileSystemMode::WorkspaceWrite,
                read_roots: vec![working_dir.clone()],
                write_roots: vec![working_dir, std::env::temp_dir()],
                protected_names: vec![".git".into(), ".agents".into(), ".codex".into()],
            },
            network: NetworkMode::Restricted,
        }
    }

    pub fn danger_full_access() -> Self {
        Self {
            filesystem: FileSystemPermissions {
                mode: FileSystemMode::DangerFullAccess,
                read_roots: Vec::new(),
                write_roots: Vec::new(),
                protected_names: Vec::new(),
            },
            network: NetworkMode::Enabled,
        }
    }

    pub fn allows(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        match self.filesystem.mode {
            FileSystemMode::DangerFullAccess => Ok(()),
            FileSystemMode::ReadOnly if kind == AccessKind::Write => Err(format!(
                "write access denied by read-only profile: {}",
                path.display()
            )),
            FileSystemMode::ReadOnly | FileSystemMode::WorkspaceWrite => {
                let roots = match kind {
                    AccessKind::Read => &self.filesystem.read_roots,
                    AccessKind::Write => &self.filesystem.write_roots,
                };
                if roots.iter().any(|root| path_is_within(path, root)) {
                    if kind == AccessKind::Write && self.is_protected(path) {
                        return Err(format!(
                            "write access denied for protected metadata path: {}",
                            path.display()
                        ));
                    }
                    Ok(())
                } else {
                    Err(format!(
                        "{} access denied outside permitted roots: {}",
                        match kind {
                            AccessKind::Read => "read",
                            AccessKind::Write => "write",
                        },
                        path.display()
                    ))
                }
            }
        }
    }

    fn is_protected(&self, path: &Path) -> bool {
        path.components().any(|component| {
            let Component::Normal(name) = component else {
                return false;
            };
            self.filesystem
                .protected_names
                .iter()
                .any(|protected| name == protected.as_str())
        })
    }
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    lexical_normalize(path).starts_with(lexical_normalize(root))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            _ => out.push(component.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_write_allows_workspace_read_and_write() {
        let profile = PermissionProfile::workspace_write("/repo");
        assert!(
            profile
                .allows(Path::new("/repo/src/main.rs"), AccessKind::Read)
                .is_ok()
        );
        assert!(
            profile
                .allows(Path::new("/repo/src/main.rs"), AccessKind::Write)
                .is_ok()
        );
    }

    #[test]
    fn workspace_write_denies_outside_write() {
        let profile = PermissionProfile::workspace_write("/repo");
        assert!(
            profile
                .allows(Path::new("/other/file"), AccessKind::Write)
                .is_err()
        );
    }

    #[test]
    fn workspace_write_denies_protected_metadata_write() {
        let profile = PermissionProfile::workspace_write("/repo");
        assert!(
            profile
                .allows(Path::new("/repo/.git/config"), AccessKind::Write)
                .is_err()
        );
    }
}
