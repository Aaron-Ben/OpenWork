use std::path::{Path, PathBuf};

use crate::permission::builtin::BuiltinRuleSet;

use super::filesystem::{AccessKind, path_is_within};

#[derive(Debug, Clone)]
pub struct PermissionProfile {
    builtins: BuiltinRuleSet,
}

impl PermissionProfile {
    pub fn from_builtin_rules(working_directory: impl Into<PathBuf>) -> Self {
        Self {
            builtins: BuiltinRuleSet::for_workspace(working_directory),
        }
    }

    pub(crate) fn workspace(&self) -> &Path {
        self.builtins.workspace()
    }

    pub(crate) fn hard_denies(&self, path: &Path, kind: AccessKind) -> bool {
        kind == AccessKind::Write && self.builtins.hard_deny_write(path)
    }

    pub(crate) fn hard_denies_resolved(
        &self,
        path: &Path,
        resolved_workspace: &Path,
        kind: AccessKind,
    ) -> bool {
        if kind != AccessKind::Write {
            return false;
        }
        path.strip_prefix(resolved_workspace)
            .ok()
            .is_some_and(|relative| {
                self.builtins
                    .hard_deny_write(&self.builtins.workspace().join(relative))
            })
    }

    pub(crate) fn allows_baseline(&self, path: &Path, kind: AccessKind) -> Result<(), String> {
        if self.hard_denies(path, kind) {
            return Err(format!(
                "write access denied for protected metadata path: {}",
                path.display()
            ));
        }
        if path_is_within(path, self.workspace()) {
            Ok(())
        } else {
            Err(format!(
                "{} access denied outside the workspace: {}",
                match kind {
                    AccessKind::Read => "read",
                    AccessKind::Write => "write",
                },
                path.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_is_derived_from_builtin_workspace_and_hard_denies() {
        let profile = PermissionProfile::from_builtin_rules("/repo");
        assert!(
            profile
                .allows_baseline(Path::new("/repo/src/main.rs"), AccessKind::Write)
                .is_ok()
        );
        assert!(
            profile
                .allows_baseline(Path::new("/repo/.git/config"), AccessKind::Write)
                .is_err()
        );
        assert!(
            profile
                .allows_baseline(Path::new("/repo/.env"), AccessKind::Write)
                .is_ok()
        );
    }
}
