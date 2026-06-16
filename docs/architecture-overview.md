# Anvil 当前架构概览

Last reviewed: 2026-06-17

## 1. 项目定位

Anvil 当前是一个基于 Rust workspace 和 Tauri 桌面端的 agent 应用基础设施。代码已经不只是 provider demo，现阶段核心能力包括：

- 多 provider 配置与调用
- 统一 AI message / content block / tool call 类型
- Agent loop：模型调用、工具审批、工具执行、多步循环
- 内置工具：文件读写、搜索、bash
- 基础权限模型与 human-in-the-loop 审批
- 会话、message parts、LLM events 持久化
- 桌面端流式 UI 与审批弹窗

## 2. Workspace 模块

```text
crates/anvil-core/
  AI 领域共享类型：Message、ContentBlock、GenerateRequest、GenerateResponse、
  GenerateStreamEvent、Provider trait、错误类型。

crates/anvil-providers/
  Provider 适配层：OpenAI、Anthropic、Kimi、DeepSeek、Qwen、GLM、
  OpenAI-compatible，以及 provider 配置存储。

crates/anvil-runtime/
  Agent 编排层：多步 agent loop、审批桥接、工具执行、stream 生命周期补齐、
  model registry。

crates/anvil-tools/
  工具抽象与内置工具：read、write、edit、list、grep、glob、bash；
  权限模型与审批模型也在这里定义。

crates/anvil-session/
  SQLite 会话存储：sessions、messages、message_parts、llm_events。

apps/desktop/
  Tauri + React + TypeScript 桌面端：provider 管理、session 管理、聊天 UI、
  stream event 消费、审批 UI。
```

## 3. 核心数据流

一次聊天请求的大致路径：

```text
ChatView
  -> chat_generate_stream Tauri command
  -> SessionStore 读取会话历史
  -> ProviderStore 构造当前 provider
  -> Agent::run
     -> provider.stream_generate
     -> AgentEvent 流式转发给 Tauri
     -> 需要工具时发 ApprovalRequest
     -> 前端 resolve_approval
     -> 工具执行
     -> ToolResult 回填模型上下文
     -> 下一轮模型调用，直到无工具调用
  -> SessionStore 持久化新增 messages
  -> emit done
  -> 前端 reload session
```

## 4. 当前已经成型的边界

### 4.1 `anvil-core` 是协议核心

所有 provider、runtime、session、desktop 都应该围绕 `anvil-core` 的类型工作。新增 provider 或工具时，优先复用：

- `Message`
- `ContentBlock`
- `ToolCallBlock`
- `ToolResultBlock`
- `GenerateStreamEvent`
- `ToolDefinition`

不要在 provider 或前端独立发明另一套 message shape，除非只是 UI 层临时 view model。

### 4.2 `anvil-runtime` 是编排层

审批、工具调用、多步循环、doom-loop 检测、取消处理都在 runtime。工具内部不应该自己决定是否需要审批；工具只负责根据 `ToolContext` 执行。

### 4.3 `anvil-tools` 是能力层

工具定义 JSON Schema、执行逻辑、权限检查都在这里。当前文件类工具会走路径权限检查；`bash` 仍然是弱隔离工具，需要审批但没有文件级 sandbox。

### 4.4 `anvil-session` 是可恢复状态

`messages` 用于恢复聊天上下文；`message_parts` 用于更细粒度地保存 block；`llm_events` 用于 trace / observability，而不是直接替代 messages。

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
