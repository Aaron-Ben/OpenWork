# OpenWork 当前架构概览

Last reviewed: 2026-07-11

> Status: current implementation snapshot. 目标架构和下一阶段顺序只见 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md)。

## 1. 项目定位

OpenWork 当前是一个基于 Rust workspace 和 Tauri 桌面端的 agent 应用基础设施。代码已经不只是 provider demo，现阶段核心能力包括：

- 多 provider 配置与调用
- 统一 AI message / content block / tool call 类型
- Agent loop：模型调用、工具审批、工具执行、多步循环
- 内置工具：文件读写、搜索、bash
- 基础权限模型与 human-in-the-loop 审批
- 基于 Event Journal 的 Thread、Turn 和 Message 持久化
- PostgreSQL 中的 Provider API Key 加密存储
- `openwork-workspace` 中的 worktree 变更快照与还原基础函数（尚未接入 runtime/Tauri/UI）
- 桌面端流式 UI 与审批弹窗

## 2. Workspace 模块

```text
crates/openwork-protocol/
  稳定协议与 Port：ModelRequest、ModelResponse、ModelEvent、ModelError、
  ModelPort、ProviderConfig、ProviderRepository、CapabilitySpec、ActionRequest、
  Observation、CapabilityResolverPort、ExecutionPort。新代码应直接依赖它。

crates/openwork-providers/
  纯模型协议适配层：OpenAI、Anthropic、Kimi、DeepSeek、Qwen、GLM、
  以及统一 HTTP 错误映射和流式感知 Transport Retry。

crates/openwork-persistence/
  PostgreSQL Adapter 与统一迁移入口：实现 Provider Repository 和 append-only
  Event Journal、Journal-backed Session/Message 和内存投影；API Key 使用
  AES-256-GCM 加密后落库。

crates/openwork-core/
  Turn 控制循环与状态机：模型流式调用、工具调用调度、审批暂停/恢复、
  doom-loop 检测和取消处理；不依赖具体 Capabilities 或 Execution Adapter。

crates/openwork-app/
  Application API 与 Composition Root：组合 Capability Catalog、Execution、Core、
  Provider 和 Session，并通过 TurnSupervisor 路由 ResolveApproval 命令。

crates/openwork-capabilities/
  Capability Catalog：持有 read、write、edit、list、grep、glob、bash 的名称、
  描述、JSON Schema 和声明侧风险提示，不执行真实 IO。

crates/openwork-execution/
  统一 Action 执行：`actions/filesystem` 持有文件和搜索 Handler，
  `actions/process` 持有 bash，service/schema/invoker/context 分别负责执行编排、
  参数校验、Handler 路由和运行环境，`policy` 持有 PermissionProfile、文件系统、
  网络权限模式以及 Allow/Deny/RequireApproval 判定；当前还没有 OS 级 sandbox。

crates/openwork-workspace/
  workspace / git 辅助能力：工作区状态读取、文件内容 diff、文本预览转换。

apps/desktop/
  Tauri + React + TypeScript 桌面端：provider 管理、session 管理、聊天 UI、
  stream event 消费、审批 UI。Rust 后端 command 已按 provider/session/chat 拆分，
  并且只通过单一 OpenWorkApplication 状态调用 Application API。
```

## 3. 核心数据流

一次聊天请求的大致路径：

```text
ChatView
  -> chat_generate_stream Tauri command
  -> OpenWorkApplication::turns
  -> 内部 ChatRuntime 从 SessionStore 读取会话历史
  -> PostgresProviderRepository 读取 Provider Profile，并解密 api_key_encrypted
  -> ProviderFactory::build 构造带 RetryPolicy 的 ModelPort
  -> SessionStore 在模型调用前记录 turn_started + user_message_recorded
  -> openwork-core::Agent::run
     -> CapabilityResolverPort 获取本轮 Tool Schema
     -> ModelPort::invoke
     -> AgentEvent 流式转发给 Tauri
     -> ExecutionPort::authorize 返回 Allow / Deny / RequireApproval
     -> RequireApproval 时 Core 进入 Waiting 并发出 ApprovalRequested
     -> 前端提交 ResolveApproval(turn_id, approval_id)
     -> App 将命令路由回拥有该 Turn 的 Core inbox
     -> Allow 后 ExecutionPort 执行 Action
     -> ToolResult 回填模型上下文
     -> 下一轮模型调用，直到无工具调用
  -> SessionStore 批量记录新增 Assistant/Tool Message 和 Turn 终态
  -> emit done
  -> 前端 reload session
```

当前主链路不会调用 `openwork-workspace`，也不会持久化本轮 snapshot/diff。它们是已有库能力，不是已完成的产品流程。

## 4. 当前已经成型的边界

### 4.1 `openwork-protocol` 是协议核心

所有 provider、Core、App、session、desktop 都应该围绕 `openwork-protocol` 的类型工作。新增 provider 或工具时，优先复用：

- `Message`
- `ContentBlock`
- `ToolCallBlock`
- `ToolResultBlock`
- `ModelRequest` / `ModelResponse` / `ModelEvent`
- `ModelError` / `RetryHint`
- `ProviderRepository`
- `ToolDefinition`

不要在 provider 或前端独立发明另一套 message shape，除非只是 UI 层临时 view model。

### 4.2 `openwork-core` 是控制循环与审批状态所有者

真正的多步循环、工具调用、doom-loop 检测、取消处理和审批暂停/恢复在 `openwork-core`。每个 Turn 独占一个有界命令 inbox；App 只能通过 `TurnSupervisor` 将稳定的 `ResolveApproval` 命令路由回来。

### 4.3 `openwork-app` 是应用组合层

`openwork-app::OpenWorkApplication` 是当前唯一 Composition Root：它组合 Provider、Session、Capabilities、Execution 与 Core，并向 Desktop 暴露 `ProviderApplicationService`、`ThreadApplicationService` 和 `TurnApplicationService`。内部 `ChatRuntime`、`RequestCancelRegistry`、Repository、Store 和 Factory 不再由 Tauri 直接管理。旧 `openwork-runtime`、`openwork-agent` 和 `openwork-permissions` crate 已删除。

Tauri 当前只注册一个 `OpenWorkApplication` State，Command 名称保持兼容。Phase A/B 已完成：Command 返回稳定的 `{ code, message }`，Application Live Event 使用 Rust tagged enum，TypeScript 使用同构 discriminated union；统一 shutdown 仍属于 Phase C，见 [Desktop Tauri 与 Application API 边界重构设计](../plans/desktop-tauri-application-boundary-refactor.md)。

当前 Desktop 到内部能力的依赖方向固定为：

```text
apps/desktop/src                    # React 展示、UI 状态、invoke/listen 封装
  -> apps/desktop/src-tauri         # Tauri IPC 与桌面宿主适配
  -> crates/openwork-app            # Application Command/Query、Turn 编排、Composition Root
  -> Core/Providers/Persistence/Capabilities/Execution
```

`openwork-app/src` 当前按以下职责拆分：

| 文件 | 当前职责 |
| --- | --- |
| `application.rs` | 创建具体 Adapter，并组装唯一 `OpenWorkApplication` |
| `provider_service.rs` | Provider CRUD、Preset 和连接测试用例 |
| `thread_service.rs` | Journal-backed Thread/Session 查询与管理用例 |
| `turn_service.rs` | 对宿主提供 Turn 启动、审批和取消入口，并统一终态错误事件 |
| `chat.rs` | 内部单 Turn 编排：加载 Provider/Session、创建 Execution/Core、持久化结果并映射 Live Event |
| `cancel.rs` | 当前 `request_id -> CancellationToken` 注册表 |
| `turn_supervisor.rs` | 当前 `TurnId -> TurnCommandHandle` 路由；审批状态仍由 Core inbox 持有 |
| `error.rs` | 底层错误到稳定 Application Error Code 的映射 |

`turn_service.rs` 是稳定的宿主用例门面，`chat.rs` 是其内部编排器，两者不是两套 Agent Runtime。`RequestCancelRegistry` 和 `TurnSupervisor` 目前分别承担取消与审批路由，统一活跃 Turn 生命周期及 cancel-all/shutdown 属于 Phase C。前端仍使用 `session_*` IPC 和 Session DTO，Application 层使用 Thread Service；这是有意保留的兼容层，不代表最终命名已经统一。

当前不提供 `ModelRegistry`、默认模型解析、按任务或 tier 自动选模，也不做跨模型/跨 Provider 静默 Fallback。前端必须明确提交 `providerId + model`；`ProviderFactory` 只根据已经选定的 Provider 创建对应协议 Adapter，不参与模型选择。

### 4.4 `openwork-capabilities` 是声明与发现层

内置 Tool 的名称、描述、JSON Schema 和风险提示在这里。它实现 `CapabilityResolverPort`，但不执行文件或进程 IO。`openwork-core` 只依赖 Protocol Port，不依赖具体 `CapabilityCatalog`。

`risk_hint` 现在是 Execution 审批原因的输入之一；它仍是粗粒度声明，不能自行降低权限，也不能替代针对实际参数、路径和运行位置的最终风险计算。

### 4.5 `openwork-execution` 是统一执行边界

`ExecutionService` 先通过注入的 `CapabilityResolverPort` 解析声明并校验参数，再调用注入的 `ActionInvoker`。内置 Handler、`ExecutionContext`、路径权限检查、取消、超时和 Observation 归一化都在这里。Execution 不反向依赖具体 Capabilities crate。

当前源码结构：

```text
openwork-execution/src/
├── actions/
│   ├── filesystem/     # read/write/edit/list/grep/glob
│   ├── process/        # bash
│   └── output.rs       # 跨 Action 共享的 UTF-8 安全输出截断
├── context.rs          # working_dir、PermissionProfile、CancellationToken
├── handler.rs          # crate 内部 ActionHandler 合同
├── invoker.rs          # 名称到 Handler 的路由
├── policy/             # PermissionProfile 与 Allow/Deny/RequireApproval 判定
├── schema.rs           # 当前内置 Schema 所需的受控校验子集
└── service.rs          # authorize；以及 resolve -> validate -> invoke -> Observation
```

“统一执行边界”不等于“已经安全隔离”：当前只有应用层路径检查，尚无 OS 级 sandbox，`bash` 的命令内部访问也无法由 `PermissionProfile` 精细约束。

### 4.6 Session 与 Message 已切换到 Journal

`openwork-protocol::journal` 定义 Recorded Event Envelope、Expected Version 和 `EventJournal`；`openwork-persistence::PostgresEventJournal` 实现聚合锁、批量 append 和读取。Session 的创建、改名、删除以及 Turn/Message 读写已经使用 `recorded_events`，查询时通过当前内存投影重放。旧 `sessions/messages/llm_events/tool_runs` 表和 `openwork-session/openwork-db-macros` crate 已删除。Action/Approval 的 intent/outcome 仍待在 Core 语义点直接持久化。

### 4.7 `openwork-workspace` 是工作区变更层

目标上应由 `openwork-workspace` 统一负责 git status、文件 diff 和快照还原。但当前 runtime、Tauri 和 UI 都没有调用这些函数，`SessionStore` 也没有持久化 worktree snapshot。

### 4.8 Persistence 统一拥有 PostgreSQL 生命周期

数据库 migration 由 `cargo run -p openwork-persistence --bin openwork-migrate` 显式执行；Desktop 启动只检查四张必需表。Provider、Journal 和 Session Repository 共享一个连接池。`DatabaseConfig`、私有连接对象和 migration runner 都位于 `openwork-persistence::postgres`，不存在第二个数据库基础设施 crate。

### 4.9 API Key 加密是 Persistence 内部实现

API Key 仍是 Provider Repository 的字段，因此没有新增 Port 或 Adapter。`openwork-persistence::ApiKeyCipher` 在写入前加密、`load_runtime` 时解密；PostgreSQL 只保存版本化密文。主密钥由 Composition Root 通过环境配置提供，不进入数据库、Protocol DTO、日志或模型工具列表。

## 5. 关键约束

- 当前没有真正的操作系统级 sandbox。
- `CapabilityRiskHint` 已参与审批原因生成，但当前策略仍只有 `Untrusted` 和 `Never` 两种真实语义。
- `schema.rs` 只支持当前内置 Action 使用的 JSON Schema 子集，不是通用 JSON Schema 引擎。
- Thread/Turn/Message 已进入 Event Journal；Action/Approval 状态仍是进程内状态，重启恢复尚未完成。
- `bash` 使用 `sh -c` 执行命令；虽然有用户审批、超时、取消和受限环境变量，但不能保证命令内部文件访问被 `PermissionProfile` 精细约束。
- Provider 的统一事件不包含 Runtime Step；不同厂商的 tool call delta 仍需持续补充 fixture 测试。

## 6. 推荐演进顺序

以 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md) 为唯一执行入口；本页不再维护第二套路线顺序。
