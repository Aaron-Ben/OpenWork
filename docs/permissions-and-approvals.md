# 权限策略与 Human-in-the-loop 审批

Last reviewed: 2026-07-15

> Status: current implementation detail. 当前已完成策略判定、人工审批、资源权限检查、审批事实持久化，以及 pending approval 的应用重启恢复；OS 级 sandbox 和已开始工具的自动对账仍未完成。

## 1. 三道不同的安全边界

一次 Action 执行会依次经过三道边界：

```text
Execution Policy
  Capability 查找 + Schema 校验 + ApprovalPolicy + CapabilityRiskHint
  -> Allow | Deny | RequireApproval

Human Approval
  仅 RequireApproval 进入
  -> Core Waiting
  -> ResolveApproval
  -> Allow | Deny | Cancelled

Resource Enforcement
  仅前两层允许后进入
  -> ExecutionPort::execute
  -> Action Handler
  -> PermissionProfile 路径检查、取消、超时等运行时限制
  -> Observation
```

这三层不能互相替代：

- `ApprovalPolicy::Never` 只是不等待人工审批，不会绕过 Handler 的路径权限。
- 用户点击 Allow 只允许这一次 Action 继续执行，不代表 Action 一定成功。
- `PermissionProfile` 允许某个路径，也不代表用户审批策略一定允许立即执行。
- 当前权限检查不是操作系统级 sandbox。

## 2. 模块职责

| 模块 | 当前职责 | 明确不负责 |
| --- | --- | --- |
| `openwork-protocol` | 审批合同、策略结果、强类型 ID | 等待 UI、执行 Action |
| `openwork-execution` | 策略判定、参数校验、Handler 调用、资源权限检查 | 持有审批等待状态 |
| `openwork-core` | Turn 控制循环、审批状态机、暂停和恢复 | 定位 Desktop 组件、访问数据库 |
| `openwork-app` | 组合 Core/Execution、按 Turn 路由命令 | 替 Core 决定审批结果 |
| Desktop | 展示审批请求、提交用户决定 | 直接唤醒 Agent 或调用 Handler |

旧的 `openwork-permissions`、`openwork-agent` 和 `openwork-runtime` crate 已删除。其职责分别进入 Execution、Core、App 和 Protocol。

## 3. Protocol 审批合同

代码位置：

```text
crates/openwork-protocol/src/approval/mod.rs
crates/openwork-protocol/src/domain/ids.rs
crates/openwork-protocol/src/turn/mod.rs
```

当前合同包括：

| 类型 | 作用 |
| --- | --- |
| `ApprovalPolicy` | 用户选择的审批策略 |
| `ExecutionPolicyDecision` | Execution 返回的 `Allow / Deny / RequireApproval` |
| `ApprovalRequested` | Core 已经进入等待状态的事实 |
| `ApprovalResolution` | 用户的 `Allow / Deny` 决定 |
| `ResolveApproval` | App 路由回所属 Turn 的命令 |
| `ApprovalResolved` | Core 已经应用决定的事实 |

`TurnId`、`StepId`、`ToolRunId` 和 `ApprovalId` 是不同的强类型 ID：

- `TurnId` 当前来自聊天请求的 `request_id`。
- `StepId` 标识一次模型调用及其返回的一组工具调用。
- `ToolRunId` 由 Core 为每个工具调用创建，不复用厂商 tool-call ID。
- `ApprovalId` 由 Core 使用 UUID 创建，不复用厂商 tool-call ID。

Provider tool-call ID 只用于回填模型上下文；`ToolRunId` 才是内部执行生命周期身份。无论策略结果是 Allow、Deny 还是 RequireApproval，每个工具调用都会先创建并记录自己的 `ToolRunId`。

## 4. 第一阶段：Execution Policy

代码位置：

```text
crates/openwork-execution/src/service.rs
crates/openwork-execution/src/policy/evaluator.rs
```

`ExecutionPort::authorize(request, approval_policy)` 当前按以下顺序执行：

```text
按 Action 名称解析 CapabilitySpec
  -> Capability 不存在或 Resolver 失败：Deny
  -> 根据 Capability Schema 校验 input
  -> 参数不合法：Deny
  -> ApprovalPolicy + CapabilityRiskHint
       Untrusted -> RequireApproval
       Never     -> Allow
```

当前 `CapabilityRiskHint` 只用于生成审批原因：

```text
ReadOnly          -> read-only action requires user approval
WorkspaceMutation -> workspace mutation requires user approval
ProcessExecution  -> process execution requires user approval
```

当前策略尚未根据下列实际语义计算不同风险：

- 文件 path 是否位于 workspace 内；
- bash 命令具体执行什么；
- 网络连接目标；
- 是否发生数据外发；
- Action 运行在本机、sandbox 还是远程服务。

`authorize` 会读取并进行 Schema 校验，但目前不会根据这些参数的具体含义改变策略结果。

### Policy 结果的后续行为

| 结果 | Core 行为 | 是否调用 Handler |
| --- | --- | --- |
| `Allow` | 直接调用 `ExecutionPort::execute` | 是 |
| `Deny` | 生成 `PermissionDenied` Observation | 否 |
| `RequireApproval` | 创建审批并暂停 Turn | 用户允许后才调用 |

## 5. 第二阶段：Core 人工审批

代码位置：

```text
crates/openwork-core/src/agent.rs
crates/openwork-core/src/approval.rs
```

只有 `ExecutionPolicyDecision::RequireApproval` 会进入审批状态机。当前代码顺序是：

```text
ExecutionPort::authorize 返回 RequireApproval
  -> Core 已持有该调用的 StepId 和 ToolRunId，并创建 ApprovalId
  -> TurnCommandInbox::begin_approval
  -> ApprovalState = Waiting
  -> await persist approval_requested
  -> emit ApprovalRequested
  -> wait_for_resolution
```

Core 先写入进程内 `Waiting` 状态，再同步持久化 `approval_requested`，最后发出 Live `ApprovalRequested`。因此 Desktop 看到审批卡片时，Core 已经能够接收命令，Journal 也已经能够重建该待审批状态。

### 状态变化

```text
Idle 或上一次 Resolved
  -> Waiting(ApprovalRequested)
       -> Resolved(Allow)
       -> Resolved(Deny)
       -> Cancelled
```

以下异常也会按取消/拒绝处理，不会执行 Action：

- Turn command channel 被关闭；
- 审批共享状态不可用；
- `CancellationToken` 被触发。

当前没有单独的 `ApprovalCancelled` Protocol 事件；取消最终通过 Turn 的 `cancelled` 流程向外体现。

### 命令路由与校验

Core 使用容量为 8 的 `mpsc` channel 接收 Turn 命令。`TurnCommandHandle::resolve` 会校验：

- 命令的 `turn_id` 必须匹配所属 Turn；
- `approval_id` 必须等于当前等待的审批；
- 当前必须处于 Waiting；
- 同一审批不能同时提交两个 resolution；
- channel 或审批状态不可用时返回错误。

内部 `oneshot` 只用于把“Core 是否接受命令”的结果返回给 App。它不承载 UI 审批等待；真正的审批决定通过有界 `mpsc` 命令返回 Turn。

### 审批结果

| 结果 | 后续行为 |
| --- | --- |
| 用户 Allow | 持久化 `approval_resolved + tool_run_started`，emit `ApprovalResolved`，然后调用 Action |
| 用户 Deny | 持久化 `approval_resolved`，emit `ApprovalResolved`，生成 `ApprovalDenied` Observation，不调用 Action |
| Cancelled | 生成 Cancelled Observation，不调用 Action |
| Channel/State 异常 | fail closed，生成 ApprovalDenied Observation |

## 6. App 与 Desktop 路由

代码位置：

```text
crates/openwork-app/src/turn_supervisor.rs
crates/openwork-app/src/chat.rs
apps/desktop/src-tauri/src/commands/chat.rs
apps/desktop/src/hooks/useChatStreamListener.ts
apps/desktop/src/components/chat/ApprovalDialog.tsx
```

App 在聊天 Turn 启动时：

```text
TurnId = request_id
  -> TurnSupervisor::register
  -> 保存 TurnId -> TurnCommandHandle
  -> 将 TurnCommandInbox 交给 Core AgentConfig
```

Desktop 收到 `approval_request` 后展示审批卡片。用户操作会调用：

```text
resolve_approval(turn_id, approval_id, allow)
  -> Tauri 调用 OpenWorkApplication::turns
  -> TurnApplicationService 构造 ResolveApproval
  -> 活跃 Turn：TurnSupervisor 查找 TurnCommandHandle，Core 校验并应用命令
  -> 非活跃 Turn：从 Journal 查询 pending approval，重建 Core/Execution 后应用命令
```

Turn 完成、失败或取消后，App 会移除 `TurnId -> TurnCommandHandle` 路由。若 Journal 中没有匹配的 pending approval，迟到或错误的审批命令会被拒绝，不会执行 Action。

## 7. 第三阶段：资源权限检查和执行

代码位置：

```text
crates/openwork-execution/src/service.rs
crates/openwork-execution/src/context.rs
crates/openwork-execution/src/policy/profile.rs
crates/openwork-execution/src/actions/
```

人工审批允许后，Core 调用 `ExecutionPort::execute`：

```text
再次解析 CapabilitySpec
  -> 再次校验 input Schema
  -> ActionInvoker 路由到 Handler
  -> Handler 使用 ExecutionContext 执行权限检查
  -> 返回 Observation
```

重复解析和 Schema 校验让 `execute` 本身保持独立安全边界：即使调用者没有先经过 `authorize`，也不能直接绕过校验调用 Handler。

### PermissionProfile

当前文件系统模式包括：

| 模式 | 当前含义 |
| --- | --- |
| `ReadOnly` | 允许配置根目录内读取，拒绝所有写入 |
| `WorkspaceWrite` | 根据 read/write roots 允许访问，并保护元数据目录 |
| `DangerFullAccess` | 跳过应用层路径限制 |

`PermissionProfile::workspace_write(working_dir)` 当前配置：

- 允许读取 `working_dir`；
- 允许写入 `working_dir` 和系统临时目录；
- 拒绝写入路径中的 `.git`、`.agents`、`.codex`；
- 网络模式为 `Restricted`。

### 各 Handler 的路径检查

| Action | 当前资源检查 |
| --- | --- |
| `read` | 目标文件必须允许 `Read` |
| `write` | 目标文件必须允许 `Write` |
| `edit` | 目标文件必须允许 `Write` |
| `list` | 目标目录必须允许 `Read` |
| `grep` | 搜索根目录必须允许 `Read` |
| `glob` | 搜索根目录必须允许 `Read` |
| `bash` | 只检查 `working_dir` 可读，不解析命令内部文件访问 |

路径检查使用 lexical normalize，不依赖 `canonicalize`。因此它可以检查尚未创建的目标文件，但不能作为完整的 symlink 安全边界。

### 网络限制与 bash

当 `NetworkMode::Restricted` 时，bash 进程只会收到：

```text
OPENWORK_NETWORK_RESTRICTED=1
```

这只是环境信号，不会阻止程序主动联网。bash 通过 `sh -c` 执行自由文本命令，当前没有：

- OS 级文件系统 sandbox；
- 真正网络隔离；
- 完整 shell 语义分析；
- 对命令内部路径逐项执行 `PermissionProfile` 检查。

因此“用户批准 bash”只表示允许这次调用继续，不表示命令被限制在 workspace 内。

## 8. 完整分支流程

```text
模型返回 tool call
  -> Core 创建 StepId + ToolRunId
  -> await persist tool_run_requested
  -> 构造 ActionRequest
  -> ExecutionPort::authorize

     [Deny]
       -> PermissionDenied Observation
       -> 不创建审批
       -> 不调用 Handler

     [Allow]
       -> await persist tool_run_started
       -> ExecutionPort::execute
       -> Handler 仍可因 PermissionProfile 拒绝

     [RequireApproval]
       -> Core 创建 ApprovalId
       -> Waiting
       -> await persist approval_requested
       -> emit ApprovalRequested
       -> Desktop 显示审批卡片
       -> ResolveApproval(turn_id, approval_id, resolution)

          [Deny]
            -> await persist approval_resolved
            -> emit ApprovalResolved
            -> ApprovalDenied Observation
            -> 不调用 Handler

          [Allow]
            -> await persist approval_resolved + tool_run_started
            -> emit ApprovalResolved
            -> ExecutionPort::execute
            -> Handler 权限检查
            -> Succeeded / PermissionDenied / Failed / Cancelled / OutcomeUnknown

          [Cancel / Channel closed / State unavailable]
            -> 不调用 Handler
            -> Cancelled 或 ApprovalDenied Observation

  -> await persist ToolRun 终态 + tool_message_recorded
  -> await persist step_completed / step_failed
```

## 9. 事件记录和恢复边界

`ApprovalRequested` 和 `ApprovalResolved` 会被 App 映射为：

```text
approval_request
approval_resolved
```

Live payload 只负责当前 UI；对应的 `approval_requested`、`approval_resolved`、`tool_run_started` 和 ToolRun 终态由 Core 通过 `TurnRecorderPort` 在真实语义点同步写入 `recorded_events`。任何必须先于副作用成立的事实如果写入失败，本次工具执行不会开始。

Session 查询会重放 Turn 事实并返回 `pending_approval`。应用重启后，前端重新加载 Session 即可恢复审批卡片；用户决定到达时，App 重建 Provider、Execution 和 Core，从被暂停的 Step 继续。

安全边界是：只有存在 `approval_requested` 且不存在对应 `approval_resolved/tool_run_started` 的调用可自动恢复。若日志已有 `tool_run_started` 但没有完成、失败、拒绝、取消或未知终态，投影标记为 `outcome_unknown`，系统不会自动重跑，因为无法证明上一次进程是否已经产生副作用。

## 10. 拒绝与失败语义

| 来源 | Observation / Error | 是否执行 Handler |
| --- | --- | --- |
| Capability 不存在、Resolver 失败、Schema 非法 | Policy `Deny`，随后为 `PermissionDenied` Observation | 否 |
| 用户点击 Deny | `ApprovalDenied` Observation | 否 |
| 路径超出 PermissionProfile | `PermissionDenied` Observation | Handler 已进入，但真实 IO 前拒绝 |
| 审批 channel/state 不可用 | `ApprovalDenied` Observation | 否 |
| Turn 取消 | `Cancelled` Observation | 否，或中断正在执行的 Action |
| Handler IO/进程错误 | `ExecutionFailed` 等 Observation | 已进入 Handler |

当前 `authorize` 把 Capability/Schema 问题统一表示为 Policy `Deny`，Core 再映射为 `PermissionDenied`。这与 `execute` 阶段可返回的 `CapabilityNotFound`、`InvalidArguments` 结构化错误并不完全对称，是后续可以继续收紧的合同边界。

## 11. 当前已经保证和尚未保证的能力

已经保证：

- Execution 不等待 UI，只返回三态策略结果；
- Core 在发出审批事件前先进入 Waiting；
- Desktop 必须同时提交 `turn_id` 和 `approval_id`；
- 错误 Turn、错误审批 ID 和并发重复 resolution 被拒绝；
- 用户 Deny、取消或审批基础设施异常不会调用 Action；
- 用户 Allow 后仍必须通过 Handler 权限检查；
- 每个工具调用都有独立 `ToolRunId` 和可重放生命周期；
- pending approval 可在应用重启后恢复；
- `tool_run_started` 写入失败时不会调用 Handler；
- 已开始但没有终态的 ToolRun 被标记为 `outcome_unknown`，不会自动重跑。

尚未保证：

- OS 级 sandbox；
- 参数语义级风险计算；
- 真正网络隔离；
- 对 `outcome_unknown` ToolRun 的外部副作用对账和人工处置；
- 普通 interrupted Turn 的自动续跑；
- 跨进程或远程宿主的 Turn 命令协议。

## 12. 当前测试覆盖重点

- Protocol 审批合同序列化和强类型 ID；
- Turn/Step/ToolRun/Approval 事实序列化与生命周期投影；
- `Untrusted -> RequireApproval`、`Never -> Allow`；
- 错误 `turn_id`、错误 `approval_id` 和未知 Turn；
- 并发重复 resolution；
- 用户 Allow 后执行 Action；
- 用户 Deny 不执行 Action；
- 副作用开始前的持久化失败不会执行 Action；
- pending approval 的 Journal 恢复和继续执行；
- Cancel 唤醒等待状态；
- workspace 内读写、workspace 外拒绝和受保护元数据拒绝；
- Capability Catalog 与 Handler 名称一致。
