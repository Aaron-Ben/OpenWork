//! What one call may read and write (permissions.md §2.2–§2.4, §4.2).

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::tiers;

/// How much the workspace is open to a call. Ordered from narrow to wide, so
/// the effective mode of a sub-agent is `parent.min(role_ceiling)`. Users
/// and roles choose from the same two values (permissions.md §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// `write` / `edit` may change the workspace; bash may not.
    AcceptEdits,
    /// Both may change the workspace. The default.
    Auto,
}

impl SandboxMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AcceptEdits => "accept_edits",
            Self::Auto => "auto",
        }
    }
}

/// Who is asking. The modes differ only in whether bash may write the
/// workspace, so the rules need to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    Bash,
    FileTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    /// Implies read.
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Exact,
    Subtree,
}

/// One path a single call may additionally read or write (permissions.md
/// §4.1). Grants live only in that call's policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathGrant {
    /// Canonical absolute path.
    pub path: PathBuf,
    pub access: Access,
    pub scope: GrantScope,
}

impl PathGrant {
    pub(crate) fn covers(&self, path: &Path) -> bool {
        match self.scope {
            GrantScope::Exact => path == self.path,
            GrantScope::Subtree => path.starts_with(&self.path),
        }
    }
}

/// The tier a path belongs to (permissions.md §2.3), for cards and errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathTier {
    HardProtected,
    Sensitive,
    Credential,
    Normal,
}

/// Why an access was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Denial {
    /// Never writable, in any mode, under any grant.
    HardProtected,
    /// Read-only until a grant names it.
    Sensitive,
    /// Unreadable (and unwritable) until a grant names it.
    Credential,
    /// Outside the writable roots of this mode and actor.
    OutsideWritableRoots,
}

/// Machine facts every policy shares: where home, temp and the protected
/// OpenWork directories are. Paths are canonical: Seatbelt matches real
/// paths, and `/tmp` is really `/private/tmp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxEnvironment {
    home: PathBuf,
    temp_roots: Vec<PathBuf>,
    openwork_home: PathBuf,
    skill_roots: Vec<PathBuf>,
}

impl SandboxEnvironment {
    pub fn new(home: PathBuf, temp_roots: Vec<PathBuf>, skill_roots: Vec<PathBuf>) -> Self {
        let openwork_home = home.join(".openwork");
        Self {
            home,
            temp_roots,
            openwork_home,
            skill_roots,
        }
    }

    /// Reads `$HOME` and `$TMPDIR` and canonicalizes everything.
    pub fn detect(skill_roots: impl IntoIterator<Item = PathBuf>) -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        let home = canonical_or_lexical(&home);
        let mut temp_roots = vec![canonical_or_lexical(Path::new("/tmp"))];
        if let Some(tmpdir) = std::env::var_os("TMPDIR") {
            let tmpdir = canonical_or_lexical(Path::new(&tmpdir));
            if !temp_roots.contains(&tmpdir) {
                temp_roots.push(tmpdir);
            }
        }
        let skill_roots = skill_roots
            .into_iter()
            .map(|root| canonical_or_lexical(&root))
            .collect();
        Ok(Self::new(home, temp_roots, skill_roots))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn temp_roots(&self) -> &[PathBuf] {
        &self.temp_roots
    }

    /// bash 启动时必须设置的环境变量（permissions.md §3.1 工具链缓存），覆盖用户环境里的同名变量。
    ///
    /// 缓存放在最后一个临时根下：`detect` 把本用户的 `$TMPDIR` 排在最后。它是 OpenWork
    /// 私有的，不与用户自己的缓存共用，沙箱内的进程因此篡改不了用户在沙箱外编译时取到的产物。
    pub fn bash_environment(&self) -> Vec<(&'static str, PathBuf)> {
        self.temp_roots
            .last()
            .map(|temp| {
                (
                    GO_CACHE_VARIABLE,
                    temp.join(PRIVATE_CACHE_DIRECTORY).join("go-build"),
                )
            })
            .into_iter()
            .collect()
    }

    /// `~/.openwork` and the skill roots.
    pub(crate) fn hard_protected_roots(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.openwork_home.as_path())
            .chain(self.skill_roots.iter().map(PathBuf::as_path))
    }
}

/// Go 编译缓存的位置变量；默认值 `~/Library/Caches/go-build` 在沙箱外。
const GO_CACHE_VARIABLE: &str = "GOCACHE";

/// 临时根下 OpenWork 私有缓存的目录名。
const PRIVATE_CACHE_DIRECTORY: &str = "openwork";

fn canonical_or_lexical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Most paths one escalation may name (permissions.md §4.2).
pub const MAX_GRANTS: usize = 16;

/// The policy of one call: the session's mode, the workspace, and the grants
/// the user approved for this call only. Bash's Seatbelt profile and the
/// file tools' fence are both derived from it (permissions.md §2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    /// Canonical.
    pub workspace_root: PathBuf,
    pub path_grants: Vec<PathGrant>,
    environment: Arc<SandboxEnvironment>,
}

impl SandboxPolicy {
    pub fn new(
        mode: SandboxMode,
        workspace_root: PathBuf,
        environment: Arc<SandboxEnvironment>,
    ) -> Self {
        Self {
            mode,
            workspace_root,
            path_grants: Vec::new(),
            environment,
        }
    }

    pub fn with_grants(mut self, grants: Vec<PathGrant>) -> Self {
        self.path_grants = grants;
        self
    }

    pub fn environment(&self) -> &SandboxEnvironment {
        &self.environment
    }

    /// Whether `actor` may perform `access` on the canonical `path`.
    pub fn check(&self, path: &Path, access: Access, actor: Actor) -> Result<(), Denial> {
        match access {
            Access::Read => self.check_read(path),
            Access::Write => self.check_write(path, actor),
        }
    }

    pub fn tier(&self, path: &Path) -> PathTier {
        if self.is_hard_protected(path) {
            PathTier::HardProtected
        } else if self.is_sensitive(path) {
            PathTier::Sensitive
        } else if self.is_credential(path) {
            PathTier::Credential
        } else {
            PathTier::Normal
        }
    }

    fn check_read(&self, path: &Path) -> Result<(), Denial> {
        // A grant of either kind unlocks reading what it covers; Seatbelt
        // carves every grant out of the credential `deny file-read*`.
        if self.is_credential(path) && !self.path_grants.iter().any(|grant| grant.covers(path)) {
            return Err(Denial::Credential);
        }
        Ok(())
    }

    fn check_write(&self, path: &Path, actor: Actor) -> Result<(), Denial> {
        if tiers::is_writable_device(path) {
            return Ok(());
        }
        if self.is_hard_protected(path) {
            return Err(Denial::HardProtected);
        }
        let sensitive = self.is_sensitive(path);
        let credential = self.is_credential(path);
        if !sensitive
            && !credential
            && self
                .base_writable_roots(actor)
                .iter()
                .any(|root| path.starts_with(root))
        {
            return Ok(());
        }
        if self
            .write_grants()
            .any(|grant| grant.covers(path) && self.grant_unlocks(grant, sensitive, credential))
        {
            return Ok(());
        }
        Err(if sensitive {
            Denial::Sensitive
        } else if credential {
            Denial::Credential
        } else {
            Denial::OutsideWritableRoots
        })
    }

    /// A grant unlocks a sensitive or credential path only when it names one
    /// itself: granting the whole workspace does not open `.git`.
    pub(crate) fn grant_unlocks(
        &self,
        grant: &PathGrant,
        sensitive: bool,
        credential: bool,
    ) -> bool {
        (!sensitive || self.is_sensitive(&grant.path))
            && (!credential || self.is_credential(&grant.path))
    }

    /// Temp roots, plus the workspace when this mode lets `actor` write it.
    pub(crate) fn base_writable_roots(&self, actor: Actor) -> Vec<&Path> {
        let workspace_writable = match (actor, self.mode) {
            (Actor::FileTool, _) => true,
            (Actor::Bash, SandboxMode::AcceptEdits) => false,
            (Actor::Bash, SandboxMode::Auto) => !self.workspace_contains_home(),
        };
        let mut roots = self
            .environment
            .temp_roots
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        if workspace_writable {
            roots.push(&self.workspace_root);
        }
        roots
    }

    /// 工作区是主目录或它的上级（permissions.md §2.2）。这时工作区里有 `~/Library/LaunchAgents`
    /// 这类会在沙箱外被执行的位置，列举不全，所以两个模式下 bash 都不能写工作区。
    fn workspace_contains_home(&self) -> bool {
        self.environment.home.starts_with(&self.workspace_root)
    }

    pub(crate) fn write_grants(&self) -> impl Iterator<Item = &PathGrant> {
        self.path_grants
            .iter()
            .filter(|grant| grant.access == Access::Write)
    }

    pub(crate) fn is_hard_protected(&self, path: &Path) -> bool {
        self.environment
            .hard_protected_roots()
            .any(|root| path.starts_with(root))
            || tiers::is_workspace_hard_protected(&self.workspace_root, path)
    }

    pub(crate) fn is_sensitive(&self, path: &Path) -> bool {
        tiers::is_workspace_sensitive(&self.workspace_root, path)
    }

    pub(crate) fn is_credential(&self, path: &Path) -> bool {
        tiers::credential_paths(&self.environment.home)
            .iter()
            .any(|credential| path.starts_with(credential))
    }

    /// Checks the paths of an escalation request against the current policy
    /// (permissions.md §4.2). Justification and approval are Core's.
    pub fn validate_grants(&self, grants: &[PathGrant], actor: Actor) -> Result<(), GrantError> {
        if grants.is_empty() {
            return Err(GrantError::Empty);
        }
        if grants.len() > MAX_GRANTS {
            return Err(GrantError::TooMany {
                count: grants.len(),
            });
        }
        for grant in grants {
            if !is_normalized_absolute(&grant.path) {
                return Err(GrantError::NotAbsolute(grant.path.clone()));
            }
            if self.is_hard_protected(&grant.path) {
                return Err(GrantError::HardProtected(grant.path.clone()));
            }
            if grant.scope == GrantScope::Subtree && self.is_too_broad(&grant.path) {
                return Err(GrantError::TooBroad(grant.path.clone()));
            }
            if self.check(&grant.path, grant.access, actor).is_ok() {
                return Err(GrantError::NoNewPermission {
                    path: grant.path.clone(),
                    access: grant.access,
                });
            }
        }
        Ok(())
    }

    /// `/`, `$HOME`, and their ancestors: a subtree that wide is the "write
    /// anywhere" permission OpenWork does not offer.
    fn is_too_broad(&self, path: &Path) -> bool {
        self.environment.home.starts_with(path)
    }
}

fn is_normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

/// Why an escalation request is refused without asking the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    Empty,
    TooMany { count: usize },
    NotAbsolute(PathBuf),
    HardProtected(PathBuf),
    TooBroad(PathBuf),
    NoNewPermission { path: PathBuf, access: Access },
}

impl std::fmt::Display for GrantError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(
                formatter,
                "sandbox_permissions.paths is empty; list the paths the command needs"
            ),
            Self::TooMany { count } => write!(
                formatter,
                "sandbox_permissions.paths has {count} entries; at most {MAX_GRANTS} are allowed"
            ),
            Self::NotAbsolute(path) => write!(
                formatter,
                "{} is not a normalized absolute path",
                path.display()
            ),
            Self::HardProtected(path) => write!(
                formatter,
                "{} is protected in every mode and cannot be granted; do not retry",
                path.display()
            ),
            Self::TooBroad(path) => write!(
                formatter,
                "a subtree grant on {} is too broad; name the specific directory the command writes",
                path.display()
            ),
            Self::NoNewPermission { path, access } => write!(
                formatter,
                "{} is already {} under the current policy; remove it from sandbox_permissions",
                path.display(),
                match access {
                    Access::Read => "readable",
                    Access::Write => "writable",
                }
            ),
        }
    }
}

impl std::error::Error for GrantError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> Arc<SandboxEnvironment> {
        Arc::new(SandboxEnvironment::new(
            PathBuf::from("/home/me"),
            vec![
                PathBuf::from("/private/tmp"),
                PathBuf::from("/private/var/folders/t"),
            ],
            vec![PathBuf::from("/home/me/.agents/skills")],
        ))
    }

    fn policy(mode: SandboxMode) -> SandboxPolicy {
        SandboxPolicy::new(mode, PathBuf::from("/home/me/project"), environment())
    }

    fn write(policy: &SandboxPolicy, path: &str, actor: Actor) -> Result<(), Denial> {
        policy.check(Path::new(path), Access::Write, actor)
    }

    fn grant(path: &str, access: Access, scope: GrantScope) -> PathGrant {
        PathGrant {
            path: PathBuf::from(path),
            access,
            scope,
        }
    }

    #[test]
    fn modes_differ_only_in_whether_bash_may_write_the_workspace() {
        let source = "/home/me/project/src/main.rs";
        for actor in [Actor::Bash, Actor::FileTool] {
            assert_eq!(write(&policy(SandboxMode::Auto), source, actor), Ok(()));
            assert_eq!(
                write(&policy(SandboxMode::Auto), "/private/tmp/x", actor),
                Ok(())
            );
            assert_eq!(
                write(&policy(SandboxMode::AcceptEdits), "/private/tmp/x", actor),
                Ok(())
            );
            assert_eq!(
                write(&policy(SandboxMode::AcceptEdits), "/dev/null", actor),
                Ok(())
            );
        }
        assert_eq!(
            write(&policy(SandboxMode::AcceptEdits), source, Actor::FileTool),
            Ok(())
        );
        assert_eq!(
            write(&policy(SandboxMode::AcceptEdits), source, Actor::Bash),
            Err(Denial::OutsideWritableRoots)
        );
    }

    /// permissions.md §2.2、§9.2 #49：主目录或它的上级作工作区时，bash 在两个模式下都不能写工作区。
    #[test]
    fn a_workspace_containing_home_is_read_only_for_bash_in_every_mode() {
        let launch_agent = "/home/me/Library/LaunchAgents/x.plist";
        for workspace in ["/home/me", "/home"] {
            for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
                let policy = SandboxPolicy::new(mode, PathBuf::from(workspace), environment());
                assert_eq!(
                    write(&policy, launch_agent, Actor::Bash),
                    Err(Denial::OutsideWritableRoots),
                    "{workspace} in {mode:?}"
                );
                assert_eq!(write(&policy, launch_agent, Actor::FileTool), Ok(()));
                assert_eq!(write(&policy, "/private/tmp/x", Actor::Bash), Ok(()));
            }
        }
        let sibling = SandboxPolicy::new(
            SandboxMode::Auto,
            PathBuf::from("/home/other"),
            environment(),
        );
        assert_eq!(write(&sibling, "/home/other/x", Actor::Bash), Ok(()));
        assert_eq!(
            write(
                &policy(SandboxMode::Auto),
                "/home/me/project/x",
                Actor::Bash
            ),
            Ok(())
        );
    }

    /// permissions.md §3.1、§9.2 #50：Go 的缓存指向用户临时目录下 OpenWork 私有的位置，两个模式下 bash 都能写。
    #[test]
    fn bash_environment_points_the_go_cache_at_a_private_temp_directory() {
        let cache = PathBuf::from("/private/var/folders/t/openwork/go-build");
        assert_eq!(
            environment().bash_environment(),
            vec![("GOCACHE", cache.clone())]
        );
        for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
            assert_eq!(
                policy(mode).check(&cache.join("00/x"), Access::Write, Actor::Bash),
                Ok(())
            );
        }
    }

    #[test]
    fn modes_order_from_narrow_to_wide() {
        assert_eq!(
            SandboxMode::AcceptEdits.min(SandboxMode::Auto),
            SandboxMode::AcceptEdits
        );
    }

    #[test]
    fn every_tier_is_classified() {
        let policy = policy(SandboxMode::Auto);
        let tier = |path: &str| policy.tier(Path::new(path));
        assert_eq!(tier("/home/me/.openwork/agents/a"), PathTier::HardProtected);
        assert_eq!(
            tier("/home/me/.agents/skills/review/SKILL.md"),
            PathTier::HardProtected
        );
        assert_eq!(
            tier("/home/me/project/.git/hooks/pre-commit"),
            PathTier::HardProtected
        );
        assert_eq!(tier("/home/me/project/.git/config"), PathTier::Sensitive);
        assert_eq!(tier("/home/me/project/.env"), PathTier::Sensitive);
        assert_eq!(tier("/home/me/.ssh/id_ed25519"), PathTier::Credential);
        assert_eq!(tier("/home/me/.docker/config.json"), PathTier::Credential);
        assert_eq!(tier("/home/me/project/src/lib.rs"), PathTier::Normal);
    }

    #[test]
    fn protected_tiers_hold_in_every_mode() {
        for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
            let policy = policy(mode);
            for actor in [Actor::Bash, Actor::FileTool] {
                assert_eq!(
                    write(&policy, "/home/me/project/.git/index.lock", actor),
                    Err(Denial::Sensitive)
                );
                assert_eq!(
                    write(&policy, "/home/me/project/.git/hooks/pre-commit", actor),
                    Err(Denial::HardProtected)
                );
                assert_eq!(
                    write(&policy, "/home/me/.openwork/x", actor),
                    Err(Denial::HardProtected)
                );
                assert_eq!(
                    write(&policy, "/home/me/notes.txt", actor),
                    Err(Denial::OutsideWritableRoots)
                );
                assert_eq!(
                    policy.check(Path::new("/home/me/.ssh/config"), Access::Read, actor),
                    Err(Denial::Credential)
                );
                assert_eq!(
                    policy.check(Path::new("/home/me/.cargo/registry/x"), Access::Read, actor),
                    Ok(())
                );
            }
        }
    }

    #[test]
    fn grants_unlock_what_they_name_but_never_hard_protected_paths() {
        let policy = policy(SandboxMode::Auto).with_grants(vec![
            grant("/home/me/project/.git", Access::Write, GrantScope::Subtree),
            grant("/home/me/.ssh", Access::Read, GrantScope::Subtree),
            grant(
                "/home/me/.cargo/registry",
                Access::Write,
                GrantScope::Subtree,
            ),
        ]);
        assert_eq!(
            write(&policy, "/home/me/project/.git/index.lock", Actor::Bash),
            Ok(())
        );
        assert_eq!(
            write(
                &policy,
                "/home/me/project/.git/hooks/pre-commit",
                Actor::Bash
            ),
            Err(Denial::HardProtected)
        );
        assert_eq!(
            policy.check(
                Path::new("/home/me/.ssh/known_hosts"),
                Access::Read,
                Actor::Bash
            ),
            Ok(())
        );
        assert_eq!(
            write(&policy, "/home/me/.ssh/known_hosts", Actor::Bash),
            Err(Denial::Credential),
            "a read grant does not allow writing"
        );
        assert_eq!(
            write(
                &policy,
                "/home/me/.cargo/registry/cache/x.crate",
                Actor::Bash
            ),
            Ok(())
        );
        assert_eq!(
            write(&policy, "/home/me/.cargo/other", Actor::Bash),
            Err(Denial::OutsideWritableRoots)
        );
    }

    #[test]
    fn a_broad_grant_does_not_open_protected_paths_it_merely_contains() {
        let policy = policy(SandboxMode::AcceptEdits).with_grants(vec![grant(
            "/home/me/project",
            Access::Write,
            GrantScope::Subtree,
        )]);
        assert_eq!(
            write(&policy, "/home/me/project/target/debug/app", Actor::Bash),
            Ok(())
        );
        assert_eq!(
            write(&policy, "/home/me/project/.env", Actor::Bash),
            Err(Denial::Sensitive)
        );
    }

    #[test]
    fn escalation_requests_are_validated() {
        let auto = policy(SandboxMode::Auto);
        let validate = |grants: &[PathGrant]| auto.validate_grants(grants, Actor::Bash);

        assert_eq!(
            validate(&[grant(
                "/home/me/project/.git",
                Access::Write,
                GrantScope::Subtree
            )]),
            Ok(())
        );
        assert_eq!(validate(&[]), Err(GrantError::Empty));
        let many = vec![grant("/home/me/.ssh", Access::Read, GrantScope::Subtree); 17];
        assert_eq!(validate(&many), Err(GrantError::TooMany { count: 17 }));
        assert!(matches!(
            validate(&[grant("relative/path", Access::Write, GrantScope::Exact)]),
            Err(GrantError::NotAbsolute(_))
        ));
        assert!(matches!(
            validate(&[grant(
                "/home/me/project/../x",
                Access::Write,
                GrantScope::Exact
            )]),
            Err(GrantError::NotAbsolute(_))
        ));
        assert!(matches!(
            validate(&[grant(
                "/home/me/project/.git/hooks",
                Access::Write,
                GrantScope::Subtree
            )]),
            Err(GrantError::HardProtected(_))
        ));
        assert!(matches!(
            validate(&[grant(
                "/home/me/.openwork",
                Access::Write,
                GrantScope::Subtree
            )]),
            Err(GrantError::HardProtected(_))
        ));
        for broad in ["/", "/home", "/home/me"] {
            assert!(matches!(
                validate(&[grant(broad, Access::Write, GrantScope::Subtree)]),
                Err(GrantError::TooBroad(_))
            ));
        }
        assert!(matches!(
            validate(&[grant(
                "/home/me/project/src/lib.rs",
                Access::Write,
                GrantScope::Exact
            )]),
            Err(GrantError::NoNewPermission { .. })
        ));
        assert!(matches!(
            validate(&[grant(
                "/home/me/.cargo/registry",
                Access::Read,
                GrantScope::Subtree
            )]),
            Err(GrantError::NoNewPermission { .. })
        ));
        assert_eq!(
            policy(SandboxMode::AcceptEdits).validate_grants(
                &[grant(
                    "/home/me/project/target",
                    Access::Write,
                    GrantScope::Subtree
                )],
                Actor::Bash
            ),
            Ok(())
        );
    }

    #[test]
    fn grants_serialize_with_snake_case_values() {
        let encoded = serde_json::to_value(grant("/x", Access::Write, GrantScope::Subtree));
        assert_eq!(
            encoded.unwrap(),
            serde_json::json!({ "path": "/x", "access": "write", "scope": "subtree" })
        );
    }
}
