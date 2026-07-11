# OpenWork Core 架构蓝图

Last reviewed: 2026-07-11

> Status: architecture direction and invariant baseline. 本文冻结 OpenWork 要解决的核心问题、模块所有权、依赖方向和关键不变量，但不冻结具体实现方案，也不把 S0-S8 固定为必须照序执行的项目排期。Plan、Tool、Skill、MCP、Context、Memory、Sandbox 等重要子系统进入实现前，必须分别完成专题设计，并以当时的代码、评测和项目目标决定是否采用蓝图中的参考路径。

## 1. 结论

OpenWork 的架构中心不是聊天界面、模型 Provider、工具集合或某个参考项目，而是一个用 Rust 实现的 **Durable Agent Harness**：

> 让一项长任务在模型具有不确定性、上下文容量有限、工具可能失败、外部操作存在副作用、应用可能中断的情况下，仍能持续、安全、可恢复并可验证地推进到完成。

在命名上：

- `openwork-core` 是 Harness 核心，拥有长任务控制循环。
- 沿用并扩展 `openwork-protocol`，不新建 `openwork-domain`。`openwork-protocol` 定义稳定类型、状态、命令、事件和端口合同。
- `openwork-capabilities` 管理 Tool、Skill、MCP 和后续 Plugin 的发现与描述。
- `openwork-execution` 负责权限判断、沙箱和安全执行；审批工作流由 `openwork-core` 持有。
- `openwork-persistence` 负责事件日志、检查点、投影、Artifact 元数据和可靠恢复。
- `openwork-workspace` 表示 Agent 正在操作的真实项目环境。
- `openwork-app` 是宿主无关的应用入口和组件组合层。

这套架构首先服务于面试项目的技术表达：重点展示可靠 Agent Runtime，而不是与商业化通用 Work 产品竞争。

## 2. 核心问题：长任务为什么会失败

OpenWork 优先处理以下问题：

1. **目标漂移**：模型在多轮调用后偏离用户目标或遗忘硬约束。
2. **上下文退化**：文件、命令输出和历史消息持续增长，重要事实被噪声挤出窗口。
3. **动作失败**：工具参数错误、临时网络故障、权限拒绝和语义方案错误需要不同恢复策略。
4. **副作用不确定**：进程中断后无法确认文件写入、命令或外部操作是否完成。
5. **执行不可恢复**：应用崩溃或重启后只能重新开始，可能重复执行副作用。
6. **完成不可验证**：模型声称完成，不等于测试、文件状态或目标成果真的满足要求。
7. **环境不可信**：仓库文件、网页、MCP 返回值和用户粘贴的内容可能包含提示注入。

OpenWork 的四项核心保证是：

1. **状态可恢复**：任何已持久化任务都能重建当前状态。
2. **上下文可重建**：每次模型调用使用的关键来源、摘要和版本可追踪。
3. **动作可控制**：副作用经过风险判断、审批和沙箱边界。
4. **结果可验证**：完成由环境状态和验证规则决定，而不是模型自评。

如果一个新功能不能增强这四项保证中的至少一项，就不应进入当前主线。

## 3. 核心领域层级

OpenWork 使用以下层级表达任务：

```text
Project
└── Thread                       持续对话与项目上下文
    └── Turn                     一项可持续数分钟或数小时的完整任务
        ├── Plan                 可版本化执行计划
        ├── ModelAttempt[]       Turn 内的多次模型调用
        ├── ActionRun[]          文件、Shell、MCP 等动作执行
        ├── Approval[]           人工或策略审批
        ├── ContextCheckpoint[]  上下文阶段检查点
        ├── VerificationRun[]    测试与成果验证
        └── Artifact[]           日志、Diff、报告、PPT 等成果
```

重要定义：

- 一个 `Turn` 不是一次 LLM API 请求，而是一项完整目标驱动任务。
- 一个 `Turn` 内可以有多次 `ModelAttempt`、`ActionRun`、重试、压缩和重新规划。
- 用户审批、暂停和恢复不会自动创建新 Turn。
- 后续用户提出新的目标或明确结束当前目标后，才创建新的 Turn。

## 4. 长任务控制循环

```text
接收 TurnCommand
  -> 恢复或初始化 TurnState
  -> 选择 Direct / PlanFirst / Adaptive 执行模式
  -> Context Builder 构造本次 ModelAttempt 上下文
  -> 调用 ModelPort
  -> 解析文本、计划更新或 Capability Call
  -> 持久化 ActionRequested
  -> 权限与风险判断
  -> 必要时暂停并等待 Approval
  -> 持久化 ActionStarted
  -> 在受控环境中执行 Action
  -> 规范化 Observation
  -> 持久化 Action Outcome
  -> Recovery Policy 决定继续、重试、重新规划、对账或停止
  -> Context Policy 决定保留、Artifact 化、压缩或 Context Reset
  -> Verification Policy 检查完成条件
  -> 完成、失败、取消或进入 needs_reconciliation
```

核心副作用规则：

```text
persist intent -> execute effect -> persist outcome
```

- 对副作用 Action，`started` 事件必须先于真实执行持久化。
- 已有 `started` 但没有终态的 Action 不得自动重放。
- 无法确定外部结果时进入 `outcome_unknown`，Turn 进入 `needs_reconciliation`。
- 只读、幂等或带可靠 Idempotency Key 的 Action 才允许策略化自动重试。

## 5. 三类事件必须分开

### 5.1 Recorded Event

用于恢复、回放和审计，必须持久化：

```text
TurnStarted
PlanCreated
ModelAttemptStarted
ModelAttemptCompleted
ActionRequested
ApprovalRequested
ApprovalResolved
ActionStarted
ActionCompleted
ActionFailed
ActionOutcomeUnknown
ContextCheckpointCreated
VerificationStarted
VerificationCompleted
TurnCompleted
TurnFailed
```

### 5.2 Live Event

用于即时 UI，允许合并或丢失：

```text
TextDelta
ReasoningDelta
ActionOutputDelta
ProgressUpdate
Heartbeat
```

Live Event 不能作为恢复的唯一来源。

### 5.3 Telemetry Trace

用于性能、成本和诊断：

```text
model latency
queue wait
tokens and cost
context size
action duration
sandbox startup
retry count
projection latency
```

Telemetry 不能代替 Recorded Event；Recorded Event 也不应塞入全部性能细节。

## 6. 目标 Workspace 结构

```text
crates/
├── openwork-protocol/        # 稳定类型、状态、命令、事件、端口合同
├── openwork-core/            # Durable Agent Harness 与 Turn 控制循环
│   ├── planning/
│   ├── context/
│   ├── recovery/
│   ├── verification/
│   ├── budget/
│   └── loop_guard/
├── openwork-capabilities/    # Tool、Skill、MCP、Plugin 的发现与选择
├── openwork-providers/       # 厂商模型协议适配
├── openwork-execution/       # 权限决策、沙箱、安全执行、Observation
├── openwork-workspace/       # Project、Git、Snapshot、Diff、Revert、文件 Hash
├── openwork-persistence/     # Journal、Checkpoint、Projection、Artifact、Repository
├── openwork-memory/          # 后期：长期记忆提取、检索、冲突和纠错
├── openwork-observability/   # Trace、Metrics、成本和诊断
├── openwork-app/             # Application API、Supervisor、Composition Root
└── openwork-evals/           # Golden Case、回归任务、Grader、报告

apps/
└── desktop/                  # Tauri + React 宿主
```

这是一张目标责任图，不表示第一阶段必须一次创建所有 crate。抽取原则是：

- 有明确独立职责和依赖边界时才创建 crate。
- 只有一个消费者的早期能力先作为模块存在。
- Context 初期放在 `openwork-core/context`，稳定后再评估是否提取。
- Memory、Observability 在核心路径完成后再成为独立 crate。
- 不为了展示 Rust workspace 而拆出大量只有几十行的 crate。

## 7. 模块职责

### 7.1 `openwork-protocol`

负责共享语言与稳定合同，不执行 IO。

包含：

- 强类型 ID：`ProjectId`、`ThreadId`、`TurnId`、`ModelAttemptId`、`ActionRunId`、`ApprovalId`、`ArtifactId`。
- 状态：`TurnStatus`、`PlanStepStatus`、`ActionRunStatus`。
- 命令：`StartTurn`、`CancelTurn`、`ResolveApproval`、`ReconcileAction`、`SteerTurn`。
- 事件：`RecordedEventV1`、`LiveEventV1` 及版本化 Envelope。
- 模型合同：`ModelRequest`、`ModelEvent`、`ModelCapabilities`、`ModelPort`。
- Capability 合同：`CapabilitySpec`、`ActionRequest`、`Observation`。
- 外部端口：`EventJournal`、`CapabilityResolverPort`、`ActionInvoker`、`ExecutionPort`、`ArtifactStore`、`WorkspaceAccessPort`、`WorkspaceSnapshotPort`。

第一阶段可以保留单一 crate，但内部必须按稳定边界拆分：

```text
openwork-protocol/
├── domain/       # ID、状态和不可变值对象
├── events/       # Recorded/Live Event 与 Envelope
├── commands/     # Core 可接受的命令
├── ports/        # 外部依赖合同
├── model/        # 厂商无关模型合同
└── capability/   # Capability、Action 与 Observation 合同
```

`MemoryStatus`、Memory Port 等后置合同不进入 Protocol V1，等 Memory 专题设计完成后再版本化加入。

禁止依赖：

- SQLx
- Tauri
- React DTO
- 具体 Provider SDK
- 真实文件系统
- UI DTO、数据库 Row、Provider 原始 DTO

### 7.2 `openwork-core`

这是原先所说 Harness 的正式名称，是项目核心。

负责：

- `TurnActor`：每个活跃 Turn 的单一状态所有者。
- `TurnRunner`：推进长任务控制循环。
- `StateReducer`：从 Recorded Event 纯函数地重建状态。
- Planning：创建、更新和重新规划结构化计划。
- Context：预算、来源选择、摘要、压缩和检查点。
- Recovery：对模型失败、Action 失败和 no-progress 做分类恢复。
- Verification：根据测试、Diff 和成果规则判断完成。
- Budget：控制步骤、重试、Token、成本和时间预算。
- Loop Guard：检测重复调用、无进展和提前结束。

`openwork-core` 不直接：

- 调用某个具体模型厂商。
- 运行 Shell。
- 直接操作 PostgreSQL 或 SQLx。
- 构造 Tauri Payload。
- 直接访问用户文件。

它通过 `openwork-protocol` 中的 Port 完成这些操作。

人工审批的工作流只由 Core 持有：Execution 只返回 `Allow | Deny | RequireApproval`，Core 记录 `ApprovalRequested` 并暂停；App 只把用户操作转换成 `ResolveApproval` 命令。任何 Adapter 都不能一边等待 UI，一边隐藏审批状态。

### 7.3 `openwork-capabilities`

负责 Agent 能力的注册、发现、筛选和描述。

包含：

- Built-in Tool Catalog。
- MCP Tool 适配。
- MCP Resource 到 Context Source 的适配。
- MCP Prompt 到 Skill/Template 的适配。
- Skill Manifest、Skill Loader 和 Skill Resolver。
- 后续 Plugin Bundle。
- Tool Search 和按需加载。

边界：

- Tool 的声明和副作用提示属于 Capabilities；该提示不是最终权限结论。
- Core 通过 `CapabilityResolverPort` 获取候选能力，不依赖具体 Registry。
- Capabilities 提供 `CapabilitySpec`，并为可执行能力注册 `ActionInvoker`。
- Execution 根据策略校验请求后调用注入的 `ActionInvoker`，不反向依赖具体 Capabilities 实现。
- Skill 提供指令、工作流、资料和所需 Capability，不直接绕过 Harness 执行。
- MCP Server 的连接生命周期由 Capabilities 管理；MCP Tool 仍经过统一的 Execution Policy 和 Action 生命周期。

### 7.4 `openwork-providers`

只负责模型协议适配：

```text
ModelRequest -> vendor request -> vendor stream -> ModelEvent
```

允许：

- 认证 Header 和请求序列化。
- 流式事件拼接。
- 厂商错误归一化。
- 连接错误、429、部分 5xx 的 Transport Retry。
- Capability 探测。

禁止：

- 数据库存储。
- UI Preset。
- Tool 执行。
- 权限决策。
- 语义重新规划。
- 静默 Fallback。

### 7.5 `openwork-execution`

负责安全地改变外部世界，不负责决定任务下一步。

包含：

- Action 参数 Schema 校验。
- 根据来源、运行位置和实际参数计算最终风险。
- 权限策略返回 `Allow | Deny | RequireApproval`，不在 Execution 内等待 UI。
- 文件、搜索、Shell 等内置 Action Handler。
- macOS Sandbox Adapter（V1 唯一承诺和验证的平台）。
- 进程树生命周期、Timeout 和取消。
- 网络出口限制。
- 输出截断与 Artifact 化。
- 结构化 Observation。

#### macOS Sandbox V1 约束

OpenWork 当前范围只承诺实现和验证 macOS 沙箱。Linux、Windows 只保留协议可扩展性，不进入当前实现范围，也不为了形式上的跨平台抽象牺牲 macOS 路径的安全语义。

[Anthropic Sandbox Runtime](https://github.com/anthropic-experimental/sandbox-runtime) 是 macOS V1 的首选候选和重要参考实现；[Anthropic 的 Claude Code 沙箱工程说明](https://www.anthropic.com/engineering/claude-code-sandboxing) 作为威胁模型和设计动机的补充证据。该 Runtime 使用 macOS Seatbelt、文件系统规则、网络代理和违规监控提供 OS 级限制。OpenWork 可以选择：

1. 通过受控 CLI/Sidecar 边界直接复用；
2. 复用其策略和安全设计，在 Rust 中实现 `SandboxPort` Adapter；
3. 经评估后拒绝采用，并记录原因。

它是 TypeScript 项目，且官方标记为 Beta Research Preview，因此“直接使用”不等于让 `openwork-core` 或 `openwork-execution` 依赖其内部 API。任何采用方案都必须先在 `macos-sandbox-and-execution-design.md` 中确定，并至少完成：

- 固定版本或 commit，记录 Apache-2.0 许可证和第三方依赖。
- 明确 Rust 与 TypeScript/CLI 的进程边界、启动失败、退出、取消和升级策略。
- 建立 macOS 版本兼容测试、威胁模型和越权回归用例。
- 审计网络域名、Unix Socket、可写路径和 Apple Events 等逃逸或数据外发风险。
- 将违规、超时、拒绝和不确定结果映射为稳定 Observation/Event。
- 沙箱初始化、策略加载或监控失效时 **fail closed**，不得静默退化为宿主机无沙箱执行。

macOS V1 默认安全基线是：工作区外禁止写入、网络默认关闭、敏感目录禁止读取、凭据不进入沙箱、Unix Socket 默认禁止。域名 Allowlist 只限制连接目的地，不等于数据防泄漏保证。

Sandbox 和 Approval 是两个独立控制面：Sandbox 执行技术限制，Core 持有审批状态机。Sandbox Adapter 发现违规后只能返回结构化结果，不得自行请求审批、降低策略或改为无沙箱重试。

具体实现必须隐藏在 `openwork-execution` 内部的 `SandboxPort` 后；Core 只依赖稳定的 `ExecutionPort`、执行结果和风险事实，不感知 Sandbox Runtime 的配置格式或进程协议。

核心状态：

```text
requested -> waiting_approval -> started -> terminal
```

终态至少包括：

```text
completed | failed | denied | cancelled | outcome_unknown
```

风险分类至少区分：本地沙箱内执行、宿主进程执行、远程服务调用、文件系统写入、网络访问和数据外发。Capabilities 的自声明只作为输入之一，不能自行降低最终风险等级。

### 7.6 `openwork-workspace`

代表 Agent 正在操作的真实项目环境，而不是普通文件存储。

负责：

- Project 发现和 canonical path。
- Workspace 信任状态。
- Git 状态和 Repo Identity。
- Turn Baseline Snapshot。
- 文件 Hash 与 Workspace Generation。
- 文件变化和 Unified Diff。
- 整轮 Revert 与并发修改保护。
- Worktree（后期）。

Workspace 为 Context、Execution 和 Verification 提供真实环境事实：

- FileDigest 是否过期由当前内容 Hash 决定。
- Agent 是否越界由文件变化集合决定。
- Revert 是否安全由 before/after Hash 决定。
- Verification 的最终依据是当前文件和测试状态，不是模型回答。

Execution 只通过注入的 `WorkspaceAccessPort` 和 `WorkspaceSnapshotPort` 访问工作区，不直接操作 Workspace 的内部实现。Revert 也作为一类需要审批和记录的 Action：Workspace 负责计算可回滚性与前置条件，Execution 负责应用变更，Core 记录完整生命周期。

### 7.7 `openwork-persistence`

负责时间、顺序、原子性和恢复，不负责业务策略。

包含：

- PostgreSQL 事务、迁移与连接池。
- Append-only Event Journal。
- Aggregate Expected Version。
- Context Checkpoint。
- 幂等 Projector。
- Thread、Turn、Message、ActionRun、Approval、Artifact 查询投影。
- Content-addressed Artifact 存储。
- Provider 配置、Project、Settings 等普通 Repository。
- Provider 等普通 Repository 中敏感字段的认证加密；主密钥不进入数据库。
- 后期的 Summary 和 Memory Record。

Persistence 不决定：

- 是否重试。
- 是否审批。
- 是否压缩。
- 是否完成。

这些决策属于 `openwork-core` 或 `openwork-execution`。

### 7.8 `openwork-memory`

Memory 是跨 Turn/Thread 的长期事实与偏好系统，后置实现。

负责：

- 从事件和 Artifact 提取候选记忆。
- 记忆范围、来源、置信度和有效期。
- 检索候选记忆。
- 与当前 Workspace 事实校验。
- 冲突、纠错、`supersedes` 和隔离。

Memory Record 存在 Persistence；提取、检索和纠错逻辑属于 Memory。

### 7.9 `openwork-observability`

负责运行可观测性，不承担恢复事实。

包含：

- Rust `tracing` Span。
- OpenTelemetry Export（后期）。
- Token、成本和延迟。
- Queue Wait、Action Duration、Retry Count。
- Context Size、Compaction Ratio。
- Sandbox 启动和违规记录。
- Trace 查询模型和 UI 投影。

### 7.10 `openwork-app`

宿主无关的 Application API 与 Composition Root。

负责：

- 创建 Project、Thread 和 Turn。
- 启动、暂停、取消、恢复和 Steering。
- 路由 Approval 与 Reconciliation。
- 管理活跃 `TurnActor`。
- 组合 Core、Provider、Capabilities、Execution、Persistence、Workspace。
- 向 UI 提供 Command、Query 和 Subscription。

桌面端最终只依赖 `openwork-app` 和必要的 `openwork-protocol` DTO，不直接访问 Store、Provider 或 ToolRegistry。

### 7.11 `openwork-evals`

负责验证 Harness + Model 的真实完成能力。

包含：

- Scripted/Fake Model。
- Fake Execution Port。
- Golden Repo Fixture。
- 崩溃、审批、重试、越界和恢复场景。
- 最终环境 Grader。
- pass@1、pass@3、恢复正确率、终态完整率和成本报告。

## 8. 功能归属总表

| 功能 | 主要实现位置 | 相关持久化/适配 |
| --- | --- | --- |
| Plan Mode | `openwork-core/planning` | Plan Artifact + Recorded Event |
| Tool 定义与发现 | `openwork-capabilities/tool` | Capability Catalog |
| Tool 实际执行 | `openwork-execution` | ActionRun Event + Artifact |
| Tool 失败恢复 | `openwork-core/recovery` | Action terminal event |
| Provider HTTP 重试 | `openwork-providers` | ModelAttempt trace/event |
| 语义重新规划 | `openwork-core/planning` | Plan version/event |
| 多轮上下文治理 | `openwork-core/context` | Context source-use record |
| 上下文压缩 | `openwork-core/context/compaction` | ContextCheckpoint |
| Task/Turn Summary | `openwork-core/context` | Persistence |
| File Digest | Context 生成，Workspace 提供 Hash | Persistence |
| 大型 Tool Output | Execution 生成 Artifact | ArtifactStore |
| 长期 Memory | `openwork-memory` | Persistence |
| Sandbox | `openwork-execution/sandbox` | violation trace/event |
| Permission Policy | `openwork-execution/policy` | 返回 Allow/Deny/RequireApproval |
| Human Approval | Core 持有工作流，App 路由用户决定 | approval event/projection |
| Recorded Trace | Core 产生，Persistence 保存 | Journal |
| Telemetry Trace | `openwork-observability` | tracing/OTel |
| Agent Eval | `openwork-evals` | eval report |
| Skill | `openwork-capabilities/skill` | Context Builder 注入 |
| MCP Tool | `openwork-capabilities/mcp` | ExecutionPort 调用 |
| MCP Resource | `openwork-capabilities/mcp` | Context Source |
| Plugin | Capabilities Bundle | Skill + MCP + assets |

## 9. Tool 完整调用链

```text
CapabilityCatalog 提供候选 Tool
  -> Context Builder 选择本轮相关 Tool Schema
  -> ModelAttempt 返回 provider tool call
  -> Core 创建内部 ActionRunId
  -> 参数 Schema 校验
  -> persist ActionRequested
  -> Execution 根据来源、运行位置和参数计算最终风险
  -> Execution Policy 返回 Allow / Deny / RequireApproval
  -> RequireApproval 时由 Core persist ApprovalRequested 并暂停
  -> persist ActionStarted
  -> Sandbox 中执行 Handler
  -> Observation Normalizer
  -> 大输出写入 ArtifactStore
  -> persist completed / failed / cancelled / outcome_unknown
  -> Core Recovery Policy 决定继续、重试、重规划或停止
  -> Observation 进入下一次 ContextPackage
```

Tool 详细 Schema、Observation、风险和重试协议将在后续专题文档中确定。

## 10. 三层失败恢复

### 10.1 Provider Transport Retry

位置：`openwork-providers`

只处理连接中断、429、部分 5xx 和流建立前失败，不理解任务语义。

### 10.2 Action Retry

位置：`openwork-core/recovery`

依据：

- Action 幂等性。
- 是否已经开始产生副作用。
- Idempotency Key。
- Retry Budget。
- 错误分类。

### 10.3 Semantic Recovery

位置：`openwork-core/recovery` + `openwork-core/planning`

处理：

- 调用成功但任务没有进展。
- 连续读取错误位置。
- 测试仍失败。
- 计划假设已失效。
- Workspace Generation 变化。

语义恢复通常应重新规划，而不是机械重复相同工具调用。

## 11. Plan、Context、Summary 与 Memory 的关系

### 11.1 Plan

Plan 是 Turn 内的可版本化执行意图，属于 Core。

```text
draft -> awaiting_approval(optional) -> active -> revised -> completed/abandoned
```

Plan 不是事实源；Action Outcome、Workspace 和 Verification 才是事实。

### 11.2 Context

Context Builder 每次 ModelAttempt 重新构造 `ContextPackage`：

```text
Invariant Instructions
Task Goal and Constraints
Active Plan
Unresolved Failures and Approvals
Recent Event Tail
Relevant File Digests
Validated Memory Candidates
Selected Capability Specs
Budget Metadata
```

### 11.3 Summary

- `TaskSummary`：当前长任务目标、进展和阻塞。
- `TurnSummary`：某段事件范围的阶段总结。
- `FileDigest`：与内容 Hash 绑定的文件事实摘要。
- `WorkingNotes`：可丢弃的临时推理和待办。

Summary 是派生信息，原始事件和 Workspace 事实仍保留。

### 11.4 Memory

Memory 是跨任务候选事实。优先级低于当前 Workspace 和当前 Turn 事件，使用前必须检查 Scope、来源和是否过期。

这些子系统的详细数据结构将在核心事件和恢复语义冻结后讨论。

## 12. Rust 设计原则

1. 使用强类型 ID，内部 `ActionRunId` 与 Provider Call ID 分离。
2. 使用枚举表达状态，不使用多个互相矛盾的布尔字段。
3. 使用纯 Reducer：`state + recorded event -> new state`。
4. 每个活跃 Turn 由单个 Actor 串行拥有状态。
5. 使用有界 `mpsc` 接收命令，使用 `broadcast` 发布可丢失 Live Event。
6. 审批决定必须作为 `ResolveApproval` 命令回到 Turn Actor，不能由 Execution 持有等待 UI 的 `oneshot`。
7. 使用 `CancellationToken` 和受管理子任务，避免无法追踪的 detached task。
8. Trait 只放在真正的外部边界；Composition Root 使用 `Arc<dyn Port>`。
9. 事件和持久化协议显式版本化。
10. 不为通用性提前实现 ORM、Plugin Framework 或网络 Server。
11. 不把 UI DTO、Provider DTO 和 Recorded Event 复用为同一类型。
12. 先实现单 Agent 的可靠性，再用 Eval 证明是否需要多 Agent。

## 13. 依赖方向

```text
apps/desktop
    -> openwork-app
         -> openwork-core
              -> openwork-protocol
         -> openwork-capabilities
              -> openwork-protocol
         -> openwork-providers
              -> openwork-protocol
         -> openwork-execution
              -> openwork-protocol
         -> openwork-workspace
              -> openwork-protocol
         -> openwork-persistence
              -> openwork-protocol
         -> openwork-memory (post-core)
              -> openwork-protocol
         -> openwork-observability (post-core)

openwork-evals
    -> openwork-app / openwork-core / openwork-protocol
```

硬约束：

- `openwork-core` 不依赖任何具体 Adapter。
- Provider、Capabilities、Execution、Persistence、Workspace 相互之间不通过具体类型耦合。
- Core 通过 `CapabilityResolverPort` 发现能力；Execution 通过注入的 `ActionInvoker` 调用 Handler，不能反向依赖 Capabilities Registry。
- Execution 通过 Workspace Port 访问项目事实，不能绕过 `openwork-workspace` 直接写用户文件。
- `openwork-app` 是唯一 Composition Root。
- Desktop 不绕过 App 访问内部 Store 或 Registry。

## 14. 当前 crate 到目标结构的映射

| 当前 crate | 目标去向 |
| --- | --- |
| `openwork-protocol` | 保留并扩展为稳定协议与 Port 合同 |
| `openwork-agent` | 核心循环迁入 `openwork-core` |
| `openwork-runtime` | Turn 控制逻辑迁入 Core；应用组合迁入 App |
| `openwork-capabilities` | 已承接 Tool 声明与 Catalog；后续增加 Skill、MCP 和按需发现 |
| `openwork-execution` | 已承接内置 Handler、参数校验和 Observation；后续增加最终风险策略与 macOS Sandbox |
| `openwork-permissions` | 权限策略迁入 Execution；审批状态机迁入 Core，用户决定路由迁入 App |
| `openwork-providers` | 保留；移除数据库和 UI 职责 |
| `openwork-session` | 迁入 Persistence 的 Repository/Projection |
| `openwork-database` | 由 Persistence 内部基础设施取代 |
| `openwork-db-macros` | 暂停扩展，评估删除；不作为 Agent 架构主线 |
| `openwork-workspace` | 保留并明确真实环境事实边界 |
| Tauri commands | 收口到 `openwork-app` Command/Query/Subscription |

不进行一次性目录大迁移。每次迁移必须由 Golden Case 和合同测试保护。

## 15. 参考项目的使用规则

| 来源 | 只参考什么 | 解决的 OpenWork 问题 | 不直接照搬 |
| --- | --- | --- | --- |
| [OpenAI Codex](https://github.com/openai/codex) | Thread/Turn/Item、结构化生命周期、macOS Seatbelt、权限与沙箱分离 | 领域协议和安全边界 | 全部实验 API 和复杂配置 |
| OpenCode | Event/Projector、Snapshot/Revert、Project/Worktree | 查询投影和工作区恢复 | Electron/TS Monorepo |
| Kimi Code | Recorded/Live 分离、seq/ack/resync | 事实恢复和实时 UI 分离 | 第二宿主出现前的网络 Server |
| [Claude Code](https://code.claude.com/docs/en/sandboxing) | Context、Skill、Hook、Sandbox 和长任务经验 | Context 与安全设计 | 无评测依据的固定多 Agent |
| cc-haha | 公开 UX、审批、变更和终端面板 | 面试 Demo 展示 | 源码、资源和专有协议 |
| [Anthropic Sandbox Runtime](https://github.com/anthropic-experimental/sandbox-runtime) | macOS Seatbelt、文件/网络限制、违规监控 | macOS Sandbox V1 候选实现 | 未经威胁建模、兼容测试和 Adapter 隔离直接绑定实验 API |
| [LangChain Deep Agents Sandbox](https://docs.langchain.com/oss/python/deepagents/sandboxes) / [Open SWE](https://github.com/langchain-ai/open-swe) | Sandbox Backend Protocol、Session/Snapshot 生命周期和凭证隔离 | Sandbox Port、生命周期与远程执行边界 | 将 Python 或远程 Sandbox Provider 直接套到本地 Rust/macOS 执行链路 |

以后每个借鉴必须回答：

1. OpenWork 当前具体失败模式是什么？
2. 参考设计保护哪个不变量？
3. 为什么适合 Rust + Tauri + 本地优先？
4. 最小实现是什么？
5. 用什么测试证明它有效？

答不出这五个问题，就不引入。

### 15.1 方案调研和采用规则

复杂基础设施进入设计前，应先检索 Anthropic、OpenAI、LangChain/LangGraph 等官方文档、官方博客和官方 GitHub 仓库，再补充活跃且高质量的开源实现。官方一手资料优先于二手解读，但“官方”不代表可以不经评估直接复制。

每份专题设计必须记录：

1. 需要解决的真实失败模式和已有代码证据。
2. 候选方案的官方链接、精确版本或 commit、许可证和维护状态。
3. 安全模型、已知限制、依赖/runtime 成本，以及与 Rust + Tauri + 本地优先的适配度。
4. `Adopt | Extend | Build` 结论：直接采用、包装扩展或自行实现。
5. 采用和拒绝各候选方案的原因、验证方法、回滚条件和 Decision Log。

研究结论只为设计决策提供证据，不能越过 Protocol、Core、Execution、Persistence 的所有权边界。

## 16. 参考建设路线（S0-S8）

S0-S8 是帮助理解依赖关系和风险收敛方式的 **参考路线**，不是固定里程碑、承诺排期或必须全部采用的功能清单。实际项目计划可以根据当前代码状态、依赖前置条件、Golden Case/Eval 证据、真实失败模式和面试展示目标，对阶段进行合并、拆分、重排、暂停或跳过。

使用规则：

- 实际执行以单独维护的当前项目计划和对应专题设计为准。
- 优先选择能够持续保持 Golden Case 可运行的纵向切片，而不是一次横向铺完所有基础设施。
- 调整或跳过某个阶段时，必须说明原问题由什么方案解决，并记录依赖、风险、退出条件和回滚方式。
- 路线可以调整，但本文冻结的模块所有权、依赖硬约束和四项可靠性保证不能被专题设计静默覆盖。
- 只有核心方向、所有权或关键不变量发生变化时才修改本蓝图；普通实现选择记录在专题设计和 Decision Log 中。

### S0：Golden Case 与目标 Trace

上下文：当前已有模型—工具循环，但架构迁移缺少一条固定验收路径。

任务：

- 创建最小 `openwork-evals` 骨架，并从此阶段开始持续扩充，而不是最后补测试。
- 建立 Fixture Repo 和 Scripted Model。
- 固定 read/edit/bash/test/diff/revert 当前行为。
- 定义正常、拒绝、失败、取消、崩溃和恢复的目标事件轨迹。

验证：

```bash
cargo test --workspace
```

退出条件：在不依赖真实模型的情况下，可重复验证目标任务和事件序列。

回滚：只增加测试和 Fixture，不改变生产链路。

### S1a：建立 Protocol Foundation

上下文：先统一不会因 Plan、Tool、Retry 细节变化而推翻的最小语言，避免过早冻结错误合同。

任务：

- 强类型 ID。
- 版本化 Envelope。
- 最小 Turn/Action 状态。
- Recorded Event 与 Live Event 的分界。
- Model、Journal、Artifact、Workspace 等基础 Port。
- 序列化和状态迁移测试。

退出条件：基础协议不引用 Tauri、SQLx、具体 Provider 或后置 Memory 类型。

回滚：与现有类型并存，先增加兼容 Adapter。

### Design Gate A：冻结关键运行语义

在冻结 Runtime Contract V1 前，先完成以下专题的最小设计：

- Plan 与 Task 状态。
- Capability、Action 与 Observation。
- Retry、Recovery 与 Reconciliation。
- Permission、Approval 与暂停恢复。

退出条件：每个专题至少明确状态机、核心类型、Recorded Event、失败路径和合同测试。

### S1b：冻结 Runtime Contract V1

任务：

- `StartTurn`、`CancelTurn`、`ResolveApproval`、`ReconcileAction`、`SteerTurn`。
- 完整 Turn/Action/Approval 状态机。
- `RecordedEventV1`、`LiveEventV1`。
- `CapabilityResolverPort`、`ActionInvoker`、`ExecutionPort`、Workspace Port。
- 序列化、Reducer 合同和兼容性测试。

退出条件：V1 合同足以支持最小 Durable Turn，且无需依赖 UI、数据库 Row 或具体 Adapter DTO。

### S2：建立本地 Persistence

上下文：恢复能力需要有序、原子、可回放的事实日志。

任务：

- PostgreSQL Event Journal、事务和迁移。
- Event Journal 与 Expected Version。
- Turn/Action/Approval 最小 Projector。
- Artifact metadata 和内容寻址目录。

退出条件：Synthetic Event 可从零 Replay 并重建相同投影。

回滚：保留旧 Session Store，未达到 Replay Parity 前不删除旧表。

### S3：Capabilities、Execution 与 App Shell

上下文：Tool 声明与真实 Handler 已完成第一阶段拆分，但权限策略、最终风险计算、审批暂停恢复和真实 Sandbox 仍处于过渡状态。

任务：

- 最小 Capability Catalog 与 `CapabilityResolverPort`。
- `ActionInvoker` 注册和注入合同。
- 内部 ActionRunId。
- Schema 校验和结构化 Observation。
- Permission Policy；只返回 `Allow | Deny | RequireApproval`。
- Sandbox Port 和 macOS V1 实现路径；优先评估 Anthropic Sandbox Runtime。
- 沙箱不可用时 fail closed；Linux/Windows Adapter 不在当前阶段范围内。
- 建立最小 `openwork-app` Composition Root 与 Turn Supervisor 壳层。

退出条件：越权文件和网络测试被确定性拒绝，并记录完整事件。

回滚：先包装现有 Built-in Tool，不立即实现 MCP/Skill。

### S4：建立 `openwork-core` Durable Turn

上下文：将当前 callback 驱动 Agent Loop 转为单所有者 Turn Actor。

任务：

- TurnActor、TurnRunner 和纯 Reducer。
- `MinimalContextBuilder`：只组装任务、近期事件、当前 Action 结果和候选 Capability。
- Persist Intent -> Execute -> Persist Outcome。
- Core 持有审批暂停、取消和恢复流程。
- `outcome_unknown` 与 reconciliation。
- 接入 S2 Journal 和 S3 的 Fake/Real Port。

退出条件：崩溃恢复不会自动重复副作用 Action，Execution 不会自行等待 UI 审批。

回滚：保留旧 Agent 路径，通过 Feature Flag 切换。

### S5：Workspace、Verification 与 Revert

上下文：长任务完成必须由真实工作区结果证明。

任务：

- Baseline、Workspace Generation、File Hash。
- Diff 与越界检测。
- Verification Policy 和有限 Repair Budget。
- 整轮 Revert。

退出条件：Golden Case 可完成修改、验证、展示 Diff 并安全撤销。

### S6：Context 与 Artifact

上下文：多轮长任务需要预算、摘要、Artifact 化和可验证压缩。

任务：

- ContextPackage 和 Context Source 记录。
- Token/Cost Budget。
- 大型 Tool Output Artifact 化。
- TaskSummary、TurnSummary、FileDigest。
- ContextCheckpoint 和 Compaction Validator。

退出条件：长任务跨压缩后仍保持目标、硬约束、失败测试和文件版本。

### S7：Application API 扩展与 Desktop 收口

上下文：桌面端当前直接依赖多个内部 Store 和 Registry。

任务：

- Command、Query、Subscription。
- 扩展 S3 建立的 Turn Supervisor。
- Approval、Cancel、Resume、Reconcile 路由。
- UI 只依赖 App/Protocol。

退出条件：Desktop 不直接访问 PostgreSQL Repository、SessionStore 或 ToolRegistry。

### S8：Observability 与面试 Demo

上下文：架构价值必须通过可重复结果展示。

任务：

- 扩展从 S0 持续维护的 Golden Repo 回归集与报告。
- Trace、Token、成本和恢复指标。
- 权限攻击和提示注入 Fixture。
- 崩溃恢复演示。
- 代码修改 + 测试 + 技术报告/PPT + Diff + Revert 综合 Demo。

退出条件：面试演示不依赖碰运气的真实模型输出，关键路径有确定性证据。

### Post-core：Skill、MCP、Memory 与多 Agent

“Post-core”表示默认优先级，不是必须等 S0-S8 全部完成的时间门槛。实际计划只有在 Durable Core、Context 和安全边界能够支撑对应功能，且有明确产品或 Eval 证据时，才选择性引入：

- Skill Manifest、加载和 Context 注入。
- MCP 生命周期、Tool/Resource/Prompt 适配。
- Memory 提取、检索、冲突和纠错。
- Planner/Generator/Evaluator 多 Agent；必须由 Eval 证明收益超过成本。

## 17. 参考依赖顺序

下图表达技术前置关系，不代表实际项目必须保留相同阶段编号或按完全相同顺序推进。

```text
S0 Golden Case
  -> S1a Protocol Foundation
  -> Design Gate A
  -> S1b Runtime Contract V1
       -> S2 Persistence
       -> S3 Capabilities/Execution/App Shell
  -> S2 + S3 -> S4 Durable Core
  -> S4 -> S5 Workspace/Verification
  -> S4 + S5 -> S6 Context/Artifact
  -> S4 + S5 + S6 -> S7 App/Desktop
  -> S5 + S6 + S7 -> S8 Observability/Demo
  -> Post-core Extensions
```

可并行窗口：

- S2 与 S3 在 S1b 后可并行。
- S5 的 Workspace 测试可在 S4 后端接线前用 Fixture 开发。
- Eval 骨架从 S0 建立并贯穿每个阶段；S8 只完成观测面板和综合演示。

## 18. 明确不做

核心路径完成前不做：

- 通用多 Agent 编排平台。
- 大型 Agent Marketplace。
- 完整 Office 编辑器。
- 手机远程控制和 IM 控制面。
- 云端多租户调度。
- 自研 ORM 或数据库框架。
- 在第二个真实宿主出现前增加 REST/WebSocket Server。
- 没有失效、来源和评测的长期 Memory。
- 只用于视觉展示但没有可靠执行语义的 Plan/Trace 动画。
- Linux 和 Windows Sandbox Adapter；除非后续项目目标明确要求并重新立项。

## 19. 后续专题文档

本蓝图只负责方向、边界和不变量。重要功能进入实现前必须先写对应的详细设计；顺序由实际项目计划决定，下面是当前已识别的专题，不是固定排期或封闭清单：

1. `plan-and-task-state-design.md`
2. `capability-tool-observation-design.md`
3. `retry-recovery-and-reconciliation-design.md`
4. `context-summary-and-compaction-design.md`
5. `skill-and-mcp-lifecycle-design.md`
6. `memory-provenance-and-correction-design.md`
7. `permission-and-approval-design.md`
8. `macos-sandbox-and-execution-design.md`
9. `trace-observability-and-eval-design.md`

Provider、Persistence、Workspace 等已进入实现的领域也遵守相同规则；已有设计文档应随实现证据校正，不能用蓝图中的一句描述替代详细设计。

每份专题文档至少包含：

1. 问题、代码/Eval 证据、范围和不做事项。
2. 模块所有权、依赖方向，以及与现有协议的关系。
3. 状态机、核心类型、Port、Command/Event 和时序。
4. 数据表、迁移、幂等和恢复策略（如涉及持久化）。
5. 失败路径、Retry/Reconciliation、权限和安全边界。
6. Trace/Metric、测试、Golden Case 和验收标准。
7. 发布、兼容、回滚方案，候选来源和 Decision Log。

专题设计可以细化蓝图未冻结的实现，但不能静默改变蓝图中的所有权、依赖硬约束或持久化语义。若二者冲突，先按第 20 节修改蓝图。小型缺陷修复和不改变行为的机械迁移不强制编写完整专题文档，但仍必须有相称的测试和变更说明。

## 20. 蓝图变更协议

后续如果修改本蓝图：

1. 先说明触发修改的代码证据、Eval 结果或真实产品需求。
2. 标记受影响的模块、事件和施工步骤。
3. 不静默重定义已经持久化的事件语义；使用新版本或迁移。
4. 新增 crate 前说明为什么模块边界不足。
5. 删除步骤前说明原问题是否已由其他步骤解决。
6. 保留 Decision Log，记录接受和拒绝的重要方案。

## 21. 待讨论决策

以下内容本蓝图有归属但尚未冻结实现：

- Plan Mode 的触发策略、审批和计划修订协议。
- CapabilitySpec、Observation 和 Action Error Schema。
- Provider/Action/Semantic 三层 Retry Matrix。
- Context Budget 算法和 Compaction Validator。
- TaskSummary、TurnSummary、FileDigest 的生成与校验。
- Skill Manifest、脚本权限和按需加载。
- MCP Server 生命周期、认证和 Tool/Resource/Prompt 适配。
- Memory 的提取时机、Scope、置信度和纠错关系。
- macOS Sandbox V1 的 Adapter 边界、Anthropic Sandbox Runtime 采用方式和权限策略映射；Linux/Windows 暂不实现。
- Recorded Event、Telemetry Trace 和 Eval Trace 的关联方式。

其中 Plan、Capability、Retry 和 Approval 的最小语义应在 Protocol Foundation 之后、Runtime Contract V1 之前确定；Context、Skill、MCP、Memory 等完整实现继续按阶段专题讨论，避免再次通过参考项目拼装出互相冲突的实现。
