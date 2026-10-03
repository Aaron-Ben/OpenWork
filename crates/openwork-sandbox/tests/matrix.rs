//! 真机矩阵：在真实的 `$HOME` 与真实的 Seatbelt 下
//! 运行日常命令。
//!
//! 工作区建在 `CARGO_TARGET_TMPDIR` 下，不在系统临时根内。对受保护位置的每次探测都用唯一的
//! 名字，事后删除，回归时也不会留下文件。

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use openwork_sandbox::{
    Access, GrantScope, PathGrant, SANDBOX_EXEC, SandboxBackend, SandboxEnvironment, SandboxMode,
    SandboxPolicy, Seatbelt,
};

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(label: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&base).expect("target tmp");
        let root = std::fs::canonicalize(base)
            .expect("canonical")
            .join(format!("sandbox-matrix-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("workspace");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, content).expect("write");
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative)).expect("read")
    }

    /// 不经沙箱运行 `script`，只用于准备测试环境。
    fn setup(&self, script: &str) {
        let output = Command::new("/bin/bash")
            .args(["-c", script])
            .current_dir(&self.root)
            .output()
            .expect("setup");
        assert!(
            output.status.success(),
            "setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_repository(&self) {
        self.write("README.md", "hello\n");
        self.write(
            "Cargo.toml",
            "[package]\nname = \"matrix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        );
        self.write("src/main.rs", "fn main() {}\n\n#[test]\nfn works() {}\n");
        self.write(".gitignore", "target/\n");
        self.setup(
            "git init -q && git -c user.name=t -c user.email=t@example.com add -A && \
             git -c user.name=t -c user.email=t@example.com commit -qm init",
        );
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Sandbox {
    backend: Seatbelt,
    environment: Arc<SandboxEnvironment>,
}

impl Sandbox {
    fn new() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
        Self {
            backend: Seatbelt::probe(SANDBOX_EXEC),
            environment: Arc::new(
                SandboxEnvironment::detect([home.join(".agents/skills")]).expect("environment"),
            ),
        }
    }

    fn policy(&self, workspace: &Workspace, mode: SandboxMode) -> SandboxPolicy {
        SandboxPolicy::new(mode, workspace.root.clone(), Arc::clone(&self.environment))
    }

    /// `script` 在 `policy` 下运行的 `(退出码, stdout + stderr)`。
    fn run(&self, policy: &SandboxPolicy, workspace: &Workspace, script: &str) -> (i32, String) {
        let argv = self
            .backend
            .wrap(
                policy,
                &[
                    "/bin/bash".to_string(),
                    "-c".to_string(),
                    script.to_string(),
                ],
            )
            .expect("sandbox available");
        // 与上线后启动 bash 的方式一致：带上 `bash_environment()`。
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&workspace.root)
            .env_remove("CARGO_TARGET_DIR")
            .envs(policy.environment().bash_environment())
            .output()
            .expect("run");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code().unwrap_or(-1), text)
    }

    fn succeeds(&self, policy: &SandboxPolicy, workspace: &Workspace, script: &str) {
        let (code, output) = self.run(policy, workspace, script);
        assert_eq!(code, 0, "`{script}` must succeed:\n{output}");
    }

    fn denied(&self, policy: &SandboxPolicy, workspace: &Workspace, script: &str) {
        let (code, output) = self.run(policy, workspace, script);
        assert_ne!(code, 0, "`{script}` must be denied:\n{output}");
        assert!(
            output.contains("Operation not permitted"),
            "`{script}` must fail with a Seatbelt denial:\n{output}"
        );
    }
}

fn write_grant(path: PathBuf, scope: GrantScope) -> PathGrant {
    PathGrant {
        path,
        access: Access::Write,
        scope,
    }
}

fn unique(label: &str) -> String {
    format!("openwork-matrix-{label}-{}", std::process::id())
}

#[test]
fn auto_builds_and_reads_but_git_writes_need_an_escalation() {
    let sandbox = Sandbox::new();
    let workspace = Workspace::new("auto-git");
    workspace.git_repository();
    let auto = sandbox.policy(&workspace, SandboxMode::Auto);

    sandbox.succeeds(
        &auto,
        &workspace,
        "cargo build --offline -q && cargo test --offline -q",
    );
    assert!(workspace.path("target").exists());
    sandbox.succeeds(
        &auto,
        &workspace,
        "git status && git log --oneline && git diff",
    );

    workspace.write("README.md", "changed\n");
    for script in [
        "git add -A",
        "git -c user.name=t -c user.email=t@example.com commit -qam change",
        "git reset --hard",
        "git stash",
    ] {
        sandbox.denied(&auto, &workspace, script);
        assert_eq!(
            workspace.read("README.md"),
            "changed\n",
            "`{script}` changed the workspace"
        );
    }

    let with_git = auto.clone().with_grants(vec![write_grant(
        workspace.path(".git"),
        GrantScope::Subtree,
    )]);
    sandbox.succeeds(
        &with_git,
        &workspace,
        "git add -A && git -c user.name=t -c user.email=t@example.com commit -qm change",
    );
    sandbox.denied(&with_git, &workspace, "touch .git/hooks/pre-commit");

    workspace.write("scratch/untracked.txt", "x\n");
    sandbox.succeeds(&auto, &workspace, "git clean -fdq");
    assert!(!workspace.path("scratch").exists());
}

#[test]
fn protected_locations_stay_closed_and_temp_stays_open() {
    let sandbox = Sandbox::new();
    let workspace = Workspace::new("protected");
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let name = unique("probe");
    let protected = [
        home.join(&name),
        home.join(".openwork").join(&name),
        home.join(".agents/skills").join(&name),
    ];
    for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
        let policy = sandbox.policy(&workspace, mode);
        for path in &protected {
            let script = format!("printf x > {}", shell_quote(path));
            let (code, output) = sandbox.run(&policy, &workspace, &script);
            let _ = std::fs::remove_file(path);
            assert!(
                code != 0 && output.contains("Operation not permitted"),
                "writing {} must be denied in {mode:?}: {output}",
                path.display()
            );
        }
        for credential in [".ssh", ".aws", "Library/Cookies"] {
            let path = home.join(credential);
            if !path.exists() {
                continue;
            }
            sandbox.denied(&policy, &workspace, &format!("ls {}", shell_quote(&path)));
        }
        for root in sandbox.environment.temp_roots() {
            let path = root.join(&name);
            sandbox.succeeds(
                &policy,
                &workspace,
                &format!("printf x > {}", shell_quote(&path)),
            );
            let _ = std::fs::remove_file(path);
        }
    }
}

#[test]
fn accept_edits_denies_bash_workspace_writes_until_escalated() {
    let sandbox = Sandbox::new();
    let workspace = Workspace::new("accept-edits");
    workspace.git_repository();
    let accept = sandbox.policy(&workspace, SandboxMode::AcceptEdits);

    sandbox.denied(&accept, &workspace, "cargo build --offline -q");
    sandbox.denied(&accept, &workspace, "touch src/x.rs");
    sandbox.succeeds(&accept, &workspace, "cat src/main.rs && git status");

    // Cargo 经同级的临时目录创建 `target` 并写 Cargo.lock，所以构建需要写工作区本身；
    // 整个工作区的授权仍不打开 `.git` 与 `.env`（tests/parity.rs）。
    let build = accept.clone().with_grants(vec![write_grant(
        workspace.root.clone(),
        GrantScope::Subtree,
    )]);
    sandbox.succeeds(&build, &workspace, "cargo build --offline -q");
    sandbox.denied(&build, &workspace, "touch .git/index.lock");
    let touch = accept.clone().with_grants(vec![write_grant(
        workspace.path("src/x.rs"),
        GrantScope::Exact,
    )]);
    sandbox.succeeds(&touch, &workspace, "touch src/x.rs");
}

/// permissions.md §5：按路径写标准流与文件描述符的常见脚本写法，在两个模式下都能用。
#[test]
fn stream_devices_are_writable_in_every_mode() {
    let sandbox = Sandbox::new();
    let workspace = Workspace::new("devices");
    for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
        let policy = sandbox.policy(&workspace, mode);
        for script in [
            "echo hi > /dev/null",
            "echo hi > /dev/stdout",
            "echo hi | tee /dev/stderr",
            "exec 3>&1; echo hi > /dev/fd/3",
            "echo hi | tee >(cat) > /dev/null",
        ] {
            sandbox.succeeds(&policy, &workspace, script);
        }
        sandbox.denied(&policy, &workspace, "touch /dev/openwork-matrix-probe");
    }
}

/// permissions.md §15 #49：主目录作工作区时，bash 在两个模式下都不能写它。
#[test]
fn a_home_workspace_is_read_only_for_bash() {
    let sandbox = Sandbox::new();
    let cwd = Workspace::new("home-workspace");
    let home = sandbox.environment.home().to_path_buf();
    let name = unique("home-workspace");
    let mut targets = vec![home.join(&name)];
    let launch_agents = home.join("Library/LaunchAgents");
    if launch_agents.is_dir() {
        targets.push(launch_agents.join(format!("{name}.plist")));
    }
    for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
        let policy = SandboxPolicy::new(mode, home.clone(), Arc::clone(&sandbox.environment));
        for target in &targets {
            let (code, output) = sandbox.run(
                &policy,
                &cwd,
                &format!("printf x > {}", shell_quote(target)),
            );
            let _ = std::fs::remove_file(target);
            assert!(
                code != 0 && output.contains("Operation not permitted"),
                "writing {} must be denied in {mode:?}: {output}",
                target.display()
            );
        }
        sandbox.succeeds(&policy, &cwd, "ls ~ > /dev/null");
    }
}

/// permissions.md §15 #50：Go 的编译缓存在 OpenWork 私有的临时目录里，`go build` / `go test` 不需要越界。
#[test]
fn go_builds_and_tests_with_the_private_cache() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("go is not installed; skipping");
        return;
    }
    let sandbox = Sandbox::new();
    let workspace = Workspace::new("go");
    workspace.write("go.mod", "module probe\n\ngo 1.21\n");
    workspace.write("main.go", "package main\n\nfunc main() {}\n");
    workspace.write(
        "main_test.go",
        "package main\n\nimport \"testing\"\n\nfunc TestProbe(t *testing.T) {}\n",
    );
    let policy = sandbox.policy(&workspace, SandboxMode::Auto);
    sandbox.succeeds(&policy, &workspace, "go build ./... && go test ./...");
}

/// `(标签, 要创建的文件, 脚本)`。
type Probe = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static str,
);

/// 其余常用工具链可能把缓存写到工作区外。结果只记录、不断言，留给以后的决策；
/// Go 已由 `go_builds_and_tests_with_the_private_cache` 断言。
#[test]
fn toolchain_probes_are_recorded() {
    let sandbox = Sandbox::new();
    let mut rows = Vec::new();
    let probes: &[Probe] = &[
        (
            "npm install (no dependencies)",
            &[(
                "package.json",
                "{\"name\":\"probe\",\"version\":\"1.0.0\"}\n",
            )],
            "npm install --offline --no-audit --no-fund",
        ),
        (
            "pnpm install (no dependencies)",
            &[(
                "package.json",
                "{\"name\":\"probe\",\"version\":\"1.0.0\"}\n",
            )],
            "pnpm install --offline",
        ),
        (
            "python3 -m pytest",
            &[("test_probe.py", "def test_probe():\n    pass\n")],
            "python3 -m pytest -q",
        ),
    ];
    for (label, files, script) in probes {
        let workspace = Workspace::new(&label.replace(' ', "-"));
        for (path, content) in *files {
            workspace.write(path, content);
        }
        let policy = sandbox.policy(&workspace, SandboxMode::Auto);
        let (code, output) = sandbox.run(&policy, &workspace, script);
        let denial = output
            .lines()
            .find(|line| {
                line.to_ascii_lowercase()
                    .contains("operation not permitted")
            })
            .unwrap_or("")
            .to_string();
        let last = output.lines().last().unwrap_or("").to_string();
        rows.push(format!(
            "{label:<32} exit {code:<4} denial: {denial}\n{:<32}          last: {last}",
            ""
        ));
    }
    println!("\nTOOLCHAIN PROBES (auto mode)\n{}", rows.join("\n"));
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}
