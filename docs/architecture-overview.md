# OpenWork 当前架构概览

Last reviewed: 2026-06-25

## 1. 项目定位

OpenWork 当前是一个基于 Rust workspace 和 Tauri 桌面端的 agent 应用基础设施。代码已经不只是 provider demo，现阶段核心能力包括：

- 多 provider 配置与调用
- 统一 AI message / content block / tool call 类型
- Agent loop：模型调用、工具审批、工具执行、多步循环
- 内置工具：文件读写、搜索、bash
- 基础权限模型与 human-in-the-loop 审批
- 会话、message parts、LLM events 持久化
- worktree 变更快照与还原
- 桌面端流式 UI 与审批弹窗

## 2. Workspace 模块

```text
crates/openwork-protocol/
  AI 领域共享协议类型：Message、ContentBlock、GenerateRequest、GenerateResponse、
  GenerateStreamEvent、Provider trait、错误类型。新代码应直接依赖它。

crates/openwork-db-macros/
  PostgreSQL entity derive 宏：当前提供 `PgEntity`，生成表名、字段、主键和索引 metadata。

crates/openwork-providers/
  Provider 适配层：OpenAI、Anthropic、Kimi、DeepSeek、Qwen、GLM、
  OpenAI-compatible，以及 provider 配置存储。

crates/openwork-agent/
  Agent loop：模型流式调用、工具调用调度、审批等待、doom-loop 检测、取消处理。

crates/openwork-runtime/
  宿主组合层：当前保留 model registry，并 re-export agent / permissions 的宿主 API。

crates/openwork-tools/
  工具抽象与内置工具：read、write、edit、list、grep、glob、bash；
  不再定义权限模型与审批模型。

crates/openwork-permissions/
  权限与审批策略：ApprovalPolicy、ApprovalsReviewer、ApprovalBridge、
  PermissionProfile、文件系统和网络权限模式。

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
  -> ProviderStore 构造当前 provider
  -> openwork-agent::Agent::run
     -> provider.stream_generate
     -> AgentEvent 流式转发给 Tauri
     -> 需要工具时发 ApprovalRequest
     -> 前端 resolve_approval
     -> 工具执行
     -> ToolResult 回填模型上下文
     -> 下一轮模型调用，直到无工具调用
  -> SessionStore 持久化新增 messages
  -> openwork-workspace 捕获本轮 worktree diff
  -> SessionStore 持久化 worktree snapshot
  -> emit done
  -> 前端 reload session
```

## 4. 当前已经成型的边界

### 4.1 `openwork-protocol` 是协议核心

所有 provider、runtime、session、desktop 都应该围绕 `openwork-protocol` 的类型工作。新增 provider 或工具时，优先复用：

- `Message`
- `ContentBlock`
- `ToolCallBlock`
- `ToolResultBlock`
- `GenerateStreamEvent`
- `ToolDefinition`

不要在 provider 或前端独立发明另一套 message shape，除非只是 UI 层临时 view model。

### 4.2 `openwork-agent` 是编排层

`openwork-runtime` 当前已经从 agent loop 中退出来，保留 model registry 和宿主 API re-export。真正的多步循环、工具调用、doom-loop 检测、取消处理在 `openwork-agent`。

### 4.3 `openwork-tools` 是能力层

工具定义 JSON Schema 和执行逻辑在这里。权限模型和审批策略在 `openwork-permissions`；工具执行时只通过 `ToolContext` 消费权限，不负责决定是否审批。

### 4.4 `openwork-session` 是可恢复状态

`messages.parts_json` 用于恢复聊天上下文并保存完整 message blocks；`llm_events` 用于 trace / observability，而不是直接替代 messages。

### 4.5 `openwork-workspace` 是工作区变更层

桌面端不直接操作 git status、文件 diff 或快照还原。`openwork-workspace` 负责把工作区变更转成稳定的 `WorktreeFileChange`，Tauri 只负责调用它并把结果交给 `SessionStore` 持久化。

### 4.6 `openwork-database` 是 PostgreSQL 基础层

当前已经有显式 `Database` / `DatabaseConfig` / `PgPool` 连接对象、`schema_migrations` migration runner，以及 `PgSchema` / `PgCrud` metadata 和基础 SQL 片段生成。`openwork-db-macros::PgEntity` 已能生成表名、字段、主键和索引 metadata；provider/session schema 已迁入 records + migrations，业务 store 的 CRUD SQL 还需要继续收口到 typed query。

## 5. 关键约束

- 当前没有真正的操作系统级 sandbox。
- `ApprovalPolicy::OnFailure`、`OnRequest`、`Granular` 还没有沙箱支撑，目前保守降级为需要审批。
- `bash` 使用 `sh -c` 执行命令；虽然有用户审批、超时、取消和受限环境变量，但不能保证命令内部文件访问被 `PermissionProfile` 精细约束。
- provider streaming 的事件规范正在统一中，不同厂商的 tool call delta 差异需要更多测试。

## 6. 推荐演进顺序

1. 补齐工具生命周期与 `tool_runs` 持久化。
2. 加强 `bash` 风险识别、审批上下文和执行记录。
3. 给 `streamAccumulator`、`ToolStream`、provider streaming 增加测试。
4. 将 model registry 真正接入桌面端请求路由。
5. 处理前端资源体积，尤其是中文字体。
