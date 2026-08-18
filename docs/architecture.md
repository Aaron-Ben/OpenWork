# 架构

OpenWork 是本地 Agent 工作台：Rust workspace（5 个 crate）+ Tauri 2 / React 桌面应用，PostgreSQL 持久化。

本文定义 crate 划分、依赖方向和所有权边界。单个功能的设计见各自文档（见 [README](README.md) 索引）。

## 1. 依赖方向

```text
desktop/src          ← 只通过 Tauri Command/Event
desktop/src-tauri    → openwork-core, openwork-models, openwork-collab
openwork-core             → openwork-agent, openwork-chat-state, openwork-models,
                            openwork-tools, openwork-credentials
openwork-agent            → openwork-models, openwork-tools
openwork-chat-state       → openwork-models
openwork-tools            → openwork-models
openwork-models           → 无 OpenWork 依赖
openwork-credentials      → 无 OpenWork 依赖

openwork-collab           → openwork-models, openwork-credentials
```

**这个方向不可逆转。** 具体禁止：

- `openwork-models` / `openwork-tools` 反向依赖 `openwork-core`；
- Desktop 直接依赖 SQLx、Provider Adapter 或 Tool Executor；
- `openwork-agent` 启动异步运行循环；
- `openwork-chat-state` 执行工具或决定权限；
- `openwork-core` 把数据库 Record 类型暴露给 Desktop。

最后一行是协作模式（见 [collaboration.md](collaboration.md)），它是图里的**独立分支**：`openwork-collab` 不依赖 `openwork-core`，反向也禁止。`desktop/src-tauri` 依赖它只为取 IPC 类型并做短生命周期 Command 适配——**不得让 Desktop 绕过 daemon 直接读写 `collab_*` 表**，daemon 是这些数据的唯一写者。

## 2. 各 crate 的职责

### openwork-core

**工作台的唯一运行时入口。** `OpenWorkCore` 拥有 Provider Repository 与 Session Registry；凭证的存储与加解密已下沉到 `openwork-credentials`。

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

### openwork-credentials

`provider_credentials` 的读写与 AES-GCM 加解密。**无 OpenWork 依赖。** 它从 `openwork-core/storage` 拆出，触发条件是出现了第二个消费者（协作模式的 triage 需要同一份凭证），符合 §6 的拆 crate 门槛。

### openwork-collab

协作模式的领域与 daemon（见 [collaboration.md](collaboration.md)）：房间、消息、看板、Agent 注册表、调度与 triage、OpenCode 客户端、**面向 Agent 的 MCP 服务端**、面向 Desktop 的 IPC 服务端。**它是协作数据的唯一写者，且不依赖 `openwork-core`。**

推理不在这里：协作 Agent 跑在外部 `opencode serve` 的 session 里，本 crate 只驱动它。

## 3. 核心不变量

1. **一个活动 Session 一个 `SessionActor`**，它同时最多推进一个 Turn。
2. **OpenWork 自身的 Agent Loop 只有一处** —— `session/run_loop.rs`。Trace、Storage、Desktop 都不能推进 Turn。协作模式的推理跑在外部 Codex 进程里，由 daemon 驱动，OpenWork 不推进它，见 [collaboration.md](collaboration.md)。
3. **Conversation 只有一个写者** —— `openwork-chat-state`。
4. **模型总是用户显式选择**（`providerId + model`），没有自动选择或跨模型 fallback。
5. **Trace 是 best-effort** —— Trace/队列/数据库失败不得让 Turn 失败。正文写入失败时 Span 本身仍须落库。
6. **同一份内容只有一个权威副本** —— `messages` 已有的内容，Trace 只留指针不复制。
7. **权限 `Allow` 不能绕过 `ToolSessionContext` 的路径边界**——对六个文件工具成立。`bash` 没有执行期边界，审批即边界，见 [permissions.md §1.2](permissions.md)。

## 4. 对外 API

Desktop 只面对这些：

```rust
OpenWorkCore::create_session / list_sessions / load_session
OpenWorkCore::start_turn(session_id, client_request_id, input: Vec<UserInput>,
                         context_window_tokens)
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
├── Turn                     一次用户输入触发的完整 Agent Loop
│   ├── Model Call           循环内一次模型请求/响应
│   ├── Tool Call            Provider Tool Call 从解析到结果
│   ├── Permission Request   Tool Call 的临时等待状态，不是独立聚合
│   └── Turn Outcome
└── Sub-Agent                本身也是一个 Session，父子关系记在 sessions 表
```

其他术语：

- **Prompt** 只表示 System/User Prompt 等指令内容，不是运行聚合；
- **Compaction** 是把 Conversation 压成摘要投影的操作，见 [compaction.md](compaction.md)；
- **SessionUpdate** 是 Live UI 消息，不是持久化事实；
- **Trace Span** 是质量记录（模型看到什么、说了什么、烧了多少 token），不是恢复状态；
- **Sub-Agent** 是主 Agent 派生的只读从属 Session，见 [multi-agent.md](multi-agent.md)。它不是新的运行聚合——一切 Turn 语义与根会话完全相同；
- **协作模式** 是与本工作台四层不相交的独立子系统：常驻 daemon + 一个外部 `opencode serve`，每个协作 Agent 是它的一个 session、各带私有 home，见 [collaboration.md](collaboration.md)。它的 **Room**、**协作 Agent** 与本篇的 Session、Sub-Agent 没有继承关系，不要混用；
- **Agent Message** 是子 Agent 回传给父的消息，以 `message_kind = 'agent_message'` 存在父的 Conversation 里，**永不触发 Turn**。

**不要重新引入的退役术语**：`StepId`、`ToolRunId`、`ApprovalId`、`TurnRecorderPort`、`JournalTurnRecorder`、Event Journal。

## 6. V1 非目标

不要添加：MCP、Memory、Artifact、Git/Diff、Worktree、跨进程未完成 Turn 恢复、Event Journal、有损压缩、后台任务恢复。

多智能体已从非目标移出，设计见 [multi-agent.md](multi-agent.md)。它**不引入新 crate、不新增 Trace kind、不新增 SessionUpdate 类型**：子 Agent 本身就是一个 Session，复用 `SessionActor` 与唯一的 Agent Loop；父子拓扑是 `sessions` 表的四个新列；五个控制工具按 `update_plan` 的先例由 Core 拥有；子 Agent 的 Trace 独立成树，靠 `sessions.parent_session_id` / `spawn_span_id` 关联。**范围严格限定在只读、单层、异步**——可写子 Agent、多层嵌套、角色文件加载、跨子 Agent 通信都仍是非目标。子 Agent 完成时只入队不唤醒父会话，因此"后台任务恢复"仍在上面那行里。若某次改动要求新增 crate、Trace kind 或让子 Agent 能写文件，先回到 multi-agent.md 确认是不是设计走偏了。

Skill 已从非目标移出，设计见 [skills.md](skills.md)。它**不引入新 crate、不新增 Trace kind**：目录挂在 System Context 上，正文走 Conversation，启停偏好单独保存在 `skill_status`，资源与脚本复用 `read` / `bash`。用户在 Desktop 选择 `$name` 时，可见 token 与 `{ name, path }` 绑定分离；Tauri 把文本和显式选择编码为同一个有序 `Vec<UserInput>`。Core 在接受 Turn 前把 `UserInput::Skill` 解析为持久化的 contextual User-role Text，模型内容层不定义 Skill 专用类型，`ModelRequestBuilder` 和 provider adapter 只处理已有 ContentBlock。文件读取不下沉到 Bridge、Chat State 或 provider adapter。若某次改动要求新增 Skill crate 或 Trace kind，先回到 skills.md 确认是不是设计走偏了。

协作模式已从非目标移出，设计见 [collaboration.md](collaboration.md)。它在 §1 的依赖图里是一条**独立分支**：`openwork-collab` 依赖 `openwork-models` 与新拆出的 `openwork-credentials`，**不依赖 `openwork-core`**；凭证读取从 `openwork-core/storage` 下沉，是因为出现了第二个消费者，符合本篇的拆 crate 门槛。它的推理由外部 OpenCode 进程执行，因此 [context-window.md](context-window.md)、[compaction.md](compaction.md)、[trace.md](trace.md) 三篇对它不适用——上下文与工具由 OpenCode 自己管，OpenWork 只记录运行日志；[permissions.md](permissions.md) 的规则集与判定同样不适用，边界与审批协议由 OpenCode 提供，OpenWork 只做审批的呈现与回传。持久化落在 `collab_*` 表与独立的 migrations 目录，**不碰 `crates/openwork-core/migrations/`**。进程上它是独立常驻的 daemon，Desktop 只是客户端。**范围严格限定在消息与看板**——协作 Agent 各有私有 home，碰不到项目代码，也没有共享工作目录，因此上面那行里的 Git/Diff、Worktree 仍是非目标。若某次改动要求让协作 Agent 写项目代码、或让 `openwork-collab` 依赖 `openwork-core`，先回到 collaboration.md 确认是不是设计走偏了。

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
