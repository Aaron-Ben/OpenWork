# 架构

OpenWork 是本地 Agent 工作台：Rust workspace（5 个 crate）+ Tauri 2 / React 桌面应用，PostgreSQL 持久化。

本文定义 crate 划分、依赖方向和所有权边界。单个功能的设计见各自文档（见 [README](README.md) 索引）。

## 1. 依赖方向

```text
apps/desktop/src          ← 只通过 Tauri Command/Event
apps/desktop/src-tauri    → openwork-core, openwork-models
openwork-core             → openwork-agent, openwork-chat-state, openwork-models, openwork-tools
openwork-agent            → openwork-models, openwork-tools
openwork-chat-state       → openwork-models
openwork-tools            → openwork-models
openwork-models           → 无 OpenWork 依赖
```

**这个方向不可逆转。** 具体禁止：

- `openwork-models` / `openwork-tools` 反向依赖 `openwork-core`；
- Desktop 直接依赖 SQLx、Provider Adapter 或 Tool Executor；
- `openwork-agent` 启动异步运行循环；
- `openwork-chat-state` 执行工具或决定权限；
- `openwork-core` 把数据库 Record 类型暴露给 Desktop。

## 2. 各 crate 的职责

### openwork-core

**唯一的运行时入口。** `OpenWorkCore` 拥有 Provider Repository、凭证、Session Registry。

```text
src/
├── core.rs         OpenWorkCore facade 与 bootstrap
├── provider.rs     Provider 预设与索引
├── session/        SessionActor、Agent Loop、压缩、Trace Guard
├── context/        System Context 物化
├── model_call/     请求组装与预算估算
└── storage/        PostgreSQL 读写、Trace Recorder
```

它**不**拥有：模型协议编码、工具实现、文件系统细节。

### openwork-agent

回答"这个 Agent 是什么"——**静态定义，不含运行状态**：System Prompt、可用工具集、默认模型参数、最大 Model Call 次数、Permission Mode。

`AgentBuilder::build` 返回近似不可变的 `Agent`。**它不得启动任何异步循环。**

### openwork-chat-state

回答"模型下一次会看到什么"：有序 Conversation、流式草稿、类型化 Conversation Item。

**它是 Conversation 的唯一写者。** Core 通过 Command 写入、通过 Snapshot 读取，不直接改内部状态。

### openwork-models

Message/ContentBlock、Model Request/Response/Event、`ModelPort` trait、各 Provider Adapter、SSE 解析、Transport Retry、错误归一化。

**它不知道 Session、Turn、Trace 或 Desktop 的存在。** 这是它能被独立测试的原因。

### openwork-tools

工具的四层运行时（契约 / 工具集 / 会话上下文 / 调用上下文）、路径安全策略、文件与进程后端、内置工具实现。详见 [tools.md](tools.md)。

分工：Tools 提供风险信息并在**执行时**强制路径/进程边界；Core 拥有 Tool Call 生命周期和用户授权等待。

## 3. 核心不变量

1. **一个活动 Session 一个 `SessionActor`**，它同时最多推进一个 Turn。
2. **Agent Loop 只有一处** —— `session/run_loop.rs`。Trace、Storage、Desktop 都不能推进 Turn。
3. **Conversation 只有一个写者** —— `openwork-chat-state`。
4. **模型总是用户显式选择**（`providerId + model`），没有自动选择或跨模型 fallback。
5. **Trace 是 best-effort** —— Trace/队列/数据库失败不得让 Turn 失败。正文写入失败时 Span 本身仍须落库。
6. **同一份内容只有一个权威副本** —— `messages` 已有的内容，Trace 只留指针不复制。
7. **权限 `Allow` 不能绕过 `ToolSessionContext` 的路径/进程边界。** 没有 OS 级沙箱。

## 4. 对外 API

Desktop 只面对这些：

```rust
OpenWorkCore::create_session / list_sessions / load_session
OpenWorkCore::start_turn(session_id, client_request_id, input)
OpenWorkCore::cancel_turn(session_id, turn_id)
OpenWorkCore::resolve_permission(session_id, turn_id, tool_call_id, decision)
OpenWorkCore::subscribe_updates()
OpenWorkCore::get_session_snapshot(session_id)
OpenWorkCore::compact_conversation / rewind_conversation
OpenWorkCore::list_traces / get_trace / get_trace_by_id
                / get_span_payload / list_compaction_spans
                / upsert_annotation
```

React **不**直接调用这些。`src-tauri` 把它们映射成短生命周期 Command，并在进程启动时把 Core Update 转成 Tauri Event。详见 [desktop.md](desktop.md)。

## 5. 领域词汇

```text
Session
└── Turn                     一次用户输入触发的完整 Agent Loop
    ├── Model Call           循环内一次模型请求/响应
    ├── Tool Call            Provider Tool Call 从解析到结果
    ├── Permission Request   Tool Call 的临时等待状态，不是独立聚合
    └── Turn Outcome
```

其他术语：

- **Prompt** 只表示 System/User Prompt 等指令内容，不是运行聚合；
- **Compaction** 是把 Conversation 压成摘要投影的操作，见 [compaction.md](compaction.md)；
- **SessionUpdate** 是 Live UI 消息，不是持久化事实；
- **Trace Span** 是质量记录（模型看到什么、说了什么、烧了多少 token），不是恢复状态。

**不要重新引入的退役术语**：`StepId`、`ToolRunId`、`ApprovalId`、`TurnRecorderPort`、`JournalTurnRecorder`、Event Journal。

## 6. V1 非目标

不要添加：MCP、Memory、Plan、Skill、Artifact、Git/Diff、Worktree、跨进程未完成 Turn 恢复、Event Journal、有损压缩、后台任务恢复。

工作目录只是 Session 创建时确定、传给 `ToolSessionContext` 的路径值，不是独立领域对象——因此没有 `openwork-workspace` crate。只有当出现多个消费者共享的 Git、Sandbox 或 Checkpoint 能力时，才重新评估是否拆出。

## 7. 新增模块时的检查

每引入一个模块，回答六个问题：

1. 谁创建它；
2. 谁修改它；
3. 谁决定下一状态；
4. 谁持久化；
5. 它失败是否会改变主流程；
6. 是否产生反向依赖。

**若一个类型需要同时回答两个以上的 Owner，先拆职责再落地。** 最终应当能从目录结构直接读出一次 Turn 的完整控制路径，而不需要在多个 crate 之间反复跳转。
