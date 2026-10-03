# Agent Note: 沙箱不可用时 bash 停用，不提供逃生口

Status: implemented

## 问题

沙箱可能失效：`sandbox-exec` 缺失、profile 被拒绝，或某个系统版本改变了行为。Seatbelt 配置失效时，它照样报告“已应用”，命令照样运行。Apple 已把 `sandbox-exec` 标记为弃用，所以这不只是假设。

系统要决定两件事。第一，怎样发现失效。第二，失效后 bash 怎么办。

## 决策

- 进程启动时，`SandboxRuntime::start`（`crates/openwork-core/src/session_tools.rs`）调用一次 `Seatbelt::probe(SANDBOX_EXEC)`。结论在进程生命周期内不变。
- `probe`（`crates/openwork-sandbox/src/probe.rs`）先在沙箱内运行 `exit 0`，再尝试写一个被禁止的探针文件。只有写入失败、文件不存在、`classify` 判为 `Denied` 时，才认定可用。非 macOS 平台直接判为不可用。
- 不可用时，`SandboxBackend::wrap` 返回 `SandboxUnavailable`。bash 返回 `ToolErrorCode::SandboxUnavailable`，不执行（`bash.rs` 的 `sandbox_unavailable`）。交互式与非交互 Session 相同。
- `sandbox-exec` 自己以 64、65、71 退出，并且输出以它的消息开头时，`classify` 返回 `SandboxFailed`。这也归为不可用，不归为被拒绝。
- 文件工具不受影响。越界参数从 schema 中删除（`crates/openwork-tools/src/escalation.rs` 的 `ESCALATION_FIELDS`）。
- 代码中没有不经 Seatbelt 启动 bash 的路径，也没有“这一次不用沙箱”的选项。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**只验证“命令能跑起来”。** DSH 的 Seatbelt 探测在 read-only profile 下运行 `true`，退出 0 就认为可用（`packages/sandbox/sandbox-local/src/index.ts` 的 `defaultProbeSeatbelt`）。DSH 只在有多个候选时才探测；macOS 只有一个候选，所以实际不探测（`packages/sandbox/sandbox-local/README.md`）。没有采用：配置失效时命令照样能跑。自检要证明的是拒绝真的会发生。

**第一次调用 bash 时再自检。** 没有采用：放在启动时，界面与模型在第一次调用之前就知道 bash 能不能用。

**沙箱坏了就临时放开。** DSH 的 `danger-full-access` 绕过约束（`packages/sandbox/sandbox/src/index.ts`）。maka 没有沙箱时返回 `requires_bypass`，切到 `bypass` 边界后命令不经沙箱运行（`packages/runtime/src/builtin-tools.ts`）。没有采用：沙箱真的坏掉时，用户最常点的就是这个按钮，而那时用户最难判断风险。沙箱故障应当修好，不应绕过。

## 后果

- 代价：沙箱故障期间 bash 完全不可用。DSH 与 maka 不切换模式时也是这样。
- 用户看到常驻提示与自检的原因。模型从 `runtime/sandbox-policy` section 得知 bash 不可用，见 [沙箱策略作为最后一个 world state section](2026-09-24-sandbox-policy-world-state-section.md)。
- 如果某个 macOS 版本移除了 `sandbox-exec`，结果是自检失败、bash 停用，而不是静默地不受约束。DSH 与 Codex 同样依赖它（`codex-rs/sandboxing/src/seatbelt.rs` 的 `MACOS_PATH_TO_SEATBELT_EXECUTABLE`）。
- 每次启动多运行两次 `sandbox-exec`，放在阻塞线程上。
- 沙箱不可用时不做危险命令检测，见 [沙箱不可用时不做危险命令检测](2026-09-24-sandbox-unavailable-skips-danger-detection.md)。
- 测试：`crates/openwork-sandbox/tests/probe.rs` 的 `acc_12_the_real_sandbox_passes_its_self_check`、`a_sandbox_that_does_not_deny_fails_the_self_check`、`acc_15_denials_and_sandbox_failures_are_told_apart`；tools `sandbox_calls::acc_10d_bash_does_not_run_when_the_sandbox_is_unavailable`。
