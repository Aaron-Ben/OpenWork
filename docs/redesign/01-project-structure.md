# OpenWork 目标项目结构

> 状态：核心 crate 边界与唯一运行链已实施；`openwork-protocol`、`openwork-persistence`、`openwork-app` 及其他 Legacy 能力 crate 已删除。Tauri 直接持有一个 `OpenWorkCore`。
>
> 参考原则：以 `grok-build` 的实际源码所有权为依据，但按 OpenWork 当前能力缩小规模。

## 1. 重构前结构为什么难维护

重构前 Workspace 有 9 个 Rust library crate：

```text
openwork-protocol
openwork-core
openwork-app
openwork-capabilities
openwork-execution
openwork-providers
openwork-persistence
openwork-observability
openwork-workspace
```

crate 数量本身不是问题，问题是运行时所有权横跨多个 crate：

```text
openwork-app::ChatRuntime
    creates openwork-core::Agent
    creates JournalTurnRecorder
    wraps recorder with Trace
    registers TurnSupervisor
    starts/finishes SessionStore Turn
    translates AgentEvent for Desktop

openwork-core::Agent
    owns Model -> Tool -> Model loop
    also records Step/ToolRun/Approval facts
    also emits Trace spans
    also waits for approval commands
```

由此产生四个结构性问题：

1. `openwork-app` 和 `openwork-core` 都像 Runtime Owner；
2. `openwork-protocol` 同时承载 Model、Tool、Journal、Trace、Approval 和领域 ID，成为公共类型堆放区；
3. 工具的声明、权限和真实副作用分散在 capabilities、execution、workspace；
4. Persistence 与 Observability 不只是 Adapter，而是反过来塑造主循环的 Step/Recovery 模型。

## 2. grok-build 给出的边界

### 2.1 xai-grok-shell 是 Runtime，不是薄壳

关键源码：

- `crates/codegen/xai-grok-shell/src/session/acp_session.rs`；
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/run_loop.rs`；
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs`；
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`。

`SessionActor` 持有 Agent、Chat State、Permission、Tool Context、Persistence 通道和当前 Prompt。`run_session` 接收命令，`handle_prompt` 准备一次 Prompt，`process_conversation_turn` 在一个 `loop` 内反复调用模型与工具。OpenWork 借用其所有权关系，但把“一次用户输入触发的完整运行”统一命名为 Turn。

因此对应关系是：

```text
xai-grok-shell::SessionActor       = 产品运行时 Owner
xai-grok-agent::Agent              = 构建完成的 Agent 定义
xai-chat-state::ChatStateHandle    = Conversation 单写者
xai-grok-tools                     = Tool Catalog/Dispatch
xai-grok-workspace                 = 工作目录与 Permission 边界参考，不复制为 OpenWork V1 crate
```

### 2.2 Agent 定义不拥有运行循环

`xai-grok-agent/src/agent.rs` 中的 `Agent` 主要持有：

- `AgentDefinition`；
- 渲染后的 System Prompt；
- `ToolBridge`；
- Reminder/Compaction 等策略；
- Hosted Tool 配置。

真正的 Model → Tool → Model 循环仍在 Shell Session 中。OpenWork 采用同样的所有权，避免再把“Agent 配置”和“Agent Runtime”塞进同一个类型。

### 2.3 Chat State 是独立单写者

Grok 的 Session Runtime 决定什么时候追加 User/Assistant/Tool Message，但 Conversation 的实际修改由 Chat State Actor 串行处理。这个边界同时解决：

- 模型请求读取到半写状态；
- 流式响应和工具结果并发修改同一个数组；
- Persistence 绕过 Runtime 自行拼接模型上下文。

OpenWork 保留这个模式。

## 3. 目标仓库结构

```text
OpenWork/
├── Cargo.toml
├── apps/
│   └── desktop/
│       ├── src/
│       │   ├── app/                     # Bootstrap、导航、唯一 Event Bridge
│       │   ├── bridge/                  # Command/Event/生成的 Host Contract
│       │   ├── features/
│       │   │   ├── sessions/            # Canonical Session/Message
│       │   │   ├── chat/                # Per-session Runtime View/Reducer
│       │   │   ├── models/
│       │   │   ├── traces/
│       │   │   └── projects/
│       │   └── shared/
│       └── src-tauri/
│           └── src/
│               ├── lib.rs              # 进程入口与 Command 注册
│               ├── state.rs            # Arc<OpenWorkCore>
│               ├── contracts.rs        # Host DTO 唯一来源
│               ├── event_bridge.rs     # Core Event -> Tauri Event
│               └── commands/           # 短生命周期 Command 适配
└── crates/
    ├── openwork-core/
    │   └── src/
    │       ├── lib.rs
    │       ├── application.rs           # OpenWorkCore Facade/Bootstrap
    │       ├── active_sessions.rs       # Session Registry
    │       ├── session/
    │       │   ├── mod.rs
    │       │   ├── actor.rs             # SessionActor 状态
    │       │   ├── handle.rs            # Cloneable API
    │       │   ├── commands.rs          # Turn/Cancel/Permission
    │       │   ├── run_loop.rs          # 唯一 Agent Loop
    │       │   ├── model_calls.rs       # 单次模型调用编排
    │       │   ├── tool_calls.rs        # Tool Call 生命周期
    │       │   ├── updates.rs           # Live Update/Snapshot
    │       │   └── tests.rs
    │       ├── storage/
    │       │   ├── mod.rs
    │       │   ├── postgres.rs
    │       │   ├── migrations.rs
    │       │   ├── models.rs
    │       │   ├── sessions.rs
    │       │   ├── turns.rs
    │       │   ├── messages.rs
    │       │   └── trace_spans.rs
    │       └── trace/
    │           ├── mod.rs
    │           ├── recorder.rs
    │           ├── instrumentation.rs
    │           ├── health.rs
    │           └── query.rs
    ├── openwork-agent/
    │   └── src/
    │       ├── lib.rs
    │       ├── definition.rs
    │       ├── builder.rs
    │       ├── prompt.rs
    │       └── policy.rs
    ├── openwork-chat-state/
    │   └── src/
    │       ├── lib.rs
    │       ├── actor.rs
    │       ├── commands.rs
    │       ├── state.rs
    │       └── snapshot.rs
    ├── openwork-models/
    │   └── src/
    │       ├── lib.rs
    │       ├── message.rs
    │       ├── request.rs
    │       ├── response.rs
    │       ├── stream.rs
    │       ├── error.rs
    │       ├── profile.rs
    │       └── providers/
    └── openwork-tools/
    │   └── src/
    │       ├── lib.rs
    │       ├── catalog.rs
    │       ├── definition.rs
    │       ├── context.rs                # working_directory/permission/cancel
    │       ├── invocation.rs
    │       ├── result.rs
    │       ├── policy.rs                 # 路径边界与 Permission Profile
    │       └── builtins/
    │           ├── filesystem/
    │           └── process/
```

目录是职责地图，不要求一次提交完成全部移动。

## 4. 依赖方向

```text
apps/desktop/src-tauri -> openwork-core + openwork-models
openwork-core -> openwork-agent + openwork-chat-state + openwork-models + openwork-tools
openwork-agent -> openwork-models + openwork-tools
openwork-chat-state -> openwork-models
openwork-tools -> openwork-models
```

允许的依赖：

| crate | 可以依赖 |
| --- | --- |
| `openwork-models` | 第三方 HTTP/Serde 库，不依赖其他 OpenWork crate |
| `openwork-tools` | `openwork-models` 与第三方 OS/进程库，不依赖 Core |
| `openwork-agent` | `openwork-models`、`openwork-tools` |
| `openwork-chat-state` | `openwork-models` |
| `openwork-core` | 上述全部能力 crate |
| `apps/desktop/src-tauri` | `openwork-core`、`openwork-models` 与 Tauri；只做 Command/Event/错误 DTO 适配 |
| `apps/desktop/src` | 只通过 Tauri Command/Event 使用 Core，不访问 SQL 或 Rust Adapter |

禁止：

- Models、Tools 反向依赖 Core；
- Desktop 直接依赖 SQLx、Provider Adapter 或 Tool Executor；
- `openwork-agent` 启动异步运行循环；
- `openwork-chat-state` 执行工具或决定权限；
- `openwork-core` 暴露数据库 Record 类型给 Desktop。

## 5. 每个 crate 的职责

### 5.1 openwork-core

`openwork-core` 是产品运行时，拥有：

- `OpenWorkCore::bootstrap`；
- 活动 Session Registry；
- `SessionActor`、`SessionHandle` 和 `SessionCommand`；
- Turn 排队、取消和终态；
- Model/Tool/Permission 编排；
- Persistence 写入顺序；
- Live Session Update 和内存 Snapshot；
- PostgreSQL Migration/Repository；
- Trace Recorder、采集健康度和查询；
- 面向 Desktop 的 Host-neutral Facade。

它不拥有具体模型协议编码、工具实现和文件系统细节。

### 5.2 openwork-agent

`openwork-agent` 回答“这个 Agent 是什么”：

- Agent 名称和描述；
- System Prompt；
- 可用 Tool Set；
- 默认模型参数；
- 最大 Model Call 次数；
- Permission Mode；
- Doom-loop 等静态策略。

`AgentBuilder::build` 返回近似不可变的 `Agent`。运行中的消息、Turn、Tool Call 和 Permission 不属于这里。

### 5.3 openwork-chat-state

`openwork-chat-state` 回答“模型下一次会看到什么”：

- 有序 Conversation；
- User/Assistant/Tool Message 追加；
- 当前 Turn 的流式草稿；
- 请求快照；
- Token/Usage 累计；
- 完整 Message 持久化通知。

只有 Actor 可以修改状态。Core 通过 Command 写入，通过 Snapshot 读取。

### 5.4 openwork-models

合并当前 `openwork-protocol::model` 与 `openwork-providers`：

- Message/Content Block；
- Model Request/Response/Event；
- `Model` trait；
- OpenAI Chat、OpenAI Responses、Anthropic Messages Adapter；
- SSE 解析、Transport Retry、Error 归一化；
- Provider Request ID、Usage 和 Opaque Block 保留。

Models 不知道 Session、Turn、Trace 表或 Desktop。

### 5.5 openwork-tools

合并当前 capability 的模型表面与 execution 的工具分发：

- Tool Definition/Schema；
- Tool Catalog 和名称解析；
- 输入反序列化与校验；
- Tool Invocation；
- Tool Result；
- `ToolContext`：工作目录、Permission Profile 和 Cancellation Token；
- 安全路径解析和读写根边界；
- 文件读写与进程/终端内置工具；
- 内置工具的业务适配和执行期强制校验。

Core 拥有 Tool Call 生命周期和用户授权等待；Tools 提供工具风险信息，并在执行时强制应用路径/进程策略。`Allow` 不能绕过 `ToolContext` 的安全边界。

### 5.6 V1 不设 openwork-workspace

工作目录仍然存在，但只是 Session 创建时确定并传给 `ToolContext` 的路径值，不是独立领域对象或服务。

当前 `openwork-workspace` 只服务 Git Worktree Snapshot、文件差异和回滚。V1 不实现 Git Status/Diff、Snapshot/Revert 或 Workspace Trust，因此删除该 crate。以后只有在出现多个消费者共享的 Git、Sandbox 或 Checkpoint 能力时，才重新评估是否拆出独立 crate。

`openwork-tools` 根据 Tool Definition、输入和 `ToolContext` 提供风险/策略结果；`SessionActor` 处理 `Allow`、`Ask`、`Deny`，并托管用户交互等待。

## 6. 当前到目标的文件映射

| 当前位置 | 目标位置 | 处理 |
| --- | --- | --- |
| `openwork-app/src/application.rs` | `openwork-core/src/core.rs` + `apps/desktop/src-tauri/src/lib.rs` | Core 装配 Repository/Credential/Runtime；Tauri 只注册 Core State；删除 App crate |
| `openwork-app/src/chat.rs` | `openwork-core/src/session/*` | 按 Actor/Loop/Model/Tool 拆分 |
| `openwork-app/src/turn_supervisor.rs` | `openwork-core/src/active_sessions.rs` + `session/actor.rs` | 删除 Turn 双重管理 |
| `openwork-core/src/agent.rs` | `openwork-core/src/session/run_loop.rs` | 只保留控制循环 |
| `agent.rs` 内 System Prompt/Config | `openwork-agent` | 与运行状态分离 |
| `openwork-protocol/src/model` | `openwork-models` | 类型跟随 Owner |
| `openwork-providers` | `openwork-models/providers` | 合并 Adapter |
| `openwork-protocol/src/capability` | `openwork-tools` | Tool 契约跟随 Tool Owner |
| `openwork-capabilities` | `openwork-tools` | 合并 Catalog/Resolve |
| `openwork-execution` | `openwork-tools` | 合并 Dispatch、文件/进程副作用与执行期策略 |
| `openwork-workspace` | 删除 | Git/Diff/Snapshot/Revert 不进入 V1 |
| `openwork-persistence` | `openwork-core/storage` | Core 决定顺序和事务 |
| `openwork-observability` | `openwork-core/trace` | 降级为旁路能力 |
| `openwork-protocol` 其余类型 | 各 Owner 或 Core public API | 最终删除公共类型仓库 |

## 7. SessionActor 的最小状态

```rust
struct SessionActor {
    session: Session,
    agent: Agent,
    chat: ChatStateHandle,
    model: Box<dyn Model>,
    tools: ToolCatalog,
    tool_context: ToolContext,
    storage: SessionStorage,
    trace: TraceRecorder,
    active_turn: Option<ActiveTurn>,
    command_rx: mpsc::Receiver<SessionCommand>,
    live_updates: broadcast::Sender<SessionUpdate>,
}
```

这是职责示意，不是要求逐字段照抄。关键不变量是：

- 一个活动 Session 只有一个 Actor；
- 一个 Actor 同时最多推进一个 Turn；
- Tool Result 写回 Chat State 后，仍由同一个 Actor 启动下一次 Model Call；
- Trace、Storage 和 Desktop 都不能自行推进 Turn。

## 8. 对外 API

Desktop 只面对：

```rust
OpenWorkCore::create_session(...)
OpenWorkCore::list_sessions(...)
OpenWorkCore::load_session(...)
OpenWorkCore::start_turn(session_id, client_request_id, input)
OpenWorkCore::cancel_turn(session_id, turn_id)
OpenWorkCore::resolve_permission(session_id, turn_id, tool_call_id, decision)
OpenWorkCore::subscribe_updates()
OpenWorkCore::get_session_snapshot(session_id)
OpenWorkCore::list_traces(...)
OpenWorkCore::get_trace(turn_id)
```

Desktop 不再创建 Agent、Provider、Recorder 或 Turn Supervisor。

React 不直接调用上述 Rust API。`src-tauri` 将它们映射为短生命周期 Command，并在进程启动时把 Core Update 统一转成 Tauri Event。TypeScript Contract 从 Rust Host DTO 生成；Canonical Message、Live Runtime View 和本地 UI State 分开保存。详细设计见 [06-frontend-architecture.md](06-frontend-architecture.md)。

## 9. 命名规则

目标领域词汇：

```text
Session
└── Turn
    ├── Model Call
    ├── Tool Call
    ├── Permission Request (optional)
    └── Turn Outcome
```

- `Turn`：一次用户输入触发的完整运行；
- `Prompt`：只表示 System/User Prompt 等模型指令内容，不是运行聚合；
- `Model Call`：循环内一次模型请求/响应；
- `Tool Call`：Provider Tool Call 从解析到结果的完整生命周期；
- `Permission Request`：Tool Call 的临时等待状态，不是独立 Aggregate；
- `model_call_index`：局部计数，不是 ID；
- `SessionUpdate`：Live UI 消息，不是持久化事实；
- `TraceSpan`：诊断记录，不是恢复状态。

目标代码不再使用：

```text
StepId
ToolRunId
ApprovalId
ApprovalRecovery
TurnRecorderPort
JournalTurnRecorder
```

## 10. 结构验收规则

每次迁移一个模块都检查：

1. 谁创建它；
2. 谁修改它；
3. 谁决定下一状态；
4. 谁持久化；
5. 失败是否会改变主流程；
6. 是否产生反向依赖。

若一个类型需要同时回答两个以上的 Owner，先拆职责，再移动文件。最终应能从目录结构直接找到一次 Turn 的完整控制路径，而不需要在 App、Core、Persistence 和 Observability 之间来回跳转。
