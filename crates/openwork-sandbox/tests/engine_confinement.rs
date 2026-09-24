//! 协作 Engine 围栏的真机测试（collaboration.md §3.1、§16 #17）：在真实 Seatbelt 下运行
//! `/bin/sh`，确认 Engine 进程只能写本 Agent 的目录，读不到 `$HOME` 内的其他内容。
//!
//! `$HOME` 是建在 `CARGO_TARGET_TMPDIR` 下的假目录，不在系统临时根内，因此不会碰到真实主目录。

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use openwork_sandbox::{EngineConfinement, SANDBOX_EXEC, SandboxEnvironment, Seatbelt};

struct Host {
    root: PathBuf,
}

impl Host {
    fn new(label: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&base).expect("target tmp");
        let root = std::fs::canonicalize(base)
            .expect("canonical")
            .join(format!("engine-confinement-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).expect("home");
        std::fs::create_dir_all(root.join("outside")).expect("outside");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn write(&self, path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, content).expect("write");
    }

    /// 只允许写 `agent`，并额外放行 `readable` 的读取。
    fn confinement(&self, agent: &Path, readable: &[&Path]) -> EngineConfinement {
        let environment = SandboxEnvironment::new(self.home(), Vec::new(), Vec::new());
        readable.iter().fold(
            EngineConfinement::new(&environment).with_writable_root(agent),
            |confinement, path| confinement.with_readable_path(path),
        )
    }

    fn run(&self, confinement: &EngineConfinement, script: &str) -> std::process::Output {
        let argv = Seatbelt::probe(SANDBOX_EXEC)
            .confine(
                confinement,
                &["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
            )
            .expect("sandbox available on macOS");
        Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .expect("spawn sandbox-exec")
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

#[test]
fn engine_reads_and_writes_its_own_agent_directory() {
    let host = Host::new("own");
    let agent = host.home().join(".openwork/agents/alpha");
    std::fs::create_dir_all(&agent).expect("agent");
    let confinement = host.confinement(&agent, &[]);

    let output = host.run(
        &confinement,
        &format!(
            "cd {agent} && mkdir -p work && printf note > work/a.txt && cat work/a.txt && ls work",
            agent = quoted(&agent)
        ),
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "notea.txt\n");
    assert_eq!(
        std::fs::read_to_string(agent.join("work/a.txt")).unwrap(),
        "note"
    );
}

#[test]
fn engine_cannot_read_or_list_another_agent_or_other_home_files() {
    let host = Host::new("isolation");
    let alpha = host.home().join(".openwork/agents/alpha");
    let beta = host.home().join(".openwork/agents/beta");
    std::fs::create_dir_all(&alpha).expect("alpha");
    host.write(&beta.join("secret.txt"), "beta secret");
    host.write(&host.home().join("Documents/plan.md"), "user plan");
    host.write(
        &host.home().join(".local/share/opencode/auth.json"),
        "{\"k\":1}",
    );
    let confinement = host.confinement(&alpha, &[]);

    for target in [
        beta.join("secret.txt"),
        host.home().join("Documents/plan.md"),
        host.home().join(".local/share/opencode/auth.json"),
    ] {
        let output = host.run(&confinement, &format!("cat {}", quoted(&target)));
        assert!(!output.status.success(), "read {}", target.display());
        assert!(output.stdout.is_empty(), "leaked {}", target.display());
    }
    let listing = host.run(&confinement, &format!("ls {}", quoted(&beta)));
    assert!(!listing.status.success());
    assert!(listing.stdout.is_empty());
}

#[test]
fn engine_cannot_write_outside_its_writable_roots() {
    let host = Host::new("writes");
    let alpha = host.home().join(".openwork/agents/alpha");
    let beta = host.home().join(".openwork/agents/beta");
    std::fs::create_dir_all(&alpha).expect("alpha");
    std::fs::create_dir_all(&beta).expect("beta");
    let confinement = host.confinement(&alpha, &[]);

    for target in [
        beta.join("planted.txt"),
        host.home().join("planted.txt"),
        host.root.join("outside/planted.txt"),
        alpha.join("../beta/escaped.txt"),
    ] {
        let output = host.run(&confinement, &format!("printf x > {}", quoted(&target)));
        assert!(!output.status.success(), "wrote {}", target.display());
    }
    assert!(!beta.join("planted.txt").exists());
    assert!(!beta.join("escaped.txt").exists());
    assert!(!host.home().join("planted.txt").exists());
    assert!(!host.root.join("outside/planted.txt").exists());
}

#[test]
fn engine_reads_explicit_home_exceptions_and_everything_outside_home() {
    let host = Host::new("exceptions");
    let alpha = host.home().join(".openwork/agents/alpha");
    let tokens = host.home().join(".openwork/runtime/session/agents");
    std::fs::create_dir_all(&alpha).expect("alpha");
    host.write(&tokens.join("alpha/runtime-token"), "alpha-token");
    host.write(&tokens.join("beta/runtime-token"), "beta-token");
    host.write(&host.root.join("outside/tool.txt"), "tool");
    let own_token = tokens.join("alpha/runtime-token");
    let confinement = host.confinement(&alpha, &[&own_token]);

    let own = host.run(&confinement, &format!("cat {}", quoted(&own_token)));
    assert_eq!(String::from_utf8_lossy(&own.stdout), "alpha-token");
    let other = host.run(
        &confinement,
        &format!("cat {}", quoted(&tokens.join("beta/runtime-token"))),
    );
    assert!(!other.status.success());
    assert!(other.stdout.is_empty());
    let outside = host.run(
        &confinement,
        &format!("cat {}", quoted(&host.root.join("outside/tool.txt"))),
    );
    assert_eq!(String::from_utf8_lossy(&outside.stdout), "tool");
    // 只拒绝读内容，不拒绝 stat：解析 Agent 目录的上级路径必须成功。
    let metadata = host.run(
        &confinement,
        &format!(
            "test -d {} && test -e {}",
            quoted(&host.home()),
            quoted(&tokens)
        ),
    );
    assert!(metadata.status.success());
}

#[test]
fn quotes_parentheses_and_spaces_in_paths_do_not_change_the_profile() {
    let host = Host::new("metachar");
    let alpha = host
        .home()
        .join(".openwork/agents/a \"b\" (allow default) ü");
    std::fs::create_dir_all(&alpha).expect("alpha");
    host.write(&host.home().join("Documents/plan.md"), "user plan");
    let confinement = host.confinement(&alpha, &[]);

    let own = host.run(
        &confinement,
        &format!(
            "printf ok > {} && cat {}",
            quoted(&alpha.join("f.txt")),
            quoted(&alpha.join("f.txt"))
        ),
    );
    assert_eq!(String::from_utf8_lossy(&own.stdout), "ok");
    let other = host.run(
        &confinement,
        &format!("cat {}", quoted(&host.home().join("Documents/plan.md"))),
    );
    assert!(!other.status.success());
}

/// testing.md §6：APFS 默认大小写不敏感，换一种大小写写 `$HOME` 内的路径不能绕过禁读。
/// 只在当前卷确实大小写不敏感（变体在沙箱外能打开）时才构成绕过，否则变体本来就不存在。
#[test]
fn case_variants_of_home_paths_are_still_unreadable() {
    let host = Host::new("case");
    let alpha = host.home().join(".openwork/agents/alpha");
    std::fs::create_dir_all(&alpha).expect("alpha");
    host.write(&host.home().join("Documents/plan.md"), "user plan");
    let variant = host.home().join("DOCUMENTS/PLAN.MD");
    let confinement = host.confinement(&alpha, &[]);

    let output = host.run(&confinement, &format!("cat {}", quoted(&variant)));

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    if std::fs::read_to_string(&variant).is_ok() {
        // 卷大小写不敏感：上面的拒绝确实是沙箱给出的。
        let control = host.run(
            &confinement,
            &format!(
                "printf x > {} && cat {}",
                quoted(&alpha.join("F.TXT")),
                quoted(&alpha.join("f.txt"))
            ),
        );
        assert_eq!(String::from_utf8_lossy(&control.stdout), "x");
    }
}

/// testing.md §6：Engine 在自己能写的目录里放符号链接，也不能借此读 `$HOME` 或写其他 Agent。
#[test]
fn symlinks_planted_in_the_agent_directory_do_not_escape() {
    let host = Host::new("symlink");
    let alpha = host.home().join(".openwork/agents/alpha");
    let beta = host.home().join(".openwork/agents/beta");
    std::fs::create_dir_all(&alpha).expect("alpha");
    std::fs::create_dir_all(&beta).expect("beta");
    host.write(&host.home().join("Documents/plan.md"), "user plan");
    let confinement = host.confinement(&alpha, &[]);

    let output = host.run(
        &confinement,
        &format!(
            "cd {alpha} && ln -s {plan} plan-link && ln -s {home} home-link && ln -s {beta} beta-link \
             && ! cat plan-link && ! cat home-link/Documents/plan.md && ! printf x > beta-link/planted.txt",
            alpha = quoted(&alpha),
            plan = quoted(&host.home().join("Documents/plan.md")),
            home = quoted(&host.home()),
            beta = quoted(&beta),
        ),
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("user plan"));
    assert!(!beta.join("planted.txt").exists());
    assert!(alpha.join("plan-link").is_symlink());
}
