//! permissions.md §2.4 / §9.2 #10: the file-tool fence (`SandboxPolicy::check`)
//! and the Seatbelt profile reach the same verdict for every tier, mode,
//! actor and grant — checked against the real kernel.
//!
//! The whole world (home, workspace, temp root) lives under
//! `CARGO_TARGET_TMPDIR`: outside the machine's real temp roots, so "outside
//! every writable root" can be tested, and with a space in the path on this
//! repository's volume.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use openwork_sandbox::{
    Access, Actor, GrantScope, PathGrant, SANDBOX_EXEC, SandboxEnvironment, SandboxMode,
    SandboxPolicy, SeatbeltProfile,
};

/// Paths below the test world, one per tier and edge.
const PATHS: &[&str] = &[
    "home/project/src/main.rs",
    "home/project/target/debug/app",
    "home/project/.git/config",
    "home/project/.git/index.lock",
    "home/project/.git/hooks/pre-commit",
    "home/project/vendor/lib/.git/hooks/post-merge",
    "home/project/.env",
    "home/project/app/.env.local",
    "home/project/.envrc",
    "home/project/.envdir/inner",
    "home/project/.vscode/settings.json",
    "home/project/.idea/workspace.xml",
    "home/project/.gitmodules",
    "home/project/.zshrc",
    "home/project/.zshrc.bak",
    "home/.openwork/agents/a/state.json",
    "home/.agents/skills/review/SKILL.md",
    "home/.ssh/id_ed25519",
    "home/.aws/credentials",
    "home/.docker/config.json",
    "home/Library/Cookies/Cookies.binarycookies",
    "home/Library/Application Support/Google/Chrome/Default/Cookies",
    "home/.cargo/registry/cache/a.crate",
    "home/notes.txt",
    "tmp/scratch.txt",
    "outside/elsewhere.txt",
];

/// New files spelled in another case than what is on disk. macOS volumes
/// are case-insensitive, so each of these lands in a protected place.
const CASE_VARIANTS: &[&str] = &[
    "home/project/.GIT/HOOKS/new-hook",
    "home/project/.Git/new-file",
    "home/project/app/.ENVRC",
    "home/project/.VSCODE/new.json",
    "home/project/src/.ZSHRC",
];

struct World {
    root: PathBuf,
}

impl World {
    fn new(label: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&base).expect("target tmp");
        let root = std::fs::canonicalize(base)
            .expect("canonical target tmp")
            .join(format!("sandbox parity {label} ({})", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for path in PATHS {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dirs");
            std::fs::write(&path, "seed\n").expect("seed file");
        }
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Every path of the test, as given to the kernel.
    fn all_paths(&self) -> Vec<PathBuf> {
        PATHS
            .iter()
            .chain(CASE_VARIANTS)
            .map(|path| self.path(path))
            .collect()
    }

    fn policy(&self, mode: SandboxMode, grants: &[(&str, Access, GrantScope)]) -> SandboxPolicy {
        let home = self.path("home");
        SandboxPolicy::new(
            mode,
            self.path("home/project"),
            Arc::new(SandboxEnvironment::new(
                home.clone(),
                vec![self.path("tmp")],
                vec![home.join(".agents/skills")],
            )),
        )
        .with_grants(
            grants
                .iter()
                .map(|(path, access, scope)| PathGrant {
                    path: self.path(path),
                    access: *access,
                    scope: *scope,
                })
                .collect(),
        )
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// `(may_read, may_write)` per path, as the kernel sees it. Reading a path
/// that still does not exist says nothing about permissions: `None`.
fn kernel_verdicts(profile: &SeatbeltProfile, paths: &[PathBuf]) -> Vec<(Option<bool>, bool)> {
    let script = r#"for p in "$@"; do
  if printf x >> "$p" 2>/dev/null; then w=1; else w=0; fi
  if [ ! -e "$p" ]; then r=-; elif cat "$p" > /dev/null 2>&1; then r=1; else r=0; fi
  echo "$r$w"
done"#;
    let mut command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        script.to_string(),
        "parity".to_string(),
    ];
    command.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    let argv = profile.wrap(Path::new(SANDBOX_EXEC), &command);
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .expect("run sandbox-exec");
    assert!(
        output.status.success(),
        "probe script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .map(|line| {
            let read = match line.as_bytes()[0] {
                b'-' => None,
                byte => Some(byte == b'1'),
            };
            (read, line.ends_with('1'))
        })
        .collect()
}

fn policy_verdicts(policy: &SandboxPolicy, actor: Actor, paths: &[PathBuf]) -> Vec<(bool, bool)> {
    paths
        .iter()
        .map(|path| {
            let path = resolve_like_the_fence(path);
            (
                policy.check(&path, Access::Read, actor).is_ok(),
                policy.check(&path, Access::Write, actor).is_ok(),
            )
        })
        .collect()
}

/// What the file tools hand to `check`: the existing ancestor canonicalized
/// (restoring its on-disk case), the new components as given.
fn resolve_like_the_fence(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(&existing) {
            return rest
                .iter()
                .rev()
                .fold(canonical, |path, name| path.join(name));
        }
        rest.push(existing.file_name().expect("name").to_owned());
        assert!(existing.pop(), "no existing ancestor");
    }
}

type GrantSet = &'static [(&'static str, Access, GrantScope)];

const GRANT_SETS: &[GrantSet] = &[
    &[],
    &[("home/project/.git", Access::Write, GrantScope::Subtree)],
    &[("home/.ssh", Access::Read, GrantScope::Subtree)],
    &[("home/.ssh", Access::Write, GrantScope::Subtree)],
    &[("home/.cargo/registry", Access::Write, GrantScope::Subtree)],
    &[("home/project", Access::Write, GrantScope::Subtree)],
    &[
        ("home/project/.env", Access::Write, GrantScope::Exact),
        ("home/.aws/credentials", Access::Read, GrantScope::Exact),
    ],
];

#[test]
fn acc_10_file_tool_fence_and_seatbelt_agree_on_every_path() {
    let world = World::new("fence");
    let paths = world.all_paths();
    for mode in [SandboxMode::AcceptEdits, SandboxMode::Auto] {
        for grants in GRANT_SETS {
            let policy = world.policy(mode, grants);
            for actor in [Actor::Bash, Actor::FileTool] {
                let kernel = kernel_verdicts(&SeatbeltProfile::new(&policy, actor), &paths);
                let fence = policy_verdicts(&policy, actor, &paths);
                for ((path, kernel), fence) in
                    PATHS.iter().chain(CASE_VARIANTS).zip(&kernel).zip(&fence)
                {
                    let agrees = kernel.1 == fence.1 && kernel.0.is_none_or(|read| read == fence.0);
                    assert!(
                        agrees,
                        "{path}: kernel (read, write) {kernel:?} vs fence {fence:?} \
                         [{mode:?}, {actor:?}, grants {grants:?}]"
                    );
                }
            }
        }
    }
}

/// The one intended difference between the two actors (permissions.md §2.2).
#[test]
fn bash_and_file_tools_differ_only_on_the_workspace_under_accept_edits() {
    let world = World::new("actors");
    let paths = world.all_paths();
    for mode in [SandboxMode::AcceptEdits, SandboxMode::Auto] {
        for grants in GRANT_SETS {
            let policy = world.policy(mode, grants);
            let bash = policy_verdicts(&policy, Actor::Bash, &paths);
            let files = policy_verdicts(&policy, Actor::FileTool, &paths);
            for ((path, bash), files) in PATHS.iter().chain(CASE_VARIANTS).zip(&bash).zip(&files) {
                let covered_by_write_grant = grants.iter().any(|(grant, access, _)| {
                    *access == Access::Write && world.path(path).starts_with(world.path(grant))
                });
                let expected_difference = mode == SandboxMode::AcceptEdits
                    && path.starts_with("home/project/")
                    && policy.tier(&resolve_like_the_fence(&world.path(path)))
                        == openwork_sandbox::PathTier::Normal
                    && !covered_by_write_grant;
                if expected_difference {
                    assert_eq!((*bash, *files), ((true, false), (true, true)), "{path}");
                } else {
                    assert_eq!(bash, files, "{path} [{mode:?}, grants {grants:?}]");
                }
            }
        }
    }
}

/// The case variants are protected on both sides, not merely consistent.
#[test]
fn case_variants_of_protected_names_are_not_writable() {
    let world = World::new("case");
    let policy = world.policy(SandboxMode::Auto, &[]);
    let paths = CASE_VARIANTS
        .iter()
        .map(|path| world.path(path))
        .collect::<Vec<_>>();
    let kernel = kernel_verdicts(&SeatbeltProfile::new(&policy, Actor::Bash), &paths);
    let fence = policy_verdicts(&policy, Actor::FileTool, &paths);
    for ((path, kernel), fence) in CASE_VARIANTS.iter().zip(&kernel).zip(&fence) {
        assert!(!kernel.1, "{path}: kernel allowed the write");
        assert!(!fence.1, "{path}: fence allowed the write");
    }
}
