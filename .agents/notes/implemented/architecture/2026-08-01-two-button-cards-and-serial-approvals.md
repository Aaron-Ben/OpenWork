# Agent Note: 卡片只有两个按钮，审批串行进行

Status: implemented

## 问题

大多数调用不出卡片，出卡片的只有越界与危险命令。卡片上的每个按钮都是一种授权。按钮的宽度与生存期，决定用户在打断时刻要判断什么。

一次模型响应可以含多个 Tool Call。几张卡片同时出现时，用户看不清顺序。用户拒绝与规则拒绝的含义也不同，Turn 是否继续要分开处理。还要决定：是否让另一个模型替用户批准。

## 决策

- `PermissionDecision`（`crates/openwork-core/src/session/commands.rs`）只有 `AllowOnce` 与 `Deny`。`ApprovalDialog`（`desktop/src/features/chat/components/ApprovalDialog.tsx`）只有这两个按钮。
- 卡片不切换模式。模式只能从常驻的指示器切换。
- 用户拒绝时，`authorize_tool_call` 返回 `TurnRunError::PermissionDenied`，Turn 停下（`crates/openwork-core/src/session/run_loop/authorization.rs`）。
- 规则拒绝作为 tool result 返回，Turn 继续。它包括硬保护、越界校验失败与非交互 Session。沙箱拒绝是结果事实，Turn 也继续。
- run loop 按顺序逐个授权并执行 Tool Call（`crates/openwork-core/src/session/run_loop/mod.rs`）。一个调用让 Turn 停下后，其余调用记为未执行。
- 没有审查模型。`permissionDecisionSource` 没有模型审查者这一取值。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**「本会话允许」与「切到 acceptEdits」按钮。** 2026-08-01 的设计有三个按钮：「允许一次」、「本会话允许 `<归约结果>`」或「本会话不再询问文件改动（切到 acceptEdits）」、「拒绝」。没有保留：会话授权是累积的，用户看不见；越界只批一次，见 [越界只列具体路径，只批这一次](2026-09-24-path-only-single-use-escalation.md)。卡片回答“这一次要不要”，模式是会话姿态，两者分开。

**「总是允许」写持久规则。** Codex 的“总是允许”写入 `default.rules`（`codex-rs/core/src/exec_policy.rs`）。没有采用：用户判断得了眼前的命令，判断不了一条永久规则以后覆盖什么。

**并行出卡片。** 没有采用：用户要在竞争的窗口之间来回切换。看不清顺序，就看不清将要发生什么。

**审查模型。** Codex 有 Guardian（`codex-rs/core/src/guardian/`）。maka 的审批者取值有 `auto_review`（`packages/core/src/permission.ts` 的 `APPROVALS_REVIEWERS`）。没有采用：越界的最终门控是人看到的命令与理由。

## 后果

- 每张卡片只问一次决定，作用范围写在文案里。
- 代价：同类越界每次都要点一下。提交类 git 操作每次一张卡片。
- 用户拒绝后，同一响应里后续的 Tool Call 都不执行。模型没有机会换方案，用户要重新发消息。
- 审批与执行交替进行：前一个调用执行完，下一个才出卡片。2026-08-01 的设计要求“全部批准完再派发执行”，现在的代码不是这样。
- Trace 记录 `deny` + `user` 与 `permissionWaitMs`。
- 测试：core `acc_24_a_user_denial_stops_the_turn_without_running_the_tool`、`acc_41_multiple_permission_requests_are_presented_serially`。
