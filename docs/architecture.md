# 架构

OpenWork 是本地 Agent 工作台。它由 Rust workspace 与 Tauri 2 / React 桌面应用组成，用 PostgreSQL 持久化。协作模式另外用 Redis 传递 wake，并保存短期协调状态。

本文定义 crate 划分、依赖方向和所有权边界。各功能的设计见对应文档，索引见 [README](README.md)。

## 1. 依赖方向

```text
desktop/src          ← 只通过 Tauri Command/Event
desktop/src-tauri    → openwork-core, openwork-models, openwork-collab
openwork-core             → openwork-agent, openwork-chat-state, openwork-models,
                            openwork-tools, openwork-sandbox, openwork-credentials
openwork-agent            → openwork-models, openwork-tools, openwork-sandbox
openwork-chat-state       → openwork-models
openwork-tools            → openwork-models, openwork-sandbox
openwork-sandbox          → 无 OpenWork 依赖
openwork-models           → 无 OpenWork 依赖
openwork-credentials      → 无 OpenWork 依赖

openwork-collab           → openwork-sandbox；除此之外是独立协作分支，内部单向依赖见下文
```

**这个方向不可逆转。** 具体禁止以下情况：

- `openwork-models` / `openwork-tools` / `openwork-sandbox` 反向依赖 `openwork-core`；
- `openwork-sandbox` 依赖任何其他 OpenWork crate，或自己启动工具进程（启动自检除外），或做审批决定；
- Desktop 直接依赖 SQLx、Provider Adapter 或 Tool Executor；
- `openwork-agent` 启动异步运行循环；
- `openwork-chat-state` 执行工具或决定权限；
- `openwork-core` 把数据库 Record 类型暴露给 Desktop。

依赖图的最后一行是协作模式，见 [collaboration.md](collaboration.md)。它是图里的**独立分支**。Server 拥有 PostgreSQL/Redis 与业务事实，本机 Computer daemon 拥有 Engine 运行时。二者只通过 crate 内 `protocol` 定义的契约通信。`desktop/src-tauri` 监督这两个进程，只使用 Desktop HTTP interface。**不得让 Desktop 绕过 Collaboration Server 直接读写 `collab_*` 表**。

## 2. 各 crate 的职责

### openwork-core

**工作台的唯一运行时入口。** `OpenWorkCore` 拥有 Provider Repository 与 Session Registry。凭证的存储与加解密已移到 `openwork-credentials`。

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

它回答"这个 Agent 是什么"。它是**静态定义，不含运行状态**，内容包括：System Prompt、可用工具集、默认模型参数、最大 Model Call 次数、沙箱模式上限（`sandbox_ceiling`，见 [multi-agent.md §4](multi-agent.md)）。

`AgentBuilder::build` 返回近似不可变的 `Agent`。**它不得启动任何异步循环。**

### openwork-chat-state

它回答"模型下一次会看到什么"，内容包括：有序 Conversation、流式草稿、类型化 Conversation Item。

**它是 Conversation 的唯一写者。** Core 通过 Command 写入，通过 Snapshot 读取，不直接修改内部状态。

### openwork-models

Message/ContentBlock、Model Request/Response/Event、`ModelPort` trait、各 Provider Adapter、SSE 解析、Transport Retry、错误归一化。

**它不知道 Session、Turn、Trace 或 Desktop 的存在。** 这是它能独立测试的原因。

### openwork-tools

工具的四层运行时（契约 / 工具集 / 会话上下文 / 调用上下文）、路径解析（`CheckedPath`）、文件与进程后端、内置工具实现、危险命令检测（`tree-sitter-bash`）。详见 [tools.md](tools.md)。

分工如下。Tools 在**执行时**强制边界：bash 经 `openwork-sandbox` 包装后启动；文件工具用 `openwork-sandbox` 的同一组路径函数做围栏。Core 拥有 Tool Call 生命周期、越界校验和用户授权等待。

### openwork-sandbox

**回答"这一次调用能读写哪里"，并让内核兑现它。** 详见 [permissions.md §2.4、§3](permissions.md)。

```text
src/
├── tiers.rs      四档路径与可写设备的唯一清单，及其两种渲染（路径匹配 / Seatbelt 过滤器）
├── policy.rs     SandboxMode、SandboxPolicy、PathGrant；check(路径, 读写, actor)；越界请求校验
├── seatbelt.rs   Seatbelt profile 生成与 argv 包装（路径与正则都经 -D 参数传入）
├── probe.rs      启动时的功能性自检
├── backend.rs    SandboxBackend：自检结论（进程内缓存）+ 包装 argv
└── denial.rs     区分"命令被内核拒绝"与"沙箱本身没能启动"
```

| 它拥有 | 它不拥有 |
|---|---|
| 模式（`auto` / `accept-edits`，子 Agent 的角色上限也取这两者之一）与四档路径的**唯一定义**：可写根、受保护子路径、凭据禁读 | 启动工具进程：`ProcessBackend` 负责。唯一的例外是启动自检：它自己运行两次 `sandbox-exec`。这是一次性的环境检查，不是 Tool Call |
| Seatbelt profile 与 `sandbox-exec` argv；越界请求的路径校验（数量、绝对路径、硬保护、过宽、是否带来新权限） | 越界是否批准、`justification` 是否为空：Core 负责 |
| 自检结论（可用 / 不可用及原因） | 危险命令检测：它关心 bash 语法，不关心沙箱，所以留在 `openwork-tools` |
| 协作 Engine 进程的围栏 `EngineConfinement`：可写根、`$HOME` 下的可读例外及其 Seatbelt profile（[collaboration.md §3.1](collaboration.md)） | 围栏里放哪些目录：协作 Computer 按 Agent home 布局决定 |
| 拒绝识别 | 会话模式的存储：Core 负责 |

**单独成 crate 的理由是多个消费者共享同一份事实。** `openwork-tools` 用它包装 bash，并为文件工具做围栏。`openwork-collab` 用它把本机 Engine 进程关进 Seatbelt，并与工作台共用 profile 生成和路径转义。`openwork-core` 用它盖章每次调用的策略、校验越界请求、写 Trace 与策略上下文。`openwork-agent` 用它声明角色的模式上限。

三处若各自持有路径知识，就会出现"bash 能写而 write 工具不能写"这类不一致。对等测试（[permissions.md §2.4](permissions.md)）也只在同一个 crate 提供推导函数时才有意义。

**无 OpenWork 依赖**，只依赖标准库与序列化。平台差异也收在它内部，例如 P2 的 Linux 后端。

### openwork-credentials

它负责 `provider_credentials` 的读写与 AES-GCM 加解密。**无 OpenWork 依赖。** BYOA 协作模式不使用它。主推理与 triage 都使用 Computer 上 OpenCode 自己的登录态，Server 不持有对应的 Provider 凭证。

### openwork-collab

协作模式仍只用一个 crate，并把各自独立的生命周期放进内部模块：

```text
protocol/   ← server/
protocol/   ← computer/
server/     ← bin/openwork-collab.rs
computer/   ← bin/openwork-collab.rs
protocol/   ← bin/openwork.rs
```

`protocol/` 只保存 Server、Computer、shim 与 Desktop host 共同使用的线协议。它不包含 SQL、Redis、进程管理、Engine 原生事件或业务判断。

`server/` 是协作世界持久事实的唯一写者。它拥有 loopback HTTP/SSE、RuntimeSession、房间/消息/看板/Climate、Run、Agenda、PostgreSQL 与 Redis coordination。它不启动 Engine。

`computer/` 是 macOS 本机 BYOA 宿主。它拥有 desired-state reconcile、`AgentRunner`、home、shim、`EngineRegistry`、`EngineAdapter` interface 与生产 `OpenCodeAdapter`。它经 `openwork-sandbox` 在 Seatbelt 下启动 Engine。它不直接使用 SQLx、Redis、`openwork-core` 或 `openwork-credentials`，也不持有 Server 数据库凭证。

目标进程与通信边界：

```text
React → Tauri Command → /desktop/* HTTP/SSE → Collaboration Server
Computer daemon → /computer/* HTTP/SSE → Collaboration Server
Agent shim → /agent/* HTTP → Collaboration Server
Collaboration Server → PostgreSQL + Redis
Computer daemon → per-Agent Engine child processes
```

Desktop 是唯一的 supervisor。每次启动时，它先创建一个 RuntimeSession 和临时凭证，再启动 Server 与 Computer。Desktop 退出时，先停止 Computer 及其 Engine 进程组，再停止 Server。Runtime 不使用系统常驻任务、持久机器身份、固定端口或本地 socket transport。

单 crate 是有意的选择。当前实现只随同一个 macOS 应用一起构建、安装和升级。protocol 没有独立发布者，Computer 也没有远程部署目标。只为目录边界拆 crate，会增加 manifest、错误类型、测试 fixture 与版本协商，但不会增加实际隔离。

## 3. 核心不变量

1. **一个活动 Session 一个 `SessionActor`**，它同时最多推进一个 Turn。
2. **OpenWork 自身的 Agent Loop 只有一处** —— `session/run_loop.rs`。Trace、Storage、Desktop 都不能推进 Turn。协作模式中，Computer 上的外部 Engine adapter 推进推理，V1 是 `OpenCodeAdapter`。`AgentRunner` 只交付 wake delta 并记录结果，见 [collaboration.md](collaboration.md)。
3. **Conversation 只有一个写者** —— `openwork-chat-state`。
4. **模型总是用户显式选择**（`providerId + model`），没有自动选择或跨模型 fallback。
5. **Trace 是 best-effort** —— Trace、队列或数据库失败时，不得让 Turn 失败。正文写入失败时，Span 本身仍须落库。
6. **同一份内容只有一个权威副本** —— 对 `messages` 已有的内容，Trace 只留指针，不复制。
7. **用户批准不能绕过执行期边界**——批准只能让这一次调用得到更宽的沙箱策略，不能跳过它。`ToolSessionContext` 的路径围栏约束文件工具，OS 沙箱约束 `bash`，两者读同一个 `SandboxPolicy`。在任何批准下，硬保护路径都不可写。见 [permissions.md §2](permissions.md)。

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
- **Sub-Agent** 是主 Agent 派生的只读从属 Session，见 [multi-agent.md](multi-agent.md)。它不是新的运行聚合，所有 Turn 语义与根会话完全相同；
- **协作模式** 是与工作台运行时不相交的 BYOA 子系统。Collaboration Server 保存业务事实，本机 Computer daemon 运行 Engine adapter（当前只有 `OpenCodeAdapter`）。每个协作 Agent 有显式 `engine_id`、主模型、triage 模型、私有 home、当前 RuntimeSession JWT 与 `AgentRunner`，见 [collaboration.md](collaboration.md)。它的 **Room**、**Computer daemon**、**协作 Agent** 与本篇的 Session、Sub-Agent 没有继承关系，不要混用；
- **Agent Message** 是子 Agent 回传给父的消息，以 `message_kind = 'agent_message'` 存在父的 Conversation 里，**永不触发 Turn**。

**不要重新引入的退役术语**：`StepId`、`ToolRunId`、`ApprovalId`、`TurnRecorderPort`、`JournalTurnRecorder`、Event Journal。

## 6. 当前非目标

工作台运行时不要添加：MCP、Memory、Artifact、Git/Diff、Worktree、跨进程未完成 Turn 恢复、Event Journal、有损压缩、后台任务恢复。

多智能体已从非目标移出，设计见 [multi-agent.md](multi-agent.md)。它**不引入新 crate、不新增 Trace kind、不新增 SessionUpdate 类型**。子 Agent 本身就是一个 Session，复用 `SessionActor` 与唯一的 Agent Loop。父子拓扑是 `sessions` 表的四个新列。按 `update_plan` 的先例，Core 拥有五个控制工具。子 Agent 的 Trace 独立成树，靠 `sessions.parent_session_id` / `spawn_span_id` 关联。

**范围严格限定在只读、单层、异步**。可写子 Agent、多层嵌套、角色文件加载、跨子 Agent 通信都仍是非目标。子 Agent 完成时只入队，不唤醒父会话，因此"后台任务恢复"仍在上面那行里。若某次改动要求新增 crate、Trace kind，或要求子 Agent 能写文件，先回到 multi-agent.md，确认设计是否偏离。

Skill 已从非目标移出，设计见 [skills.md](skills.md)。它**不引入新 crate、不新增 Trace kind**。目录放在 System Context 中，正文走 Conversation，启停偏好单独保存在 `skill_status`，资源与脚本复用 `read` / `bash`。用户在 Desktop 选择 `$name` 时，可见 token 与 `{ name, path }` 绑定分开。Tauri 把文本和显式选择编码为同一个有序 `Vec<UserInput>`。

Core 在接受 Turn 前，把 `UserInput::Skill` 解析为持久化的 contextual User-role Text。模型内容层不定义 Skill 专用类型，`ModelRequestBuilder` 和 provider adapter 只处理已有 ContentBlock。文件读取不移到 Bridge、Chat State 或 provider adapter。若某次改动要求新增 Skill crate 或 Trace kind，先回到 skills.md，确认设计是否偏离。

[collaboration.md](collaboration.md) 约束协作模式。它在 §1 的依赖图里是一条**独立分支**：`openwork-collab` 不依赖 `openwork-core`；其 `server/`、`computer/` 与 `protocol/` 按模块单向依赖。Engine 是每个 Agent 的显式领域属性。`AgentRunner` 通过 `EngineRegistry` 取得 adapter，当前生产 registry 只有 `OpenCodeAdapter`。以后接入 Codex 时，增加真实 adapter，不预留空实现或 capability 矩阵。

Provider 登录态只在对应本机 Engine 的 data root 中，协作分支不依赖 `openwork-credentials`。持久化数据放在 `collab_*` 表，短期协调放在 Redis，瞬时 Runner 状态放在 Computer 内存。三者都不碰 `openwork-core` migrations。

协作范围严格限定为本机 macOS、消息、群聊、Climate、看板和 Agenda，当前 Engine 只有 OpenCode。协作范围不含：Windows/Linux、远程 Mac、多 Computer assignment、系统后台常驻、MCP、审批、共享项目目录、Git/Diff、Worktree、Memory、Notes、Skills 或 reaction。未来的 Codex adapter 属于已经确认的 Engine seam，但只在真实接入时实现。若有人重新提出其他范围，先重新评审。不要因此让 Computer 直连数据库、让 Server 启动 Engine，或让 Desktop 承担业务规则。

工作目录只是一个路径值：Session 创建时确定，传给 `ToolSessionContext`。它不是独立的领域对象，因此没有 `openwork-workspace` crate。只有出现多个消费者共享的 Git、Sandbox 或 Checkpoint 能力时，才重新评估是否拆出。

## 7. 新增模块时的检查

每引入一个模块，回答六个问题：

1. 谁创建它；
2. 谁修改它；
3. 谁决定下一状态；
4. 谁持久化；
5. 它失败是否会改变主流程；
6. 是否产生反向依赖。

**若一个类型需要同时回答两个以上的 Owner，先拆职责再落地。** 最终，读者应能从目录结构直接读出一次 Turn 的完整控制路径，不需要在多个 crate 之间反复跳转。
