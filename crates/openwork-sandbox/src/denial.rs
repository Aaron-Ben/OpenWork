//! 区分"内核拒绝了命令里的操作"与"沙箱根本没有启动"（permissions.md §3.3）。

/// 一次结束的 `sandbox-exec` 运行对工具结果意味着什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// 命令执行了，退出码就是它自己的。
    Completed,
    /// 命令执行了，并碰到了 Seatbelt 的拒绝。这是推断：其他来源的 `EPERM` 也会被标记，
    /// 最坏情况只是多给一次越界提示。
    Denied,
    /// `sandbox-exec` 在运行命令之前就失败了。什么都没执行：这是沙箱不可用，不是命令被拒。
    SandboxFailed { message: String },
}

/// `sandbox-exec` 自己的退出码：用法错误（64）、profile 错误（65）、exec 命令失败（71）。
/// 命令本身也可能以这些码退出，所以只有输出开头同时出现 `sandbox-exec` 自己的消息时才采信——
/// 它在命令启动之前打印。
const SANDBOX_EXEC_FAILURES: &[i32] = &[64, 65, 71];

pub fn classify(exit_code: i32, output: &str) -> RunOutcome {
    let start = output.trim_start();
    if SANDBOX_EXEC_FAILURES.contains(&exit_code)
        && (start.starts_with("sandbox-exec:") || start.starts_with("Usage: sandbox-exec"))
    {
        return RunOutcome::SandboxFailed {
            message: start.lines().next().unwrap_or_default().to_string(),
        };
    }
    if exit_code != 0 && mentions_eperm(output) {
        return RunOutcome::Denied;
    }
    RunOutcome::Completed
}

/// 各运行时对 `EPERM` 的写法：C 与 Rust 写 "Operation not permitted"，Go 与 Node 写
/// "operation not permitted"。
fn mentions_eperm(output: &str) -> bool {
    const EPERM: &str = "operation not permitted";
    output
        .as_bytes()
        .windows(EPERM.len())
        .any(|window| window.eq_ignore_ascii_case(EPERM.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_needs_a_failure_and_the_seatbelt_message() {
        assert_eq!(
            classify(1, "touch: .git/index.lock: Operation not permitted\n"),
            RunOutcome::Denied
        );
        assert_eq!(
            classify(0, "cp: x: Operation not permitted (ignored)\n"),
            RunOutcome::Completed
        );
        assert_eq!(classify(2, "error: no such file\n"), RunOutcome::Completed);
        assert_eq!(
            classify(
                1,
                "go: open /Users/me/Library/Caches/go-build/trim.txt: operation not permitted\n"
            ),
            RunOutcome::Denied,
            "Go and Node spell EPERM in lowercase"
        );
    }

    #[test]
    fn sandbox_exec_failures_are_not_denials() {
        assert_eq!(
            classify(
                65,
                "sandbox-exec: unbound variable: bogus at <input string>, line 1\n"
            ),
            RunOutcome::SandboxFailed {
                message: "sandbox-exec: unbound variable: bogus at <input string>, line 1"
                    .to_string()
            }
        );
        assert!(matches!(
            classify(
                71,
                "sandbox-exec: execvp() of 'bash' failed: No such file or directory\n"
            ),
            RunOutcome::SandboxFailed { .. }
        ));
        assert_eq!(
            classify(64, "usage: mytool [-v]\n"),
            RunOutcome::Completed,
            "a command's own exit 64 is not the sandbox"
        );
    }
}
