# Agent Note: 沙箱不可用时不做危险命令检测

Status: implemented

## 问题

review-branch 时发现设计文档前后矛盾。permissions.md §1 的流程图先做危险命令检测，再判断沙箱是否可用。按这个顺序，沙箱不可用时，`rm -rf build` 会先出一张危险命令卡片。用户批准后，bash 仍返回 `sandbox_unavailable`，命令不执行。§9.2 #13 则要求沙箱不可用时不出卡片。

还有一个相关问题：界面从哪里得知沙箱状态。快照如果另存一份状态，它可能与工具实际使用的后端不一致。

## 决策

- 以 §9.2 #13 为准。§2.1 的流程图改为先判断沙箱是否可用，再做危险命令检测。
- `prepare`（`crates/openwork-tools/src/prepare.rs`）只在 `ToolSessionContext::escalation_available()` 为真时调用 `detect_danger`。沙箱不可用时，`PreparedCall.danger` 总是 `None`，越界参数也不解析。
- bash 不执行，返回 `sandbox_unavailable`。run loop 在 Trace 里记 `permissionDecision = deny`、来源 `sandbox_unavailable`。
- `SessionSnapshot.sandbox` 的类型是 `SandboxStatus`（`crates/openwork-sandbox/src/probe.rs`）。它序列化为 `{state: "available"}` 或 `{state: "unavailable", reason}`。
- actor 从 `FinalizedToolset` 的 `sandbox_status()` 取这个值。它读的是工具集使用的 `SandboxBackend::status()`，Core 不另存一份。
- 同一次 review 把 `control_tool` 补进 §7 的 `permissionDecisionSource` 取值与 §6.4 的分类。
- 测试：tools `prepare::an_unavailable_sandbox_reports_no_dangerous_command`；core `session_runtime::acc_13_an_unavailable_sandbox_never_asks_about_a_dangerous_command`。

设计见 [permissions.md §1、§6、§10.1](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**按原流程图先检测危险命令。** 没有采用：为一条不会执行的命令出卡片，只会让用户多做一次无用的决定（permissions.md §10.1）。

**快照另存一份沙箱状态。** 没有采用：那样就有两个来源，界面可能显示与 bash 实际行为不同的结论。

## 后果

- 沙箱不可用时，交互式与非交互 Session 都不出卡片。用户只看到常驻提示。
- 这类调用的 Trace 里没有 `dangerMatch`。回看时看不出这条命令本来会命中清单。
- 界面提示、`runtime/sandbox-policy` 与 bash 的行为来自同一个后端状态。
- 这个状态在进程启动时由自检定下，直到下次启动才会改变。
- 危险命令清单为什么是封闭的，见 [危险命令清单是封闭的](2026-09-24-closed-danger-command-list.md)。
