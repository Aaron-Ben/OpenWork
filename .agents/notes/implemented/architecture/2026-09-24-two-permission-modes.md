# Agent Note: 只保留两个权限模式，子 Agent 的模式在派生时定下

Status: implemented

## 问题

沙箱方案的草案里有一个内部的 ReadOnly 模式，用户的两个模式之外另有这一档。于是界面上有两个模式，代码里有三个。

子 Agent 带来两个问题。第一，父会话切换模式后，已派生的子 Agent 是否跟着变。第二，委派不能成为放宽权限的途径。

旧的审批卡片可以切换模式，所以 `PermissionResolved` 带一个 `permissionMode` 字段。新设计里，卡片只回答“这一次要不要”，这个字段失去了来源。

旧实现的模式不落盘，进程重启后回到默认。默认模式是较宽的 `auto`。用户可能刻意把一个会话切到 `accept-edits`。如果模式不落盘，重启就会悄悄放宽这个会话。

## 决策

- `SandboxMode` 只有 `AcceptEdits` 与 `Auto` 两个变体，按从窄到宽排序（`crates/openwork-sandbox/src/policy.rs`）。用户与角色从同样的两个取值里选。
- 角色用 `AgentDefinition.sandbox_ceiling` 声明上限（`crates/openwork-agent/src/definition.rs`）。explorer 的上限是 `AcceptEdits`（`crates/openwork-agent/src/explorer.rs`）。它没有 write / edit，bash 又写不了工作区，所以改不了仓库。
- 子 Agent 的模式是 `sub_agent_mode(parent)`，即 `parent.min(ceiling)`（`crates/openwork-core/src/session_tools.rs`）。`start_sub_agent`（`crates/openwork-core/src/core.rs`）在派生时把它写入 `sessions.sandbox_mode`。父会话之后切换，不影响它。
- `OpenWorkCore::set_permission_mode` 先检查 `parent_session_id`。子 Agent 直接返回 `OpenWorkCoreError::SubAgentModeFixed`，不写库，也不通知 actor。根会话先落库，再通知 actor。
- run loop 在每次 Tool Call 前从 `permission_state` 读取模式。因此 Turn 运行时也能切换，下一次调用生效。Desktop 的 `PermissionModeSelect` 不再因 Turn 运行而禁用。
- `SessionUpdate::PermissionResolved` 只剩 `toolCallId` 与 `decision`（`crates/openwork-core/src/session/updates.rs`）。Session Update 与快照共用 `SESSION_UPDATE_VERSION = 7`，与 `desktop/src/bridge/compat.ts` 的值一致。
- 迁移 `202609240002_add_session_sandbox_mode.sql` 加入 `sandbox_mode` 列，默认 `auto`。已有子 Agent 会话回填 `accept_edits`，即 explorer 的上限。

设计见 [permissions.md §2、§13.1、§13.3](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**保留内部的 ReadOnly 模式。** 原始记录只写了“删除”，没有保存它的定义与删除理由。现在的结构说明了它为什么多余：`accept-edits` 加上“工具面里没有 write / edit”，已经让 explorer 改不了仓库。

**已有子 Agent 会话按默认值回填 `auto`。** 开发计划原定已有会话一律取 `auto`。没有采用：现有子 Agent 角色只有 explorer。取 `auto` 会让恢复后的 explorer 越过自己的上限。

**子 Agent 继承父会话的模式。** DSH 的子 Agent 取父会话的沙箱模式覆盖值（`packages/subagent/subagent/src/child-agent.ts`）。没有采用：explorer 这类只读角色会因父会话是 `auto` 而获得写权限。maka 要求子边界包含于父边界（`packages/runtime/src/session-manager.ts` 调用 `executionBoundaryContains`）。取两者中较窄者，与 maka 的效果等价。

**模式不落盘，重启后回到默认。** 这是旧实现的做法。没有保留：默认较宽，不落盘会悄悄放宽会话。DSH 把会话的模式写进会话日志，并从日志恢复（`packages/sandbox/sandbox-policy/src/session-mode.ts`）。maka 的会话边界带修订号（`packages/core/src/sandbox-boundary.ts` 的 `ExecutionBoundary.revision`），它如何落库没有读到（未确认）。本设计与两者等价。

## 后果

- 委派不能放宽权限。父会话切到 `accept-edits` 后，新派生的子 Agent 回不到 `auto`。
- 子 Agent 的模式在整个生命周期内固定。父会话之后放宽，已派生的子 Agent 也不会变宽。
- explorer 不能请求越界。要写 `target/` 的 `cargo build` 会被内核拒绝，所以它只能做不写工作区的检查。
- `sub_agent_mode` 直接取 explorer 的上限。新增上限不同的角色时，要改成按角色取上限。
- 卡片与模式的职责分开。模式只能从常驻的指示器切换。
- 进程重启后，模式恢复为该会话最后的模式。模式在界面上常驻可见，持久化不会藏起任何东西。
