# OpenWork 重构实施路线

> 状态：Phase 1-7 的代码迁移已完成；Provider 凭证和模型已回填到 Core V2 Storage，旧数据库表已归档为 `legacy_*`。最终 DROP 仍遵循独立 Migration 和保留周期。
>
> 原则：每一阶段都必须可编译、可测试、可回退；先建立新的唯一运行链，再删除旧链。

## 1. 目标与顺序

当前实施结果：`SessionActor -> Model -> Tool/Permission -> Model` 已成为唯一执行链，Desktop 已切换 Runtime Command/Update/Snapshot/Trace API；新数据和 Provider 加密凭证均写入 Core V2 Storage，`openwork-protocol`、`openwork-persistence` 已删除。

这次重构同时涉及：

- crate 所有权；
- Model/Tool 类型移动；
- Session Runtime；
- PostgreSQL Schema；
- Trace；
- Desktop API；
- Legacy Journal/Recovery 删除。

不能一次性重命名全部目录。实施顺序必须遵循：

```text
行为基线
  -> 叶子能力边界
  -> SessionActor + 唯一 Agent Loop
  -> 直接持久化
  -> 简化 Trace
  -> Desktop 切换
  -> 删除旧 Runtime/Journal/Recovery/crate
```

运行链优先于数据库和 Trace。只要 Tool Result → 下一次 Model Call 还没有由新 Session Runtime 完整拥有，就不开始恢复能力。

## 2. 全局实施规则

1. 不在同一阶段同时改变运行语义、数据库格式和 Desktop 协议。
2. 类型先移动，保留临时 re-export；消费者切完后再删除兼容层。
3. 新旧 Runtime 不得同时接收同一个 Turn。
4. 新旧 Storage 不得同时无条件执行同一个工具副作用。
5. Trace 永远是旁路，不能成为 Cutover Gate 的唯一证据。
6. 有现存数据时只新增/回填，不自动 DROP 旧表。
7. 每阶段提交前运行目标 crate 测试和依赖方向检查。
8. 发现行为不一致时先补 Characterization Test，再修改实现。

## 3. Phase 0：冻结当前行为

### 目标

在移动代码前记录当前真正可用的行为，避免结构重构悄悄改变模型/工具语义。

### 工作

- 为当前 `openwork-core::Agent` 补齐黑盒测试；
- 为 `OpenWorkCore` 与 Tauri Command 边界建立最小集成 Harness；
- 固定 Provider 流事件到最终 Message 的转换；
- 固定当前 Tool Input/Observation 序列化；
- 固定 Session 列表、加载和 Message 顺序；
- 记录当前数据库表、Migration 版本和数据量检查 SQL；
- 记录当前 Desktop 调用的 Tauri Command/DTO。

### 必须覆盖

```text
无工具完成
单工具后继续模型
一次响应多个工具
工具错误后继续模型
Unknown Tool
Invalid Tool Input
Permission Allow/Deny
取消模型流
取消工具等待
Doom Loop
Max Steps
Trace Recorder 失败
Journal 写入失败时不继续副作用
```

### Gate

```sh
cargo test -p openwork-core
cargo test -p openwork-models
cargo test -p openwork-tools
cargo test -p openwork-desktop
```

测试失败但确认是已有问题时，测试应使用明确名称和注释隔离；不能把失败行为当作目标契约。

## 4. Phase 1：建立叶子能力 crate

### 4.1 openwork-models

移动：

```text
openwork-protocol/src/model/*
openwork-providers/src/adapters/*
openwork-providers/src/transport/*
```

目标：

- `Message/ContentBlock`；
- `ModelRequest/ModelResponse/ModelEvent`；
- `Model` trait；
- Provider Adapter；
- Transport Error/Retry；
- Model Profile 的运行时类型。

临时兼容：`openwork-protocol::model` 可以 re-export `openwork_models::*`，但禁止新增类型。

### 4.2 openwork-tools

合并：

```text
openwork-protocol/src/capability/*
openwork-capabilities/*
openwork-execution/*
```

形成：

```text
ToolDefinition
ToolCatalog
ToolContext
ToolInvocation
ToolResult
ToolExecutor
```

同时把当前 `openwork-execution` 中真正需要的执行能力合入：

- `working_directory`、Permission Profile、Cancellation Token；
- 安全路径解析与读写根边界；
- 文件读取、写入、编辑、搜索；
- 进程/终端执行；
- Tool 风险信息与执行期强制策略。

Tool Executor 直接使用 `ToolContext`，不能依赖 Core。Core 负责 `Allow/Ask/Deny` 编排和等待用户；Tools 负责不可绕过的路径/进程安全检查。

### 4.3 删除 openwork-workspace

当前 `openwork-workspace` 只提供 Git Worktree Snapshot、差异计算和回滚。V1 不做 Git Status/Diff、Snapshot/Revert、Workspace Trust、Fast Worktree、Checkpoint Pool 或 Workspace Daemon，因此：

- 不建立目标 `openwork-workspace` crate；
- 从 Cargo Workspace 和 workspace dependencies 删除当前 crate；
- 删除只服务上述能力的类型、测试和依赖；
- 不把 Git 能力迁移进 `openwork-tools`。

### Gate

- `openwork-models` 不依赖任何上层 crate；
- `openwork-tools` 只依赖 Models 和第三方 OS/进程库，不依赖 Core；
- 目标代码和 Cargo Manifest 不再引用 `openwork-workspace`；
- Provider 与工具行为测试不变；
- 当前 App/Core 可通过兼容 re-export 继续编译。

## 5. Phase 2：拆出 Agent 与 Chat State

### 5.1 openwork-agent

从当前 `AgentConfig` 和 `agent.rs` 中移动：

- System Prompt；
- Agent 名称/说明；
- Tool Set；
- 最大 Model Call 数；
- Permission Mode；
- Doom-loop Policy。

不要移动：

- `run/run_steps/stream_once`；
- Cancellation Token；
- Recorder；
- Trace Context；
- Approval Command Inbox；
- Session/Turn ID。

目标 `Agent` 构建后近似不可变。

### 5.2 openwork-chat-state

建立 Actor/Command：

```text
AppendUser
AppendAssistant
AppendToolResult
BeginDraft
ApplyModelDelta
FinishDraft
DiscardDraft
BuildRequest
Snapshot
```

先使用内存实现，并通过测试证明：

- 顺序稳定；
- 不暴露可变 Message Vec；
- Tool Call/Tool Result 配对；
- 流中断不会产生半条持久 Message；
- Request Snapshot 与当前 Conversation 一致。

### Gate

```sh
cargo test -p openwork-agent
cargo test -p openwork-chat-state
```

## 6. Phase 3：建立新的 Session Runtime

这是整个重构的核心阶段。

### 6.1 先在 openwork-core 建立模块

```text
core/session/actor.rs
core/session/handle.rs
core/session/commands.rs
core/session/run_loop.rs
core/session/model_calls.rs
core/session/tool_calls.rs
core/session/updates.rs
```

把当前：

```text
openwork-app::ChatRuntime
openwork-app::TurnSupervisor
openwork-core::Agent
```

中的运行职责合并进一个 `SessionActor`。

### 6.2 先接 Legacy Adapter

此阶段先通过内部 Adapter 继续使用当前 Provider、SessionStore 和 Trace No-op，避免同时切数据库：

```text
LegacyModelAdapter
LegacySessionHistoryAdapter
Noop/NewTraceAdapter
```

Adapter 只用于迁移，不允许继续暴露 Step/Recovery 到新 Runtime API。

### 6.3 实现唯一循环

验收主链：

```text
Turn
  -> User Message
  -> Model Call
  -> Assistant Message
  -> Tool Call(s)
  -> Tool Result Message(s)
  -> next Model Call
  -> Final Assistant Message
  -> TurnOutcome
```

删除新路径中的：

```text
StepId
TurnRecorderPort
ApprovalRecovery
JournalTurnRecorder
TracingTurnRecorder
```

旧 Runtime 暂时保留，但只由 Feature Flag/测试入口调用。生产入口必须二选一。

### 6.4 Live Update

SessionActor 分配 `SessionUpdate.sequence`，建立：

- Broadcast；
- 有界 Ring Buffer；
- `SessionSnapshot`；
- Desktop 断开不影响 Turn 的测试。

不创建数据库 Updates 表。

### Gate

- 新 Runtime 通过 [02-event-update-model.md](02-event-update-model.md) 的 12 个最小行为测试；
- Tool Result 后发生下一次 Model Call 的测试直接断言请求内容；
- 同一 Session 不会并行推进两个 Turn；
- Trace 关闭时行为不变；
- 新路径不读取 `recorded_events` 决定下一状态。

## 7. Phase 4：切换直接持久化

### 7.1 建立 V2 表

按 [03-database-schema.md](03-database-schema.md) 创建版本化物理表：

```text
models_v2
sessions_v2
turns_v2
messages_v2
trace_spans_v2
```

`schema_migrations` 沿用现有基础设施，但 Migration 内容不可覆盖旧版本。

### 7.2 Core Storage API

只暴露主循环所需操作：

```rust
create_turn_with_user_message(...)
append_assistant_message(...)
append_tool_result(...)
finish_turn(...)
load_conversation(...)
mark_running_turns_interrupted(...)
```

避免重新造通用 Repository/UnitOfWork 层。事务由上述用例方法拥有。

### 7.3 写入顺序

必须测试：

- Turn/User Message 失败时不调用模型；
- Assistant Tool Call Message 失败时不执行工具；
- Tool Result Message 失败时不进行下一次 Model Call；
- Turn 完成更新失败时返回 Persistence Error；
- Trace 写失败不回滚业务写入。

### 7.4 数据回填

从旧 Journal 只读回放到 V2，生成核对报告：

```text
session_count
turn_count
message_count_by_role
tool_call/tool_result_pair_count
orphan_message_count
non_terminal_turn_count
trace_span_count_by_kind
```

无法可靠映射的数据记录到 Migration Report，不凭猜测生成恢复状态。

### Gate

- 新旧 Session 列表数量和关键元数据一致；
- 每个 Session 的 Message 顺序抽样/全量校验；
- Tool Result Provider Call ID 无重复；
- 启动修正只改状态，不调度任务；
- V2 写路径稳定后才能切读路径。

## 8. Phase 5：简化 Trace

### 工作

- 在 `openwork-core::trace` 实现有界 Recorder；
- Runtime 直接产生 ModelCall/ToolCall Signal；
- 使用 RAII Guard 结束 Span；
- Permission Wait 聚合到 Tool Span；
- Transport Retry 聚合到 Model Span；
- 实现 Trace Completeness；
- 将 Trace List Root 改为查询 `turns`；
- 移除 `TracingTurnRecorder` 和 Step/Approval/Recovery Span 生成。

### Gate

- [04-trace-design.md](04-trace-design.md) 的 Trace 测试通过；
- Queue 满和数据库不可用测试证明 Turn 结果不变；
- Trace Payload 脱敏测试通过；
- Current Turn Count 与 Trace Completeness 能显式显示缺口。

## 9. Phase 6：Desktop 切换

Desktop 的 Tauri State 只持有：

```rust
Arc<OpenWorkCore>
```

迁移按 [06-frontend-architecture.md](06-frontend-architecture.md) 的 F0-F6 执行，不能只在旧 DTO 外改名。Tauri Bridge 先定义 Rust Host Contract，再由它生成 TypeScript 类型。

Tauri Command 改为短生命周期调用：

```text
create/list/load session
turn_start（Core 接受后立即返回 TurnAccepted）
turn_cancel
resolve_permission
session_snapshot
list/get trace
```

进程启动时只建立一个 `Core Event -> openwork://session-update` Bridge。React 侧建立：

```text
canonical sessionStore
per-session runtimeStore
pure reduceSessionUpdate
derived Transcript View Model
```

删除 Desktop 对以下对象的直接认知：

```text
ProviderFactory
AgentConfig
TurnSupervisor
SessionStore
TraceRuntime
JournalTurnRecorder
StepId/ToolRunId/ApprovalId
```

同时删除：

```text
长时间等待的 chat_generate_stream
chat-stream-event
全局 activeStream
approvalStore
手写的 Turn/Step/ToolRun/Recovery TypeScript DTO
Trace raw input/output 页面
```

兼容字段只能暂存在 `src/bridge/compat.ts`，不得继续进入 Feature Store 或页面。前端完成迁移后删除整个兼容层。

### Gate

- 新建 Session、发送 Turn、工具权限、取消、历史加载、Trace 页面全链可用；
- 重启后遗留 Turn 显示为 interrupted；
- 重启不会自动重新执行工具；
- Tauri crate 直接管理唯一 `OpenWorkCore` State；不直接依赖 SQLx、Repository 实现或 Tool Executor。
- 每个 Session 有独立 Runtime View，切换 Session 不丢失后台更新；
- Event 重复可去重、Sequence 缺口触发 Snapshot，Reducer 不执行 I/O；
- Rust Host Contract 与生成的 TypeScript Binding 无 Drift；
- `pnpm --dir apps/desktop test` 与 `pnpm --dir apps/desktop build` 通过。

## 10. Phase 7：删除 Legacy

只有前述 Gate 全部通过后才删除：

### 代码（已完成）

```text
openwork-protocol
openwork-persistence
```

`openwork-capabilities`、`openwork-execution`、`openwork-providers`、`openwork-observability`、`openwork-workspace`、`openwork-app` 也已删除。Provider/Runtime 入口由 `OpenWorkCore` 统一拥有，Tauri 仅保留 Host Command/Event 与安全错误映射。

### 类型/机制

```text
StepId
ToolRunId
ApprovalId
TurnRecorderPort
ApprovalRecovery
replay_turn_lifecycle
TurnSupervisor
recorded_events write path
legacy Trace span kinds
```

### 数据库

旧表先改名并只读：

```text
legacy_providers
legacy_provider_models
legacy_recorded_events
legacy_trace_spans
```

删除必须是后续独立 Migration，并要求：

- 已完成备份；
- V2 运行至少一个约定发布周期；
- 回填报告归档；
- 没有旧版本应用仍会连接该数据库；
- 操作者明确确认。

## 11. 最终验证

### 11.1 构建与测试

```sh
cargo fmt --all -- --check
cargo test -p openwork-models
cargo test -p openwork-tools
cargo test -p openwork-agent
cargo test -p openwork-chat-state
cargo test -p openwork-core
cargo test -p openwork-desktop
cargo check --workspace
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
```

实际 Desktop package 名称以 `apps/desktop/src-tauri/Cargo.toml` 为准。

### 11.2 结构搜索

```sh
rg -n 'StepId|ToolRunId|ApprovalId|ApprovalRecovery|TurnRecorderPort|JournalTurnRecorder' crates apps
rg -n 'openwork-protocol|openwork-persistence|openwork-observability|openwork-workspace' Cargo.toml crates apps
rg -n 'recorded_events|replay_turn_lifecycle' crates apps
rg -n 'TurnLiveEvent|TurnLifecycle|StepLifecycle|ToolRunLifecycle|ApprovalRecovery' apps/desktop/src
rg -n 'chat-stream-event|chat_generate_stream|approvalStore|activeStream' apps/desktop/src apps/desktop/src-tauri/src
rg -n "invoke\(|listen\(" apps/desktop/src --glob '!bridge/**'
```

最终结果应为空，或只出现在明确的 Legacy Migration/Compatibility Test 中。

### 11.3 依赖检查

确认：

- Models 是叶子；
- Tools 不依赖 Core；
- Agent/Chat State 不启动 Session Runtime；
- Core 是唯一组合根；
- Desktop 只通过 Core；
- 没有循环依赖和重复类型定义。

### 11.4 运行场景

至少手工或 E2E 验证：

1. 创建 Session；
2. 发送一个需要 `read_file` 的 Turn；
3. Tool Result 后模型生成最终答案；
4. 发送一个需要 Permission 的进程工具；
5. Allow 和 Deny 各一次；
6. 模型流中途取消；
7. 工具完成后、结果持久化前模拟失败；
8. 重启应用并确认 Turn 为 interrupted 且工具未重放；
9. 打开 Trace，核对 Model/Tool 数量与采集完整度；
10. 关闭 Trace 存储重复运行，业务结果不变。

## 12. 完成定义

只有同时满足以下条件，重构才算完成：

- 新目录结构已经成为实际编译结构，而不只是 re-export；
- `SessionActor` 是 Turn 的唯一 Runtime Owner；
- Tool Result 到下一次 Model Call 的路径只存在一份；
- Chat State 是 Conversation 唯一写入者；
- 数据库只保留目标业务表和 best-effort Trace；
- 未完成 Turn 不会被自动恢复或重放；
- Desktop 不再装配运行时内部依赖；
- Legacy crate、类型、Journal 写路径和旧 Trace 节点已删除；
- 文档中的命名、SQL 与实际代码一致。
