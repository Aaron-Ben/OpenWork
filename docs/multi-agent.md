# 多智能体

主 Agent 可以派生只读的子 Agent，让它回答代码库问题。子 Agent 异步回传结果。**唯一目的是上下文隔离**：把"翻二十个文件才能回答一个问题"的中间材料挡在主对话之外。

它不用于并行写代码，也不用于交叉验证。这两件事需要权限冒泡、写冲突纪律和失败回滚，不在本篇范围内。

## 1. 定位与边界

| 是什么 | 不是什么 |
|---|---|
| 子 Agent 是一个完整 Session，复用 `SessionActor` | 不是父 Turn 内的嵌套循环，不是新 crate |
| 只读：`read` / `grep` / `glob` / `list` / `bash` | 不能写文件，不能改工作区 |
| 只有一层：子 Agent 不能再派生 | 没有树、没有层级路径、没有兄弟寻址 |
| 异步：`spawn` 立刻返回，结果走 mailbox | 不阻塞父 Turn，也不自动唤醒父 Session |
| 模型无条件继承父会话 | 不能按角色或参数覆盖模型 |

**核心不变量：**

1. **子 Agent 就是一个 Session。** 它有自己的 `sessions` 行、`turns` 行、`messages` 行、ChatState、压缩状态和 Trace。`session/run_loop.rs` 不为多智能体增加任何分支。所以 [architecture.md §3](architecture.md) 的不变量 #2「Agent Loop 只有一处」继续成立。
2. **子 Agent 不可写。** 工具池里没有 `write` / `edit`。explorer 的模式上限是 `accept-edits`：它用 `bash` 写工作区时，内核拒绝写入，而且它无法请求越界。
3. **子→父的消息永不触发 Turn。** 这类消息只入队，父在自己的 Turn 内消费它。用户没有输入，就没有 Model Call。
4. **父子共享工作目录与沙箱后端，模式只收紧不放宽。** 子 Agent 用父的 `working_directory` 和沙箱后端构造 `ToolSessionContext`。生效模式取父会话生效模式与角色上限中较窄的一个，派生时取快照（[permissions.md §6.6](permissions.md)）。
5. **子 Agent 的 Trace 完全独立。** 它不与父共享 `trace_id`，也不设跨 Trace 的 `parent_span_id`。

## 2. 对象与所有权

```text
OpenWorkCore
├── Session Registry
│   ├── SessionActor（根）
│   │   ├── AgentControl ────────┐   同一个 Arc，父子共享
│   │   └── ActiveTurn?          │
│   └── SessionActor（子 agent）  │
│       └── AgentControl ────────┘
└── AgentControl 内部
    ├── Weak<建 Session 所需部件>   打断 Core → SessionActor → Core 的环
    ├── 子 Agent 注册表            task_name ↔ SessionId ↔ SessionHandle
    └── 活跃 Turn 计数             并发限额
```

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 子 Agent 的创建、注册、寻址 | `AgentControl` | 五个控制工具 |
| 并发名额 | `AgentControl` | RAII guard |
| 子 Agent 的 Conversation / Turn | 子 `SessionActor` | 与根会话完全相同的路径 |
| 父的 mailbox | 父 `SessionActor` | `DeliverAgentMessage` 命令 |
| 父子拓扑的持久事实 | `sessions` 表 | 递归查询 |

`AgentControl` 以 `Arc` 的形式放进 `SessionRuntimeConfig`。父创建子时，把**同一个实例**传下去。`AgentControl` 对 Core 的引用是 `Weak`，原因如下：`OpenWorkCore` 持有 `SessionHandle`，`SessionHandle` 背后的 Actor 持有 `AgentControl`。如果 `AgentControl` 再强引用 Core，引用就成环，进程退出时没有一个对象会析构。

**为什么不是父 `SessionActor` 自己管子 Session**：创建、注册表、限额、寻址是四份状态。如果把它们放进已经 898 行的 `SessionActor`，它就要同时回答 [architecture.md §7](architecture.md) 的多个 Owner 问题。

## 3. 身份

模型用 **`task_name`** 指代子 Agent。`task_name` 在父会话内唯一。不向模型暴露 UUID，没有昵称池，也没有 `/root/xxx` 路径：深度为 1 时，前缀只是噪声。

拓扑记在 `sessions` 的四个新列上：`parent_session_id` / `task_name` / `agent_role` / `spawn_span_id`。**DDL 与约束理由见 [data-model.md](data-model.md)，那里是唯一权威副本**。本节只说语义。

| 规则 | 说明 |
|---|---|
| 前三列同生共死 | 要么全 NULL（根会话），要么全非 NULL（子 Agent）。`spawn_span_id` 是例外，可以为空：Trace 写入失败不能阻塞 spawn |
| `task_name` 格式 | `^[a-z][a-z0-9_]{0,47}$`，与 Skill name 同风格。唯一索引拒绝重名，错误直接回给模型 |
| 深度上限 1 | **两道防线**：工具面不给子 Agent 注册 `spawn_agent`，模型看不见它；数据库 CHECK 兜底。只靠工具面不够：那是运行时决策，判断写错时就没有第二道防线 |
| 不进会话列表 | `list_sessions` 加 `WHERE parent_session_id IS NULL` |
| 级联删除 | 删除父会话时，现有的 `ON DELETE CASCADE` 同时删除子 Session 及其 `turns` / `messages` / `trace_spans` |

用户不能直接给子 Agent 发消息，也不能重命名或独立管理它。用户只能在父会话的子 Agent 面板中展开查看（[desktop.md §8](desktop.md)）。

## 4. 角色

V1 只有一个角色 `explorer`，**在 `openwork-agent` 里 hardcode**，不做文件加载。

```rust
AgentDefinition {
    name: "explorer",
    description: "回答关于代码库的具体、范围明确的问题",
    system_prompt: EXPLORER_SYSTEM_PROMPT,
    tool_names: ["read", "grep", "glob", "list", "bash"],
    sandbox_ceiling: SandboxMode::AcceptEdits,
    policy: AgentPolicy { max_model_calls: 15, doom_loop_threshold: 3 },
}
```

`sandbox_ceiling` 是角色允许的最宽沙箱模式，取值与用户模式相同：`AcceptEdits` / `Auto`。子 Agent 的生效模式取父会话生效模式与它中**较窄**的那个（[permissions.md §6.6](permissions.md)）。所以 explorer 在任何父会话下都是 `accept-edits`。它的工具面里没有 `write` / `edit`，而 `accept-edits` 下 bash 写不了工作区。因此对工作区来说，内核保证了"只读角色"。explorer 仍能写临时目录：here-document 之类需要临时文件的写法照常可用，工作区也不受影响。

**不为子 Agent 单设第三个模式。** `accept-edits` 加上没有写工具，已经等于"不能改工作区"。如果再加一个只能写 `/dev/null` 的取值，换来的只是临时目录不可写。代价是 here-document 失败，以及多一套要测试的规则。

参照物：codex 的 `explorer.toml` 是一个 **0 字节的空文件**，真正起作用的全是描述文本。在出现第二个角色之前，做目录发现、frontmatter 解析、启停状态和管理 UI，等于把 Skill 系统重做一遍，却得不到收益。

`EXPLORER_SYSTEM_PROMPT` 必须包含四件事，缺一不可：

1. **身份与产出形态**——你在回答一个被委派的具体问题；最终回答就是交付物，直接、完整、可独立阅读，不要说"我可以继续查"。
2. **只读边界**——你不能修改任何文件。
3. **`bash` 的边界在哪**——命令在不能写工作区的沙箱里执行：查看类命令（`git status` / `git log` / `git diff` / `rg` / `ls` / `cat` 等）直接可用，任何会写工作区的命令（包括 `cargo check` 写 `target/`）都会被拒绝，且你无法请求越界；临时目录可写。**不说清楚会产生一类沉默失败**：模型只能靠试错找边界，浪费轮次。
4. **不要反问**——没有人会回答你的澄清问题，信息不足时给出基于现有证据的最佳答案并标注不确定处。

## 5. 工具面

共有五个工具，全部由 **Core 拥有**，不注册进 `openwork-tools`。原因：它们操作 `AgentControl` 和 SessionActor 状态，不操作工作区。[`plan/tool.rs`](../crates/openwork-core/src/plan/tool.rs) 的 `update_plan` 已经走通这条路径，这里沿用同一形态：定义放在 `agent/tool.rs`，分派走 `TurnToolset::ResolvedTurnTool`。

| 工具 | 参数 | 语义 |
|---|---|---|
| `spawn_agent` | `task_name`, `message` | 创建子 Agent 并投递初始任务，**立刻返回** |
| `wait_agent` | `timeout_ms?` | 等本会话 mailbox 出现任何活动或超时 |
| `list_agents` | 无 | 列出直属子 Agent 及状态 |
| `followup_task` | `task_name`, `message` | 给已有子 Agent 追加任务并唤醒它 |
| `interrupt_agent` | `task_name` | 中断子 Agent 的当前 Turn |

**`spawn_agent` 不暴露 `model` / `reasoning_effort` / `agent_type` 参数。** 子 Agent 无条件继承父会话的模型。[architecture.md §3](architecture.md) 的不变量 #4 规定：模型总是用户显式选择的。让角色或模型自己挑模型，等于引入自动选择。V1 只有一个角色，所以 `agent_type` 也没有信息量。

**`wait_agent` 不接受 targets。** 它等"mailbox 有动静"，不等"某个具体 Agent 完成"。这样就没有"等 A 时 B 完成却唤不醒"的死锁，父也能立刻看到先完成的结果。超时范围是 min 10s / default 60s / max 10min，min 用于挡住忙轮询。max 不取 codex 的 1 小时：OpenWork 没有驻留卸载机制，卡住 1 小时只能靠用户取消 Turn。

**`wait_agent` 的返回值不含子 Agent 的回答。** 它只返回 `{ delivered: bool, timed_out: bool }`。回答内容通过 mailbox 排空进入 Conversation（§6.3）。如果 Tool Result 再复制一份，同一内容会在 Conversation 中出现两次，而且两份可能漂移。`update_plan` 只回 `"Plan updated"` 也是这个道理。

**砍掉的两个**：`send_message` 和 `close_agent`。`send_message` 给正在跑的 explorer 补充信息，但不让它开工，在只读场景下几乎无用。`close_agent` 也不需要：并发限额按活跃 Turn 数计算，空闲 Actor 不占名额；父会话卸载时级联关闭即可。

### 5.1 子 Agent 的工具面

子 Agent 拿不到这五个工具中的任何一个。它的 `TurnToolset` 只有 `read` / `grep` / `glob` / `list` / `bash`，**也没有 `update_plan`**。原因：子 Agent 只处理一个界定好的单一问题，任务清单是主 Agent 的职责。

### 5.2 触发策略

工具描述按**任务形态**引导模型，不要求用户显式授权。

codex 要求"除非用户或 AGENTS.md 明确要求委派，否则不要 spawn"。codex 这样保守有理由：它默认对所有用户开启，子 Agent 有全套可写工具，而且层级不限。OpenWork 的 explorer 只读、单层、并发 3，最坏情况只是多消耗一些 token。如果一个能力必须等用户想起来说一句话才能用，它就等于不存在。

描述文本必须同时给出肯定式和否定式。只写一半，行为会走向两种极端。`plan/tool.rs` 的注释已经记下这个教训：「仅提供工具定义不足以得到稳定行为」。

**该用**：有多个可独立回答的代码库问题；回答它们会产生大量不需要留在上下文里的中间材料。

**不该用**：单个小问题，自己查。下一步动作依赖这个结果时，不要委派，把关键路径留在本地。不要重复查自己已经查过的东西。不要在同一个未解决的问题上反复派工。

把并发上限写进描述文本，让模型自己控制节奏，而不是达到上限后再重试。

## 6. 通信

### 6.1 mailbox

父 `SessionActor` 持有一个内存 mailbox。新增一条命令：

```rust
SessionCommand::DeliverAgentMessage {
    task_name: String,
    kind: AgentMessageKind,   // FinalAnswer | Interrupted | Failed
    body: String,
},
```

这**改变了 [session-runtime.md §2](session-runtime.md) 的输入模型**：`StartTurn` 不再是唯一的输入来源。但它不改变 Turn 的定义：`DeliverAgentMessage` 永远只入队，永不创建 Turn。

mailbox **不持久化**。事实来源是子 Session 和它的 `turns` 行。内存队列只是"还没交给父"的缓冲。

### 6.2 信封

消息以 **user-role + `message_kind = 'agent_message'`** 进入 Conversation，格式与 Skill 的 contextual Message 同构：

```text
<agent_message>
<task>find_auth_flow</task>
<kind>final_answer</kind>
<body>
认证走 JWT，签发在 crates/api/src/auth/issue.rs:42 ……
</body>
</agent_message>
```

`kind` 取 `final_answer` / `interrupted` / `failed`。失败与中断也必须投递：父等不到任何东西，比拿到失败通知更糟。

**为什么是 user-role 而不是 assistant-role。** 语义上它确实更像 assistant，因为它不是用户说的话；codex V2 也选了 assistant，但 OpenWork 不能这样做。Anthropic adapter 直接透传 role（[`adapters/anthropic_messages/request.rs`](../crates/openwork-models/src/adapters/anthropic_messages/request.rs)）。mailbox 的排空点正好在组装 Model Request 之前，所以 assistant-role 消息会成为请求的最后一条。Anthropic 把它视为 **assistant prefill**，模型会从 `<body>` 的文字**接着往下写**，而不是把它当输入读。适配层也兜不住这个问题：`openwork-models` 不知道 Session 和 Agent 的存在，把多智能体的约束下沉到 provider adapter 是反向依赖。

另外三件事满足"不伪装成用户消息"这个诉求，它们与 role 无关：

| 诉求 | 手段 |
|---|---|
| Desktop 不渲染成用户气泡 | `canonicalItems` 已经只收 `messageKind === 'normal'`，非 normal 的消息自动被过滤 |
| 模型知道这不是用户说的 | 信封文本写明 `<task>` 与 `<kind>` |
| 压缩不把它当成用户请求 | 见下 |

**`last_real_user` 必须改。** 它现在从后往前找最后一条 `role == User && is_real()` 的消息（[`compaction/projection.rs`](../crates/openwork-core/src/session/compaction/projection.rs)）。Skill 的 contextual Message 之所以安全，纯粹因为它写在用户可见消息**之前**;而 agent message 在 Turn 中途到达，写在用户消息**之后**。所以 `last_real_user` 会选中 agent message，把它当作 last-user replay，并把真实用户请求从压缩投影里挤掉。

修法：给 `openwork-chat-state` 的 `ConversationItem` 增加 kind 维度，让 `last_real_user` 只认 `normal`。这不是绕路，而是修正：现有实现靠"写入顺序"保证正确性，这本身就脆弱。

### 6.3 排空

**规则一句话：每次组装 Model Request 之前无条件排空 mailbox。**

排空点只能在这里。provider 要求 `assistant(tool_calls)` 之后紧跟全部 `tool_results`，中间插入任何消息都会破坏配对。"所有 Tool Result 写完、下一次 Model Call 之前"正是 `run_loop` 每轮循环的顶部。Turn 开始时也走同一个点，所以不需要两套逻辑。

所以子 Agent 完成后，父**在同一个 Turn 内**就能看到结果，并继续工作。这是并行的全部价值所在。

**不建投递阶段状态机。** codex 有 `MailboxDeliveryPhase`，因为它有 review / compact 这类不可打断的独立 turn 类型。OpenWork 的循环是"无 Tool Call → 立即完成 Turn"。模型输出最终回答时，就没有下一次 Model Call，也就没有排空点。后到的消息自然留给下一个用户 Turn。

> **这条依赖一个必须显式维护的不变量：`run_loop` 中"无 Tool Call"必须立即终止 Turn，不得在其后再发起任何 Model Call。** 如果将来引入"Turn 结束前再问一次模型"之类的逻辑，这里的自动门控就失效。那时必须补上投递阶段状态机。

### 6.4 完成回传

子 Agent 的 Turn 到达终态时，**由子 `SessionActor` 自己**通过 `AgentControl` 向父投递。不起后台 watcher：回传发生在子 Agent 自己的执行上下文里，可以顺带写自己的 Trace，而且每个子 Agent 省一个 tokio task。

```text
子 Turn 终态
  → 组装 <agent_message> 信封
  → AgentControl 找到父 SessionHandle
  → SessionCommand::DeliverAgentMessage
  → 父 mailbox 入队（永不触发 Turn）
```

这时父会话可能处于以下状态：

| 父的状态 | 结果 |
|---|---|
| 正在跑 Turn，且还会有下一次 Model Call | 下一轮循环顶部排空，**同 Turn 内可见** |
| 正在跑 Turn，但已输出最终回答 | Turn 已结束，留给下一个用户 Turn |
| 空闲 | 留在队列，下一个用户 Turn 开始时排空 |

**父空闲时不自动拉起 Turn。** [architecture.md §5](architecture.md) 把 Turn 定义为"一次用户输入触发的完整 Agent Loop"。自动唤醒会产生用户没有授权的 Model Call 和费用。它还会引入"后台任务"语义，而 §6 明确把这个语义列为非目标。

## 7. 限额与非交互

### 7.1 并发

限额按**同时活跃的 Turn 数**计算，默认 3，不含父会话自己。空闲的子 Agent 不占名额，因为它要留给 `followup_task` 复用。

实现用 RAII guard：子 Agent 开始 Turn 时取一个 guard，Turn 结束（含失败、取消）时 Drop 归还。**不要写"成功路径减一"**：spawn 与 Turn 启动都有多个提前返回点，人工配平一定会漏。

第 4 个 `spawn_agent` 返回错误 Tool Result（`agent_limit_reached`）。模型自行决定等待，还是收缩范围。不做累计总数限额。父的 `max_model_calls`（默认 20）已经间接限制了单个 Turn 内能 spawn 多少次。"连续问不同问题"是正常的使用方式，按累计数封顶会误伤这种用法。

### 7.2 非交互 Session

子 Agent 是**没有人在环路里**的会话。`SessionRuntimeConfig` 增加：

```rust
pub enum SessionApproval {
    Interactive,      // 根会话：Ask 挂起等用户
    NonInteractive,   // 子 Agent：Ask 立即 Deny
}
```

这**不是一个沙箱模式**。沙箱模式决定命令能写哪里；这个字段决定"需要问的时候有没有人可问"，而答案是没有。沙箱、四档路径、硬保护全部照旧，只有"需要问"的落地方式改变（[permissions.md §6.6](permissions.md)）：

| 本该出卡片的情况 | 非交互 Session |
|---|---|
| 越界请求 | 立即返回 `denied` 的 Tool Result |
| 危险命令检测命中 | 同上 |
| 沙箱不可用 | bash 不执行（与交互式 Session 相同，不是特例） |

拒绝附带一句可操作的说明：子 Agent 无法请求授权，在当前模式内能做什么，做不到的事报告给父 Agent。模型收到后能自行换做法继续，不会卡死。

所以 explorer 拿着 `bash` 也是安全的。它在 `accept-edits` 沙箱里执行，`git log` / `git diff` / `rg` / `ls` / `cat` 直接运行。它写工作区时，内核拒绝写入；它写工作区外临时目录以外的位置时，内核同样拒绝。而且它无法越界。**边界是沙箱，不是一份命令清单。**

## 8. 重启对账

进程重启时，`mark_running_interrupted()` 把所有 `running` 的 Turn 改成 `interrupted`。**子 Agent 的 Session 也在其中**，所以子 Agent 必然死掉。父的内存 mailbox 同时清空。

父会话**下一次由用户触发的 Turn 开始时**，对直属子 Agent 做一次幂等对账：

| 子 Agent 状态 | 未交付时补发 |
|---|---|
| `completed` | `final_answer` 信封，正文取子 Session 最后一条 assistant 消息 |
| `interrupted` | `interrupted` 信封 |
| `failed` / `cancelled` | `failed` 信封，带 `turns.error_code` |

幂等靠**确定性消息 ID**：`agent-msg:{child_session_id}:{child_turn_id}:{kind}`。`messages.id` 是主键，所以 `INSERT ... ON CONFLICT DO NOTHING` 直接得到幂等，不需要先查再写。

**对账不启动父 Turn**，只在已经开始的 Turn 里补消息。它也不"恢复"任何东西：被中断的子 Agent 不重新执行。这与 [session-runtime.md §10](session-runtime.md) 处理悬空 Tool Call 的原则相同：修复 Conversation 的可理解性，而不是恢复执行。

## 9. Trace 与 Desktop

### 9.1 Trace

子 Agent 的 Trace **完全独立**，采用 [trace.md §16](trace.md) 已经写下的形态：子 Turn 的 `trace_id` 等于子 `turn_id`，不与父共享，也不设跨 Trace 的 `parent_span_id`。

两条硬理由：

- **共享 `trace_id` 会破坏完整度派生。** [trace.md §12](trace.md) 的 expected 来自 `turns.model_submission_count`，captured 是 `kind='model_call' AND parent_span_id IS NULL` 的 Span 数。子 Agent 的 Model Call 在它自己的 Turn 里是顶层 Span。如果混进父 Trace，captured 会恒大于 expected，**每个用了子 Agent 的正常 Turn 都被误判成 `Partial`**。摘要采样子 Span 当年也出过完全相同的问题。
- **跨 Trace 的 `parent_span_id` 会误报 orphan。** orphan 的定义是"指向同一 Trace 中不存在的 Span"。子 Agent 的根 Span 如果指向父 Trace，就永远是 orphan。

业务列表达关联关系：`sessions.parent_session_id` 给出拓扑，`sessions.spawn_span_id` 给出"哪个 Tool Call Span 发起了它"。Trace UI 要跳转时，用这两列，不用 Span 指针。`spawn_span_id` **不建外键**：Trace 是 best-effort，那个 Span 可能因队列满而根本没有落库。

父侧照常：`spawn_agent` / `wait_agent` 等五个工具各产生一个普通的 `tool_call` Span。**不新增 Trace kind。**

### 9.2 Desktop

**不新增 SessionUpdate 类型。** 子 Session 用 `spawn_with_global_updates` 启动。它的 Update 带自己的 `sessionId`，走现有的全局事件流，前端已有的分发机制直接可用。

父会话的子 Agent 面板按 `parent_session_id` 找到子 Session，并读取对应的 per-session runtime view 来渲染状态。V1 只展示**运行状态、终态和最终摘要**，不内嵌子 Agent 的 `text_delta` / `reasoning` / `tool_call` 流。前端 reducer 直接丢弃子会话的 delta，这样就不必为了一个折叠面板维护 N 份完整 runtime state。

点开详情时，调用 Tauri Command 读取子 Session 的持久化 `messages` 和 snapshot，展示完整 transcript。详情**只读**。

## 10. 明确不做

| 不做 | 理由 |
|---|---|
| 可写子 Agent / worker 角色 | 需要权限冒泡、write set 划分、失败回滚，是另一个量级的工程 |
| 多层嵌套 | 上下文隔离场景下，二层没有价值；它会立刻引入路径解析、兄弟寻址、级联传递闭包 |
| 角色文件加载（`~/.openwork/agents/*.md`） | 只有一个角色时，这等于把 Skill 系统重做一遍。有了第二、第三个角色再说 |
| 用户显式触发（`@explorer` 选择器） | 只有一个角色时，显式触发能携带的信息量只有"用/不用"一个 bit，而 §5.2 的策略文本正在回答这个问题。有了多个角色后，按 `$skill` 的成熟路径加 `UserInput::Agent { role }`。`UserInput` 是 `#[non_exhaustive]` 的，届时加变体不破坏兼容 |
| `send_message` / `close_agent` | 只读单层场景下，真实使用频率存疑 |
| mailbox 持久化 | 子 Agent 重启后必死。能持久化的只是"重启前已完成但未消费"这个窄窗口，§8 的对账已经覆盖它 |
| 驻留 LRU 卸载 | codex 需要它，因为 codex 有持久 rollout 和大规模场景。并发卡在 3 时，空闲 Actor 的开销可以忽略 |
| fork 父历史 / prompt cache 复用 | 收益真实，但属于第二梯队。子 Agent 从空历史 + 一条任务描述开始 |
| 子 Agent 之间通信 | 深度 1 下，它们互不知道对方存在，也不该知道 |
| 独立的 `openwork-subagent` crate | 与 CLAUDE.md「设计原则」的“避免投机抽象”和 [architecture.md §2](architecture.md)「只有多个消费者共享时才拆 crate」冲突 |

## 11. 分期与验收

### P0 — 骨架

迁移加列与约束；`message_kind` 增加 `'agent_message'`；`ConversationItem` 增加 kind 维度并修正 `last_real_user`；`AgentControl` 类型、注册表与并发 guard；`SessionApproval` 字段。

1. 现有全部测试通过，行为无变化；
2. `list_sessions` 不返回 `parent_session_id IS NOT NULL` 的行；
3. 唯一索引拒绝同一父会话下重复的 `task_name`；
4. CHECK 拒绝不合格式的 `task_name`；
5. 单测能通过 `AgentControl` 创建一个子 Session、跑完一个 Turn、读到它的终态；
6. `last_real_user` 跳过 `agent_message` 与 `skill_instruction`，只认 `normal`。

### P1 — 通信

`DeliverAgentMessage` 命令；`run_loop` 排空点；信封构造；子 Turn 终态回传。

7. 子 Agent 完成后，父在**同一个 Turn 内**的下一次 Model Request 里看到 `<agent_message>`；
8. 排空发生在全部 Tool Result 之后。Anthropic 与 OpenAI 两条 adapter 组装出的请求都合法（末尾不是 assistant）；
9. 父已输出最终回答（无 Tool Call）后，到达的消息不进入本 Turn，留到下一个用户 Turn；
10. 子 Agent 失败与被中断时同样产生信封，父不会无限等待；
11. 压缩发生时，`last-user replay` 仍是真实用户请求，`agent_message` 不会挤掉它。

### P2 — 工具面

五个控制工具；`explorer` 角色；非交互 Ask-Deny；策略文本。

12. 端到端：主 Agent 并行派出 3 个 explorer，并汇总结果；
13. 第 4 个 `spawn_agent` 返回 `agent_limit_reached`，模型能继续；
14. 空闲子 Agent 不占并发名额，`followup_task` 能复用它；
15. explorer 调用 `cargo check` 时，因为写 `target/`，沙箱拒绝执行，结果带拒绝标记；explorer 带越界请求重试时，直接收到拒绝和可操作说明；
16. explorer 调用 `git log` / `rg` 时，命令在沙箱内直接执行，不产生审批卡片。父会话为 `auto` 时，explorer 的生效模式仍是 `accept-edits`，内核拒绝它写工作区；
17. 子 Agent 的工具面里没有 `write` / `edit` / `spawn_agent` / `update_plan`；
18. `wait_agent` 超时返回后，模型可以继续，也可以再次 wait。

### P3 — 对账与展示

重启对账；Desktop 子 Agent 面板。

19. 重启后，父的下一个用户 Turn 补发完成或中断通知，且**只发一次**（重复对账不产生第二条消息）；
20. 对账不自动启动父 Turn；
21. 面板列出直属子 Agent 及状态，点开后可以读完整 transcript；
22. 删除父会话时，子 Session 及其 `turns` / `messages` / `trace_spans` 一并消失。

### 贯穿全程

23. 子 Agent 的 Trace 与父的 `trace_id` 不同，父 Turn 的完整度仍判定为 `Complete`；
24. `session/run_loop.rs` 除排空点外，没有为多智能体新增分支。
