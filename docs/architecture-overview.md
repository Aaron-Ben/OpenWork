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
- 会话、message parts、LLM events 持久化
- PostgreSQL 中的 Provider API Key 加密存储
- `openwork-workspace` 中的 worktree 变更快照与还原基础函数（尚未接入 runtime/Tauri/UI）
- 桌面端流式 UI 与审批弹窗

## 2. Workspace 模块

```text
crates/openwork-protocol/
  稳定协议与 Port：ModelRequest、ModelResponse、ModelEvent、ModelError、
  ModelPort、ProviderConfig、ProviderRepository、CapabilitySpec、ActionRequest、
  Observation、CapabilityResolverPort、ExecutionPort。新代码应直接依赖它。

crates/openwork-db-macros/
  PostgreSQL entity derive 宏：当前提供 `PgEntity`，生成表名、字段、主键和索引 metadata。

crates/openwork-providers/
  纯模型协议适配层：OpenAI、Anthropic、Kimi、DeepSeek、Qwen、GLM、
  以及统一 HTTP 错误映射和流式感知 Transport Retry。

crates/openwork-persistence/
  PostgreSQL Repository Adapter：当前实现 Provider 配置与 Provider Models 的事务、
  Migration 和 `ProviderRepository`；API Key 使用 AES-256-GCM 加密后落库，
  不包含模型 HTTP 调用或 UI Preset。

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

crates/openwork-session/
  PostgreSQL 会话存储：sessions、messages、llm_events、tool_runs。

crates/openwork-database/
  PostgreSQL 连接与 entity metadata 基础设施：DatabaseConfig、PgPool 创建与共享、
  PgSchema、PgCrud SQL 片段生成。

crates/openwork-workspace/
  workspace / git 辅助能力：工作区状态读取、文件内容 diff、文本预览转换。

apps/desktop/
  Tauri + React + TypeScript 桌面端：provider 管理、session 管理、聊天 UI、
  stream event 消费、审批 UI。Rust 后端 command 已按 provider/session/chat 拆分。
```

## 3. 核心数据流

一次聊天请求的大致路径：

```text
ChatView
  -> chat_generate_stream Tauri command
  -> SessionStore 读取会话历史
  -> PostgresProviderRepository 读取 Provider Profile，并解密 api_key_encrypted
  -> ProviderFactory::build 构造带 RetryPolicy 的 ModelPort
  -> openwork-app::ChatRuntime 组装一次 Turn
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
  -> SessionStore 持久化新增 messages
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

`openwork-app` 组合 Provider、Session、Capabilities、Execution 与 Core，并提供 Desktop 使用的 `ChatRuntime`。旧 `openwork-runtime`、`openwork-agent` 和 `openwork-permissions` crate 已删除。

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

### 4.6 `openwork-session` 保存消息和 trace

`messages.parts_json` 用于重新加载聊天上下文并保存 message blocks；`llm_events` 用于 trace / observability，而不是直接替代 messages。当前还没有可恢复 Turn 状态机，工具执行后崩溃不能仅靠这些表可靠恢复。

### 4.7 `openwork-workspace` 是工作区变更层

目标上应由 `openwork-workspace` 统一负责 git status、文件 diff 和快照还原。但当前 runtime、Tauri 和 UI 都没有调用这些函数，`SessionStore` 也没有持久化 worktree snapshot。

### 4.8 `openwork-database` 是 PostgreSQL 基础层

当前已经有显式 `Database` / `DatabaseConfig` / `PgPool` 连接对象与 `schema_migrations` runner。Provider 的 SQL、Migration 和事务已进入 `openwork-persistence`；Session 仍保留在 `openwork-session`，将在目标架构后续阶段迁移。

### 4.9 API Key 加密是 Persistence 内部实现

API Key 仍是 Provider Repository 的字段，因此没有新增 Port 或 Adapter。`openwork-persistence::ApiKeyCipher` 在写入前加密、`load_runtime` 时解密；PostgreSQL 只保存版本化密文。主密钥由 Composition Root 通过环境配置提供，不进入数据库、Protocol DTO、日志或模型工具列表。

## 5. 关键约束

- 当前没有真正的操作系统级 sandbox。
- `CapabilityRiskHint` 已参与审批原因生成，但当前策略仍只有 `Untrusted` 和 `Never` 两种真实语义。
- `schema.rs` 只支持当前内置 Action 使用的 JSON Schema 子集，不是通用 JSON Schema 引擎。
- 审批状态当前是进程内可观察状态；Event Journal 和应用重启恢复尚未完成。
- `bash` 使用 `sh -c` 执行命令；虽然有用户审批、超时、取消和受限环境变量，但不能保证命令内部文件访问被 `PermissionProfile` 精细约束。
- Provider 的统一事件不包含 Runtime Step；不同厂商的 tool call delta 仍需持续补充 fixture 测试。

## 6. 推荐演进顺序

以 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md) 为唯一执行入口；本页不再维护第二套路线顺序。
