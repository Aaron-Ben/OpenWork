//! 一次调用能读写什么（permissions.md §2.2–§2.4、§4.2）。

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::tiers;

/// 工作区对一次调用开放到什么程度。按从窄到宽排序，所以子 Agent 的生效模式是
/// `parent.min(role_ceiling)`。用户与角色从同样的两个取值里选（permissions.md §2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// `write` / `edit` 能改工作区，bash 不能。
    AcceptEdits,
    /// 两者都能改工作区。默认模式。
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

/// 发起访问的一方。两个模式只在 bash 能否写工作区上不同，所以规则需要知道是谁。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    Bash,
    FileTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    /// 蕴含读。
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Exact,
    Subtree,
}

/// 单次调用额外获得读或写的一个路径（permissions.md §4.1）。授权只存在于那次调用的策略里。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathGrant {
    /// 规范化后的绝对路径。
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

/// 路径所属的档（permissions.md §2.3），供卡片与错误文本使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathTier {
    HardProtected,
    Sensitive,
    Credential,
    Normal,
}

/// 访问被拒绝的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Denial {
    /// 任何模式、任何授权下都不可写。
    HardProtected,
    /// 只读，直到授权点名它。
    Sensitive,
    /// 不可读（也不可写），直到授权点名它。
    Credential,
    /// 不在这个模式与访问方的可写根之内。
    OutsideWritableRoots,
}

/// 所有策略共享的主机事实：主目录、临时目录与受保护的 OpenWork 目录在哪里。路径都已规范化：
/// Seatbelt 按真实路径匹配，`/tmp` 实际是 `/private/tmp`。
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

    /// 读取 `$HOME` 与 `$TMPDIR`，并把所有路径规范化。
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

    /// `~/.openwork` 与各 skill 根。
    pub(crate) fn hard_protected_roots(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.openwork_home.as_path())
            .chain(self.skill_roots.iter().map(PathBuf::as_path))
    }
}

/// Go 编译缓存的位置变量；默认值 `~/Library/Caches/go-build` 在沙箱外。
const GO_CACHE_VARIABLE: &str = "GOCACHE";

/// 临时根下 OpenWork 私有缓存的目录名。
const PRIVATE_CACHE_DIRECTORY: &str = "openwork";

pub(crate) fn canonical_or_lexical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 一次越界最多可列的路径数（permissions.md §4.2）。
pub const MAX_GRANTS: usize = 16;

/// 一次调用的策略：会话模式、工作区，以及用户只为这一次批准的授权。bash 的 Seatbelt profile
/// 与文件工具的围栏都由它推导（permissions.md §2.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    /// 已规范化。
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

    /// `actor` 能否对规范化后的 `path` 执行 `access`。
    pub fn check(&self, path: &Path, access: Access, actor: Actor) -> Result<(), Denial> {
        match access {
            Access::Read => self.check_read(path),
            Access::Write => self.check_write(path, actor),
        }
    }

    /// bash 能否不经越界写工作区：只有 `auto`，且工作区不包含主目录（permissions.md §2.2）。
    pub fn bash_writes_workspace(&self) -> bool {
        self.mode == SandboxMode::Auto && !self.workspace_contains_home()
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
        // 读、写两种授权都解开它覆盖范围的读取；Seatbelt 也把每条授权从凭据的
        // `deny file-read*` 中扣除。
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

    /// 授权只有自己点名敏感或凭据路径时才解开它：授权整个工作区不会打开 `.git`。
    pub(crate) fn grant_unlocks(
        &self,
        grant: &PathGrant,
        sensitive: bool,
        credential: bool,
    ) -> bool {
        (!sensitive || self.is_sensitive(&grant.path))
            && (!credential || self.is_credential(&grant.path))
    }

    /// 临时根；这个模式允许 `actor` 写工作区时再加上工作区。
    pub(crate) fn base_writable_roots(&self, actor: Actor) -> Vec<&Path> {
        let workspace_writable = match actor {
            Actor::FileTool => true,
            Actor::Bash => self.bash_writes_workspace(),
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

    /// 按当前策略校验越界请求里的路径（permissions.md §4.2）。理由与审批归 Core 管。
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

    /// `/`、`$HOME` 及其祖先：这么宽的子树等于 OpenWork 不提供的"哪里都能写"。
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

/// 越界请求不经询问用户就被拒绝的原因。
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
                "sandboxPermissions.paths is empty; list the paths the command needs"
            ),
            Self::TooMany { count } => write!(
                formatter,
                "sandboxPermissions.paths has {count} entries; at most {MAX_GRANTS} are allowed"
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
                "{} is already {} under the current policy; remove it from sandboxPermissions",
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
