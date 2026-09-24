//! 用真实的 `sandbox-exec` 验证 permissions.md §3.2–§3.3。

#![cfg(target_os = "macos")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use openwork_sandbox::{
    RunOutcome, SANDBOX_EXEC, SandboxBackend, SandboxEnvironment, SandboxMode, SandboxPolicy,
    SandboxStatus, Seatbelt, classify, probe,
};

fn scratch(label: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&base).expect("target tmp");
    let directory = std::fs::canonicalize(base)
        .expect("canonical")
        .join(format!("sandbox-probe-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("scratch");
    directory
}

/// §9.2 #12
#[test]
fn acc_12_the_real_sandbox_passes_its_self_check() {
    assert_eq!(probe(Path::new(SANDBOX_EXEC)), SandboxStatus::Available);
}

/// 同一进程里并发的自检互不干扰：它们曾在同一微秒内拿到同一个临时目录，
/// 先结束的一次删掉目录，另一次误报沙箱不可用。
#[test]
fn concurrent_self_checks_do_not_share_a_directory() {
    const CHECKS: usize = 16;
    let barrier = Arc::new(std::sync::Barrier::new(CHECKS));
    let checks = (0..CHECKS)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                probe(Path::new(SANDBOX_EXEC))
            })
        })
        .collect::<Vec<_>>();
    for check in checks {
        assert_eq!(
            check.join().expect("probe thread"),
            SandboxStatus::Available
        );
    }
}

#[test]
fn a_missing_sandbox_exec_is_unavailable_with_a_reason() {
    let status = probe(Path::new("/nonexistent/sandbox-exec"));
    let SandboxStatus::Unavailable { reason } = status else {
        panic!("a missing binary cannot be available");
    };
    assert!(reason.contains("/nonexistent/sandbox-exec"), "{reason}");
}

/// 不加约束直接运行命令的替身："命令跑起来了"不能被当成"沙箱在工作"。
#[test]
fn a_sandbox_that_does_not_deny_fails_the_self_check() {
    let directory = scratch("fake");
    let fake = directory.join("sandbox-exec");
    std::fs::write(&fake, "#!/bin/sh\nshift 3\nexec \"$@\"\n").expect("fake");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let status = probe(&fake);

    let SandboxStatus::Unavailable { reason } = status else {
        panic!("an unconfined runner must fail the self-check");
    };
    assert!(reason.contains("succeeded"), "{reason}");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn an_unavailable_backend_refuses_to_wrap() {
    let backend = Seatbelt::probe("/nonexistent/sandbox-exec");
    let policy = SandboxPolicy::new(
        SandboxMode::Auto,
        PathBuf::from("/private/tmp"),
        Arc::new(SandboxEnvironment::detect([]).expect("environment")),
    );
    assert!(backend.wrap(&policy, &["true".to_string()]).is_err());
}

fn run(argv: &[String], directory: &Path) -> (i32, String) {
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(directory)
        .output()
        .expect("run");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(-1), text)
}

/// §9.2 #15、§3.3：内核拒绝是 `Denied`；`sandbox-exec` 没能启动命令则不是。
#[test]
fn acc_15_denials_and_sandbox_failures_are_told_apart() {
    let workspace = scratch("denial");
    let backend = Seatbelt::probe(SANDBOX_EXEC);
    let policy = SandboxPolicy::new(
        SandboxMode::Auto,
        workspace.clone(),
        Arc::new(SandboxEnvironment::detect([]).expect("environment")),
    );
    let wrap = |command: &[&str]| {
        backend
            .wrap(
                &policy,
                &command
                    .iter()
                    .map(|part| part.to_string())
                    .collect::<Vec<_>>(),
            )
            .expect("available")
    };

    std::fs::create_dir(workspace.join(".git")).expect("git dir");
    let (code, output) = run(
        &wrap(&["/bin/bash", "-c", "touch .git/index.lock"]),
        &workspace,
    );
    assert_eq!(classify(code, &output), RunOutcome::Denied, "{output}");

    let (code, output) = run(
        &wrap(&["/bin/bash", "-c", "echo fine > ok.txt"]),
        &workspace,
    );
    assert_eq!(classify(code, &output), RunOutcome::Completed, "{output}");
    assert!(workspace.join("ok.txt").exists());

    let (code, output) = run(&wrap(&["/nonexistent/bash", "-c", "true"]), &workspace);
    assert!(
        matches!(classify(code, &output), RunOutcome::SandboxFailed { .. }),
        "{code}: {output}"
    );
    let _ = std::fs::remove_dir_all(workspace);
}
