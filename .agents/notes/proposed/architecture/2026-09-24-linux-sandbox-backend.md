# Agent Note: Linux 沙箱后端：bwrap，不可用时退到 Landlock

Status: proposed

## 问题

`probe`（`crates/openwork-sandbox/src/probe.rs`）在非 macOS 平台直接返回不可用。所以在 Linux 上 bash 停用，只有文件工具可用。

Linux 上没有 `sandbox-exec`。常见的选择是 bwrap，但它在容器里、或系统禁用了非特权 user namespace 时不能用。Landlock 不需要 user namespace，但它的规则模型与 Seatbelt 不同。

## 提议

- 新增 Linux 后端，实现 `SandboxBackend`（`crates/openwork-sandbox/src/backend.rs`）。首选 bwrap，bwrap 不可用时退到 Landlock。
- 用功能性探测在两者之间仲裁。与 Seatbelt 自检一样，只有被禁止的写入真的失败，才认定可用。
- 两个后端都从同一个 `SandboxPolicy` 推导，四档路径不变。
- 两者都不可用时，bash 照旧停用。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**DSH 的 runner 链。** DSH 在 Linux 上先 bwrap、后 Landlock。有多个候选时，它按顺序各探测一次，缓存第一个可用的结论（`packages/sandbox/sandbox-local/src/index.ts`、`packages/sandbox/sandbox-local/README.md`）。它的 bwrap 探测只运行 `true`。本提议采用它的顺序，但探测要求拒绝真的发生，理由见 [沙箱不可用时 bash 停用，不提供逃生口](../../implemented/architecture/2026-09-24-sandbox-fail-closed-without-escape-hatch.md)。

## 验收条件

- 在 Linux 上，`crates/openwork-sandbox/tests/matrix.rs` 的日常命令在 bwrap 与 Landlock 下得到与 macOS 相同的结论。
- `crates/openwork-sandbox/tests/parity.rs` 的对等测试在 Linux 内核上通过。
- bwrap 不可用时选中 Landlock。两者都不可用时 bash 停用，界面显示原因。
- 拒绝识别能区分后端自身失败与命令被拒绝。

## 风险

- `classify`（`crates/openwork-sandbox/src/denial.rs`）只认 `Operation not permitted`。bwrap 的只读挂载通常报 `Read-only file system`，Landlock 通常报 `Permission denied`（都未确认）。拒绝识别要按后端扩展，否则模型得不到越界提示。
- 工作区内的正则档位（`.git/hooks`、`.env*`）在 bwrap 中要用挂载表达。Landlock 只能加授权，可能表达不了“允许子树、扣除其中一部分”（未确认）。某些档位可能在 Linux 上无法与 macOS 完全对等。
- 协作 Engine 的约束经 `Seatbelt::confine` 生成（`crates/openwork-collab/src/computer/opencode/launch.rs`），不在 `SandboxBackend` 上。Linux 后端也要覆盖它。
- `SandboxRuntime::start`（`crates/openwork-core/src/session_tools.rs`）直接创建 `Seatbelt`。加入第二个后端时，要改为按平台选择。
