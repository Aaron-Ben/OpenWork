# 权限策略与 Human-in-the-loop 审批

Last reviewed: 2026-07-11

> Status: current implementation detail. 当前已经完成进程内的策略判定、人工审批和资源权限检查链路；应用重启后的审批恢复仍依赖后续 Event Journal 与 Durable Turn。

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

`TurnId`、`ActionRunId` 和 `ApprovalId` 是不同的强类型 ID：

- `TurnId` 当前来自聊天请求的 `request_id`。
- `ApprovalId` 由 Core 使用 UUID 创建，不复用厂商 tool-call ID。
- `ActionRunId` 当前只在 `RequireApproval` 分支创建，用于关联审批请求和结果。

最后一点是当前实现边界：尚未做到“每一个 Action 都先创建 ActionRunId 并记录完整生命周期”。

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
  -> Core 创建 ApprovalId 和 ActionRunId
  -> TurnCommandInbox::begin_approval
  -> ApprovalState = Waiting
  -> emit ApprovalRequested
  -> wait_for_resolution
```

Core 先写入 `Waiting` 状态，再发出 `ApprovalRequested`。因此 Desktop 看到审批卡片时，Core 已经能够接收对应的 `ResolveApproval` 命令。

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
| 用户 Allow | emit `ApprovalResolved`，然后调用 Action |
| 用户 Deny | emit `ApprovalResolved`，生成 `ApprovalDenied` Observation，不调用 Action |
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
  -> 内部 ChatRuntime::resolve_approval
  -> TurnSupervisor 查找 TurnCommandHandle
  -> Core 校验并应用命令
```

Turn 完成、失败或取消后，App 会移除 `TurnId -> TurnCommandHandle` 路由。迟到的审批命令会得到 `TurnNotFound`，不会执行 Action。

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
  -> 构造 ActionRequest
  -> ExecutionPort::authorize

     [Deny]
       -> PermissionDenied Observation
       -> 不创建审批
       -> 不调用 Handler

     [Allow]
       -> ExecutionPort::execute
       -> Handler 仍可因 PermissionProfile 拒绝

     [RequireApproval]
       -> Core 创建 ApprovalId + ActionRunId
       -> Waiting
       -> emit ApprovalRequested
       -> Desktop 显示审批卡片
       -> ResolveApproval(turn_id, approval_id, resolution)

          [Deny]
            -> emit ApprovalResolved
            -> ApprovalDenied Observation
            -> 不调用 Handler

          [Allow]
            -> emit ApprovalResolved
            -> ExecutionPort::execute
            -> Handler 权限检查
            -> Succeeded / PermissionDenied / Failed / Cancelled / OutcomeUnknown

          [Cancel / Channel closed / State unavailable]
            -> 不调用 Handler
            -> Cancelled 或 ApprovalDenied Observation
```

## 9. 事件记录和持久化边界

`ApprovalRequested` 和 `ApprovalResolved` 会被 App 映射为：

```text
approval_request
approval_resolved
```

这些 payload 只实时发送给 Desktop，不再通过 detached task 写数据库。Turn 结束时 Tool Call/Tool Result 会作为 Assistant/Tool Message 写入 `recorded_events`，但 `approval_requested` 和 `approval_resolved` 尚未在各自语义点持久化。

因此当前真正的审批状态仍只存在于进程内的 Core Turn 中，不能在应用重启后恢复 Waiting 状态。下一阶段必须在暂停和恢复状态转换前直接 await `EventJournal`，不能重新引入 best-effort trace。

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
- 用户 Allow 后仍必须通过 Handler 权限检查。

尚未保证：

- OS 级 sandbox；
- 参数语义级风险计算；
- 真正网络隔离；
- 每个 Action 都具有完整 ActionRun 生命周期；
- 审批事件的可靠、原子持久化；
- 应用崩溃或重启后的审批恢复；
- 跨进程或远程宿主的 Turn 命令协议。

## 12. 当前测试覆盖重点

- Protocol 审批合同序列化和强类型 ID；
- `Untrusted -> RequireApproval`、`Never -> Allow`；
- 错误 `turn_id`、错误 `approval_id` 和未知 Turn；
- 并发重复 resolution；
- 用户 Allow 后执行 Action；
- 用户 Deny 不执行 Action；
- Cancel 唤醒等待状态；
- workspace 内读写、workspace 外拒绝和受保护元数据拒绝；
- Capability Catalog 与 Handler 名称一致。
