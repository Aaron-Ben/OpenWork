# Agent Note: Trace 时间线的权限类别

Status: implemented

## 问题

绝大多数调用不出卡片，用户当场看不到它们。回看的地方是 Trace。permissions.md §14.1 规定，时间线必须分得出七类调用。实现时，有三种 Tool Span 落不进这七类：

- Core 控制工具（`update_plan`、子 Agent 工具），它们不经沙箱；
- 等待审批时用户取消了 Turn；
- 属性缺失，或来自旧版本的 Trace。

还有一种 Span 同时满足两类：用户批准了越界或危险命令，命令也执行了，但内核仍拒绝了其中某个文件操作。

## 决策

- `permissionCategory`（`desktop/src/features/traces/permissionCategory.ts`）有十个取值：七类，加上 `control_tool`、`cancelled`、`unknown`。`unknown` 显示“来源未知”，不留白。
- 判断有固定顺序。没有执行的调用先判：`sandbox_unavailable`、`cancelled`、用户拒绝、规则拒绝。之后，`permissionDecision` 不是 `allow` 的归 `unknown`。
- 执行过的调用先看 `control_tool`，再看 `sandboxDenied`。因此，用户批准过、却被内核拒绝的调用归入“被沙箱拒绝”。
- Core 写入的取值：控制工具记 `allow` + `control_tool`；等待审批时取消记 `cancelled` + `system`；沙箱不可用记 `deny` + `sandbox_unavailable`（`crates/openwork-core/src/session/run_loop/`）。
- 测试：`desktop/src/features/traces/permissionCategory.test.ts`、`desktop/src/features/traces/components/TraceTimeline.test.tsx`。

设计见 [permissions.md §14.1、§14.2](../../../../docs/subsystems/permissions.md) 与 [trace.md §7.2](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**按审批来源归类“批准后被内核拒绝”的调用。** 也就是归入“用户批准的越界”或“用户批准的危险命令”。没有采用：回看时要找的是结果。用户批准过，不等于命令做成了。

## 后果

- 时间线上每个 Tool Span 都有类别。缺属性的 Span 显示“来源未知”，不会被误读成沙箱内自动执行。
- 批准后被拒的调用不出现在“用户批准”两类里。统计批准次数时，要另查 `escalationPaths`、`dangerMatch` 与来源。
- `sandboxDenied` 是推断（permissions.md §7）。其他原因产生的 `EPERM` 也会归入“被沙箱拒绝”。
- 新增 `permissionDecisionSource` 取值时，必须同时改这个函数。否则新取值会落入 `unknown`。
