//! Startup self-check (permissions.md §3.2): the sandbox is available only
//! once a write it forbids has actually failed with `EPERM`.
//!
//! "The command ran" does not prove "the sandbox applied"; a broken setup
//! still reports success. The probe therefore demands the refusal itself.
//! It runs `sandbox-exec` directly because it is a one-off check at startup,
//! not a tool call; tool processes are the `ProcessBackend`'s.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::denial::{RunOutcome, classify};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SandboxStatus {
    Available,
    /// `reason` is shown to the user as is.
    Unavailable {
        reason: String,
    },
}

impl SandboxStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

const PROBE_PROFILE: &str = "(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write* (literal \"/dev/null\"))\n";

/// Runs the self-check with the `sandbox-exec` at `sandbox_exec`.
pub fn probe(sandbox_exec: &Path) -> SandboxStatus {
    if !cfg!(target_os = "macos") {
        return unavailable("the sandbox is only implemented for macOS");
    }
    let directory = probe_directory();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return unavailable(format!(
            "could not create the probe directory {}: {error}",
            directory.display()
        ));
    }
    let status = check(sandbox_exec, &directory);
    let _ = std::fs::remove_dir_all(&directory); // 清理失败只留下一个空的临时目录，不影响结论
    status
}

fn check(sandbox_exec: &Path, directory: &Path) -> SandboxStatus {
    let directory = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let target = directory.join("probe");
    let run = |script: &str| {
        Command::new(sandbox_exec)
            .args(["-p", PROBE_PROFILE, "--", "/bin/sh", "-c", script, "probe"])
            .arg(&target)
            .output()
    };

    let control = match run("exit 0") {
        Ok(output) => output,
        Err(error) => {
            return unavailable(format!(
                "could not start {}: {error}",
                sandbox_exec.display()
            ));
        }
    };
    if !control.status.success() {
        return unavailable(format!(
            "{} could not run a command: {}",
            sandbox_exec.display(),
            first_line(&control.stderr)
        ));
    }

    let denied = match run("printf probe > \"$1\"") {
        Ok(output) => output,
        Err(error) => {
            return unavailable(format!(
                "could not start {}: {error}",
                sandbox_exec.display()
            ));
        }
    };
    let stderr = String::from_utf8_lossy(&denied.stderr);
    let exit_code = denied.status.code().unwrap_or(-1);
    if target.exists() {
        return unavailable(format!(
            "a write the sandbox forbids succeeded ({}); refusing to run commands",
            target.display()
        ));
    }
    match classify(exit_code, &stderr) {
        RunOutcome::Denied => SandboxStatus::Available,
        RunOutcome::SandboxFailed { message } => unavailable(message),
        RunOutcome::Completed => unavailable(format!(
            "the forbidden write failed without the expected EPERM (exit {exit_code}): {}",
            first_line(&denied.stderr)
        )),
    }
}

/// 每次自检用自己的目录。时间戳区分不同进程（进程号会被复用，崩溃的进程可能留下旧目录）；
/// 计数器区分同一进程里的并发自检：macOS 的时钟精度是微秒，只靠时间戳时两次自检会拿到
/// 同一个目录，先结束的一次删掉它，另一次就误报沙箱不可用。
fn probe_directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    std::env::temp_dir().join(format!(
        "openwork-sandbox-probe-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn unavailable(reason: impl Into<String>) -> SandboxStatus {
    SandboxStatus::Unavailable {
        reason: reason.into(),
    }
}
