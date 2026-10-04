/** 一次结束的 `sandbox-exec` 运行意味着什么。 */
export type SandboxOutcome =
  /** 命令执行了，退出码是它自己的。 */
  | { kind: "completed" }
  /** 命令执行了，并碰到了 Seatbelt 的拒绝。这是推断：其他来源的 EPERM 也会被这样标记。 */
  | { kind: "denied" }
  /** `sandbox-exec` 在运行命令之前就失败了，什么都没执行。这是沙箱不可用，不是命令被拒。 */
  | { kind: "sandbox-failed"; message: string };

/**
 * `sandbox-exec` 自己的退出码：用法错误（64）、profile 错误（65）、exec 命令失败（71）。
 * 命令本身也可能以这些码退出，所以只有输出开头同时是 `sandbox-exec` 自己的消息时才采信，它在命令启动前打印。
 */
const SANDBOX_EXEC_FAILURES = new Set([64, 65, 71]);

/** 根据退出码与 stderr 区分三种结果。做法沿用 Rust 版的 `crates/openwork-sandbox/src/denial.rs`。 */
export function classifySandboxExit(exitCode: number, stderr: string): SandboxOutcome {
  const start = stderr.trimStart();
  if (
    SANDBOX_EXEC_FAILURES.has(exitCode) &&
    (start.startsWith("sandbox-exec:") || start.startsWith("Usage: sandbox-exec"))
  ) {
    return { kind: "sandbox-failed", message: start.split("\n")[0] ?? start };
  }
  // C 与 Rust 写 "Operation not permitted"，Go 与 Node 写 "operation not permitted"。
  if (exitCode !== 0 && stderr.toLowerCase().includes("operation not permitted")) {
    return { kind: "denied" };
  }
  return { kind: "completed" };
}
