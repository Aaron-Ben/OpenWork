//! Telling "the kernel refused the command" from "the sandbox never started"
//! (permissions.md §3.3).

/// What a finished `sandbox-exec` run means for the tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// The command ran; its exit code is its own.
    Completed,
    /// The command ran and hit a Seatbelt denial. This is an inference: other
    /// sources of `EPERM` are marked too, which costs one extra escalation
    /// hint at worst.
    Denied,
    /// `sandbox-exec` itself failed before running the command. Nothing ran;
    /// the sandbox is unavailable, not the command denied.
    SandboxFailed { message: String },
}

/// `sandbox-exec`'s own exit codes: usage (64), profile error (65), exec of
/// the command failed (71). Commands can exit with these too, so the code is
/// only trusted together with `sandbox-exec`'s own message at the start of
/// the output — it prints before the command starts.
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

/// `EPERM` as runtimes spell it: C and Rust write "Operation not permitted",
/// Go and Node "operation not permitted".
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
