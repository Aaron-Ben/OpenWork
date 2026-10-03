# 多智能体

本页描述主 Agent 怎样派生只读的子 Agent，以及子 Agent 的结果怎样回到主对话。控制面、mailbox、对账与存储由 `openwork-core` 负责；`explorer` 角色定义在 `openwork-agent`；Message 的 kind 由 `openwork-chat-state` 定义。

子 Agent 的权限模式见 [permissions.md §13.3](permissions.md)，Trace 形态见 [trace.md](trace.md) §16，Desktop 视图见 [desktop.md](../desktop.md) §8。

## 1. 边界

| 事实 | 代码 |
|---|---|
| 子 Agent 是一个完整的 Session，有自己的 `sessions`、`turns`、`messages` 行、ChatState、压缩状态与 Trace | `OpenWorkCore::build_session_handle`（`core.rs`） |
| 子 Agent 只有 `read` / `grep` / `glob` / `list` / `bash` | `explorer_definition`（`openwork-agent/src/explorer.rs`） |
| 只有一层：子 Agent 的工具面里没有控制工具 | `ControlToolSurface::SubAgent`（`session/toolset.rs`） |
| `spawn_agent` 立刻返回，结果经父的 mailbox 到达 | `AgentControl::spawn`（`agent/control.rs`） |
| 子 Agent 的模型等于派生时父会话的 `default_model_id` | `SubAgentHost::start_sub_agent`（`core.rs`） |

不变量：

1. 子 Agent 与根会话走同一个 run loop。差异只来自 `TurnToolset` 的工具面与 `SessionApproval`。
2. 子 Agent 改不了工作区。它没有写文件的工具，bash 在 `accept-edits` 沙箱里写不了工作区。
3. 子 Agent 发给父的消息只入队，**永不创建 Turn**。
4. 子 Agent 用父会话的工作目录与进程级沙箱后端。它的生效模式在派生时取快照（[permissions.md §13.3](permissions.md)）。
5. 子 Agent 的 Trace 与父 Trace 互相独立（[trace.md](trace.md) §16）。

用途与范围的理由见 [Agent Note：只读、单层的子 Agent](../../.agents/notes/implemented/architecture/2026-08-08-read-only-single-level-sub-agents.md)。

## 2. 对象与所有权

```text
OpenWorkCore
├── sessions              SessionId → SessionHandle（根与子都在这里）
├── agent_controls        根 SessionId → AgentControl
└── AgentControl（每个根会话一个，父子共享同一实例）
    ├── Weak<dyn SubAgentHost>   由 OpenWorkCore 实现
    ├── SubAgentRegistry         task_name → SessionId
    └── TurnSlots                活跃子 Turn 计数
```

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 子 Agent 的创建、寻址 | `AgentControl` | 五个控制工具 |
| 并发名额 | `AgentControl` 的 `TurnSlots` | `TurnSlot` guard |
| 子 Agent 的 Conversation 与 Turn | 子 `SessionActor` | 与根会话相同的路径 |
| 父的 mailbox | 父 `SessionActor` | `SessionCommand::DeliverAgentMessage` |
| 父子拓扑的持久事实 | `sessions` 表 | `list_sub_agent_sessions` |

- `AgentControl` 克隆后共享同一份状态。`SessionRuntimeConfig.agent_control` 在根与它的子 Session 中是同一个实例。
- `AgentControl` 以 `Weak` 持有 Core，打断 Core → `SessionHandle` → `SessionActor` → `AgentControl` 的引用环。Core 不在时，控制工具返回 `agent_host_unavailable`。
- 注册表只存 `task_name` 到 `SessionId` 的索引，不存 `SessionHandle`。handle 只在 Core 的 `sessions` 里。
- Core 首次为根会话建 `AgentControl` 时，从 `sessions` 表恢复它的全部子 Agent 身份。恢复不启动 Session，也不占名额。
- 卸载或删除根会话时，Core 同时关闭它的子 Session，并移除它的 `AgentControl`。

## 3. 身份与拓扑

模型用 `task_name` 指代子 Agent。`task_name` 在一个根会话内唯一，格式是 `^[a-z][a-z0-9_]{0,47}$`。模型看不到 `SessionId`。

拓扑记在 `sessions` 的四列：`parent_session_id`、`task_name`、`agent_role`、`spawn_span_id`。DDL 见 [data-model.md](../data-model.md) §5。

| 规则 | 由谁保证 |
|---|---|
| 前三列要么全为空（根会话），要么全不为空（子 Agent）。`spawn_span_id` 可以为空 | CHECK `sessions_subagent_fields_consistent` |
| `task_name` 格式 | `AgentControl::spawn` 与 `validate_sub_agent_session` 先校验；CHECK `sessions_task_name_format` 兜底 |
| 同一父会话下 `task_name` 不重复 | 注册表在建行之前占用名字；唯一索引 `uq_sessions_parent_task_name` 兜底 |
| 深度上限 1 | 子 Agent 的工具面没有 `spawn_agent`；`AgentControl::spawn` 总以根会话为父；`create_sub_agent_session` 拒绝以子 Agent 为父。数据库只用 CHECK `sessions_spawn_not_self` 拒绝自引用 |
| 子 Agent 不进顶层会话列表 | `list_sessions` 带 `WHERE parent_session_id IS NULL` |
| 删除父会话时删除子 Session 及其 `turns`、`messages`、`trace_spans` | 外键 `ON DELETE CASCADE` |

`spawn_span_id` 是发起它的 `spawn_agent` Tool Call Span，不建外键。

Desktop 只提供只读详情，不提供给子 Agent 发消息、重命名或单独管理的入口（[desktop.md](../desktop.md) §8.2）。Core 的 `start_turn` 与 `rename_session` 不检查目标是不是子 Session。

## 4. 角色

只有一个角色 `explorer`，定义写死在 `openwork-agent/src/explorer.rs`：

```rust
AgentDefinition {
    name: "explorer",
    description: "回答关于代码库的具体、范围明确的问题",
    system_prompt: EXPLORER_SYSTEM_PROMPT,
    tool_names: ["read", "grep", "glob", "list", "bash"],
    policy: AgentPolicy { max_model_calls: 15, doom_loop_threshold: 3 },
    sandbox_ceiling: SandboxMode::AcceptEdits,
}
```

- `AgentControl::spawn` 写入的 `agent_role` 固定为 `"explorer"`。
- 生效模式是 `sub_agent_mode(parent)`，即父会话模式与 `sandbox_ceiling` 中较窄者，所以 explorer 总是 `accept-edits`。它不能请求越界（[permissions.md §13.3](permissions.md)）。
- explorer 的工具面没有根会话才有的 `conversation_history` 回读工具。

`EXPLORER_SYSTEM_PROMPT` 告诉模型四件事：

1. 最终回答直接交给父 Agent，要独立可读，并给出代码位置或命令输出。
2. 它的任务是调查，不是改仓库；它没有改文件的工具。
3. bash 的沙箱不让它写工作区，临时目录可写；被沙箱拒绝的调用没有人能批准。
4. 不要提问。信息不足时，写明所用假设，并标出不确定处。

沙箱模式不写进提示词，由 world state 给出（[permissions.md §11](permissions.md)）。

只设 `accept-edits` 上限、不设第三个模式的理由见 [Agent Note：两个权限模式](../../.agents/notes/implemented/architecture/2026-09-24-two-permission-modes.md)。只有一个硬编码角色的理由见 [Agent Note：只读、单层的子 Agent](../../.agents/notes/implemented/architecture/2026-08-08-read-only-single-level-sub-agents.md)。

## 5. 控制工具

五个工具由 Core 定义（`agent/tool.rs`），不注册进 `openwork-tools`。run loop 用 `ResolvedTurnTool::Agent` 分派，执行在 `run_agent_tool`（`session/run_loop/mod.rs`）。它们不经沙箱，Trace 记来源 `control_tool`（[permissions.md §14](permissions.md)）。

| 工具 | 参数 | 成功时的 Tool Result |
|---|---|---|
| `spawn_agent` | `task_name`, `message` | `{"task_name":"<name>"}`；子 Turn 已开始 |
| `wait_agent` | `timeout_ms`（可选） | `{"delivered":<bool>,"timed_out":<bool>}` |
| `list_agents` | 无 | `[{"task_name","status","started_at"}]`，按 `task_name` 排序 |
| `followup_task` | `task_name`, `message` | `Follow-up task started` |
| `interrupt_agent` | `task_name` | `Interrupt requested` |

- 参数 schema 都不接受未声明的字段。`task_name` 带格式 pattern，`message` 不能为空白。
- `spawn_agent` 没有 `model`、`reasoning_effort`、`agent_type` 参数。
- `list_agents` 的 `status` 取 `idle`、`running`、`completed`、`failed`、`cancelled`，来自子 Session 当前的快照。
- `followup_task` 在已有的子 Session 上开始一个新 Turn。子 Agent 正在运行时，它返回 `agent_turn_start_failed`。
- `interrupt_agent` 取消子 Agent 当前的 Turn。子 Agent 没有活跃 Turn 时，它返回 `agent_interrupt_failed`。

领域错误写成失败的 Tool Result，正文是 `<code>: <message>`，父 Turn 继续：

| code | 情况 |
|---|---|
| `invalid_task_name` | `task_name` 不合格式 |
| `duplicate_task_name` | 名字已被本根会话占用 |
| `unknown_agent` | 没有这个 `task_name` |
| `agent_limit_reached` | 活跃子 Turn 已达上限（§7） |
| `agent_host_unavailable` | Core 已不存在 |
| `agent_start_failed` / `agent_turn_start_failed` | 建 Session 或开始 Turn 失败 |
| `agent_inspection_failed` / `agent_interrupt_failed` | 读快照或取消失败 |

`spawn_agent` 失败时，已占用的名字与名额都被释放。

### 5.1 `wait_agent`

- `timeout_ms` 默认 60,000，范围 10,000 到 600,000。超出范围返回参数错误。
- 等待的是本会话的 mailbox 出现任意一条消息，不针对某个子 Agent。mailbox 已有消息时立即返回。
- 返回值不含消息正文。正文在下一次 Model Call 前进入 Conversation（§6.3）。
- 父 Turn 被取消时，等待立即结束，Turn 以取消结束。
- `wait_agent` 不参与“相同调用重复”的 doom loop 计数。连续超时达到父会话的 `doom_loop_threshold`（3）时，Turn 以 `doom_loop` 失败。一次成功的等待把超时计数清零。

### 5.2 子 Agent 的工具面

子 Agent 拿不到五个控制工具中的任何一个，也拿不到 `update_plan`。`TurnToolset` 既不广告它们，也不分派它们。

### 5.3 模型看到的委派规则

根会话的系统提示词末尾追加 `agent_prompt_rules`（`agent/tool.rs`）的原文：

```text
## Sub-agent delegation

You can run up to 3 read-only explorer turns concurrently. Delegate when you have multiple specific, bounded codebase questions that can proceed independently, especially when their intermediate research should stay out of the parent context. Keep doing useful parent-side work while explorers run. Handle a single small question locally. Keep critical-path research local when your next action depends on its result. Do not delegate work you already investigated or repeatedly delegate the same unresolved question. Use wait_agent only when waiting is the next useful action; delivered messages appear before the next model call. Reuse an idle explorer with followup_task when its existing context helps.
```

工具描述同时写“该用”与“不该用”。`spawn_agent` 与 `followup_task` 的描述写明并发上限。数字 3 来自 `AgentControl::max_active_turns`。

工具集合、触发策略与等待上限的理由见 [Agent Note：控制工具与并发上限](../../.agents/notes/implemented/architecture/2026-08-08-sub-agent-tools-and-concurrency.md)。

## 6. 通信

### 6.1 mailbox

父 `SessionActor` 持有一个内存队列 `AgentMailbox`（`session/agent_message.rs`）。`SessionHandle::deliver_agent_message` 发送：

```rust
SessionCommand::DeliverAgentMessage { message: AgentMessage }

struct AgentMessage { id: String, task_name: String, kind: AgentMessageKind, body: String }
enum AgentMessageKind { FinalAnswer, Interrupted, Failed }
```

- 这条命令只把消息放进队列，永不创建 Turn。父会话空闲时，消息留到下一个用户 Turn。
- 队列不持久化。持久的事实来源是子 Session 与它的 `turns` 行（§8）。

### 6.2 信封

消息以 User role、`message_kind = 'agent_message'` 进入 Conversation：

```text
<agent_message>
<task>find_auth_flow</task>
<kind>final_answer</kind>
<body>
Authentication is implemented in crates/api/src/auth.rs.
</body>
</agent_message>
```

- `kind` 取 `final_answer`、`interrupted`、`failed`。
- 消息 ID 是 `agent-msg:{child_session_id}:{child_turn_id}:{kind}`。
- 入队前，`deliver_agent_message` 估算信封的 token 数。**达到 4,000 token 的消息被拒绝**，返回 `agent_delivery_failed`，不入队。

### 6.3 排空

**run loop 在每次 Model Call 之前排空 mailbox**，包括 Turn 的第一次 Model Call。

- 排空点在上一轮全部 Tool Result 写入之后，所以不会拆开 `assistant(tool_calls)` 与它的 `tool_results`。
- 每条消息先用 `INSERT ... ON CONFLICT (id) DO NOTHING` 写进 `messages`，写入成功才追加进 ChatState。同一 ID 的第二条消息被丢弃。
- 子 Agent 在父 Turn 运行中完成时，父在同一 Turn 的下一次 Model Request 里看到信封。
- 父 Turn 输出最终回答后，不再有 Model Call，也就没有排空点。之后到达的消息留给下一个用户 Turn。这依赖 [session-runtime.md](session-runtime.md) §4 的第 7 条性质。

### 6.4 终态回传

子 Turn 结束时，子 `SessionActor` 在处理 `RunnerEvent::Finished` 时自己投递，不另起 watcher：

```text
子 Turn 终态 → deliver_terminal_outcome → AgentControl::deliver_to_parent
  → 父 SessionHandle::deliver_agent_message → 父 mailbox 入队
```

| 子 Turn 结果 | `kind` | `body` |
|---|---|---|
| `Completed { final_text }` | `final_answer` | `final_text` |
| `Failed { code, message }` | `failed` | `<code>: <message>` |
| `Cancelled`（来自 `interrupt_agent`） | `interrupted` | `Sub-agent turn was interrupted.` |

投递失败时（父会话已不存在、信封超过 4,000 token），子 Agent 写一条 `warn` 日志，子 Turn 的结果不变。

| 父的状态 | 结果 |
|---|---|
| Turn 运行中，还会有下一次 Model Call | 下一次 Model Call 前排空，同 Turn 可见 |
| Turn 已输出最终回答 | 留给下一个用户 Turn |
| 空闲 | 不创建 Turn；下一个用户 Turn 开始时排空 |

### 6.5 kind 的作用

`MessageKind::AgentMessage` 是 contextual kind（`openwork-chat-state/src/item.rs`）：

- 压缩选择 last-user replay 时，`last_real_user`（`session/compaction/compacted_view.rs`）跳过 contextual kind，只认 `Normal`。agent message 写在用户请求之后，也不会被当成用户请求重放。
- Desktop 的 `canonicalItems` 只渲染 `messageKind === 'normal'` 的消息，agent message 不显示为用户气泡。

投递方式的理由见 [Agent Note：mailbox 投递](../../.agents/notes/implemented/architecture/2026-08-08-sub-agent-mailbox-delivery.md)。

## 7. 并发与非交互

### 7.1 并发

- 上限按同时活跃的子 Turn 数计算，默认 3（`DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS`），不含父会话自己。
- 名额属于一个根会话的 `AgentControl`。不同根会话各有各的名额。
- `spawn_agent` 与 `followup_task` 开始子 Turn 前取一个 `TurnSlot`。名额满时返回 `agent_limit_reached`，父 Turn 继续。
- `TurnSlot` 存在子 Session 的活跃 Turn 里，Turn 以任何方式结束都会归还它。
- 空闲的子 Agent 不占名额，可以用 `followup_task` 再次使用。
- 没有累计总数上限。父 Turn 的 `max_model_calls`（默认 20）间接限制一个 Turn 内的派生次数。

### 7.2 非交互 Session

子 Session 的 `SessionApproval` 是 `NonInteractive`。需要卡片的调用直接拒绝，拒绝文本说明出路。规则与原文见 [permissions.md §13.3](permissions.md)。

## 8. 对账

`OpenWorkCore::start_turn` 在每个用户 Turn 开始前，对直属子 Agent 做一次对账（`reconcile_sub_agent_sessions`）。对账在一个锁住父 `sessions` 行的事务里查询：

1. 删除没有任何 Turn 的子 Session，即中途失败的派生。Core 同时释放它们的 `task_name`，并关闭它们的 actor。
2. 找出已结束、但父 `messages` 里没有对应消息 ID 的子 Turn，按下表补发进父的 mailbox。

| 子 Turn 状态 | `kind` | `body` |
|---|---|---|
| `completed` | `final_answer` | 该 Turn 最后一条 assistant 消息的文本；为空时是 `Sub-agent completed without a persisted final response.` |
| `interrupted` | `interrupted` | `Sub-agent was interrupted by a process restart and is no longer available. Spawn a new explorer if this result is still needed.` |
| `cancelled` | `failed` | `cancelled: Sub-agent turn was cancelled.` |
| `failed` | `failed` | `<error_code>: <error_message>` |

- 对账靠消息 ID 判断是否已投递。同一结果在 mailbox 中出现两次时，§6.3 的 `ON CONFLICT` 只写入一条。
- 实时回传把 `Cancelled` 记为 `interrupted`，对账把 `cancelled` 记为 `failed`，两者的消息 ID 不同。
- 补发的消息也做 §6.2 的 4,000 token 检查。检查失败时，`start_turn` 返回错误。
- 对账不创建父 Turn，也不重新执行被中断的子 Agent。

进程启动时，`mark_running_interrupted` 把所有 `running` 的 Turn 改为 `interrupted`，子 Turn 也在其中。内存中的 mailbox 随进程消失，由对账补发。悬空 Tool Call 的修复见 [session-runtime.md](session-runtime.md) §10。

## 9. Trace 与 Desktop

- 子 Turn 的 `trace_id` 等于子 `turn_id`，不设跨 Trace 的 `parent_span_id`。父侧五个控制工具各产生一个普通的 `tool_call` Span。规则与理由见 [trace.md](trace.md) §16。
- 不新增 Session Update 类型。子 Session 用 `SessionHandle::spawn_with_global_updates` 启动，它的 Update 带自己的 `sessionId`，走全局事件流。
- Desktop 对子 Session 只保留状态，丢弃草稿、工具详情、Permission 与 Plan 类 Update。右栏与只读详情见 [desktop.md](../desktop.md) §8。

## 10. 验收

编号沿用原设计文档 §11。测试路径相对 `crates/`，前端测试写出文件与用例名。带 Postgres 的测试需要 `TEST_DATABASE_URL`。

1. 根会话的四个拓扑列全为空；子 Agent 的 `parent_session_id`、`task_name`、`agent_role` 全不为空。
   - 测试：`openwork-core/tests/postgres_sub_agent_sessions.rs::a_sub_agent_session_records_its_parent_task_name_and_role`
   - 缺口：没有测试直接写入半填充的行来触发 CHECK `sessions_subagent_fields_consistent`。
2. `list_sessions` 不返回 `parent_session_id IS NOT NULL` 的行；父会话能列出它的子 Agent。
   - 测试：`openwork-core/tests/postgres_sub_agent_sessions.rs::list_sessions_hides_sub_agents_but_the_parent_can_list_them`；`desktop/src/features/sessions/sessionStore.test.ts › "loads child canonical history without adding the child to the top-level session order"`
3. 同一父会话下重复的 `task_name` 被拒绝；不同父会话可以同名。
   - 测试：`openwork-core/src/agent/control.rs::duplicate_task_names_are_refused_without_reaching_the_host`；`openwork-core/tests/postgres_sub_agent_sessions.rs::a_duplicate_task_name_under_the_same_parent_is_rejected`
4. 不合格式的 `task_name` 在建行之前被拒绝；48 个字符是上限。
   - 测试：`openwork-core/src/agent/control.rs::malformed_task_names_are_refused_before_reaching_the_host`；`openwork-core/src/agent/control.rs::the_longest_accepted_name_is_forty_eight_characters`；`openwork-core/tests/postgres_sub_agent_sessions.rs::malformed_task_names_are_rejected`
   - 缺口：存储层先于数据库校验，没有测试触发 CHECK `sessions_task_name_format`。
5. 通过 `AgentControl` 能创建子 Session、跑完一个 Turn，并读到它的终态。
   - 测试：`openwork-core/tests/session_runtime.rs::parent_spawns_three_explorers_and_aggregates_their_deliveries`；`openwork-core/tests/postgres_core_host_flow.rs::production_host_persists_and_starts_an_idle_explorer_session`
   - 缺口：前者用测试 host，后者经真实 Core 建 Session 但不跑 Turn。
6. `last_real_user` 跳过 `agent_message` 与 `skill_instruction`，只认 `normal`。
   - 测试：`openwork-core/src/session/compaction/compacted_view.rs::an_agent_message_never_displaces_the_real_user_request`；`openwork-core/src/session/compaction/compacted_view.rs::a_skill_instruction_is_not_the_user_request_either`；`openwork-core/src/session/compaction/compacted_view.rs::a_conversation_with_only_contextual_user_items_has_no_user_request`；`openwork-chat-state/src/item.rs::contextual_kinds_stay_real_but_are_not_user_requests`
7. 子 Agent 完成后，父在同一个 Turn 内的下一次 Model Request 里看到 `<agent_message>`。
   - 测试：`openwork-core/tests/session_runtime.rs::agent_message_is_persisted_after_tool_results_and_seen_by_the_same_turn`；`openwork-core/tests/session_runtime.rs::parent_spawns_three_explorers_and_aggregates_their_deliveries`
8. 排空发生在全部 Tool Result 之后；Anthropic 与 OpenAI 的请求末尾是 user，不是 assistant。
   - 测试：`openwork-core/tests/session_runtime.rs::agent_message_is_persisted_after_tool_results_and_seen_by_the_same_turn`；`openwork-models/src/adapters/anthropic_messages/request.rs::agent_message_after_tool_results_is_a_user_input_not_an_assistant_prefill`；`openwork-models/src/adapters/openai_responses/request.rs::agent_message_after_tool_results_is_the_last_user_input_item`
   - 缺口：OpenAI Chat Completions adapter 没有对应测试。
9. 父已输出最终回答后到达的消息不进入本 Turn，也不创建 Turn，留到下一个用户 Turn。
   - 测试：`openwork-core/tests/session_runtime.rs::an_idle_parent_does_not_start_a_turn_and_consumes_mail_on_the_next_user_turn`
10. 子 Agent 失败与被中断时同样产生信封，父不会无限等待。
    - 测试：`openwork-core/tests/session_runtime.rs::failed_and_cancelled_children_both_notify_the_parent`；`openwork-core/tests/session_runtime.rs::a_child_still_completes_when_its_parent_can_no_longer_be_reached`
11. 压缩时，last-user replay 仍是真实用户请求，`agent_message` 不会挤掉它。
    - 测试：`openwork-core/src/session/compaction/compacted_view.rs::a_live_agent_message_never_displaces_the_real_user_request`
    - 缺口：没有带 agent message 的端到端压缩测试。
12. 主 Agent 并行派出 3 个 explorer，并汇总结果。
    - 测试：`openwork-core/tests/session_runtime.rs::parent_spawns_three_explorers_and_aggregates_their_deliveries`；`openwork-core/tests/session_runtime.rs::p2_acceptance_12_three_explorer_deliveries_do_not_trigger_wait_doom_loop`
    - 缺口：子 Session 由测试 host 构造，不经真实 Core。
13. 第 4 个 `spawn_agent` 返回 `agent_limit_reached`，父 Turn 继续。
    - 测试：`openwork-core/tests/session_runtime.rs::fourth_spawn_is_a_failed_tool_result_and_parent_turn_continues`；`openwork-core/src/agent/control.rs::spawn_and_followup_share_the_active_turn_limit`
14. 空闲子 Agent 不占名额，`followup_task` 能再次使用它。
    - 测试：`openwork-core/src/agent/control.rs::spawn_and_followup_share_the_active_turn_limit`；`openwork-core/tests/session_runtime.rs::child_active_turn_owns_and_releases_its_slot_at_terminal`；`openwork-core/src/core.rs::restored_sub_agent_can_be_inspected_and_followed_up_after_restart`
    - 缺口：没有经 `followup_task` 工具调用的 run loop 测试。
15. explorer 的上限是 `accept-edits`，内核拒绝 `cargo build` 一类写工作区的命令；explorer 带越界请求时，直接收到拒绝和可操作说明。
    - 测试：`openwork-agent/src/builder.rs::explorer_cannot_change_the_workspace`；`openwork-sandbox/tests/matrix.rs::accept_edits_denies_bash_workspace_writes_until_escalated`；`openwork-core/tests/session_runtime.rs::acc_36_37_an_unattended_explorer_refuses_escalations_without_a_card`
    - 缺口：没有让 explorer 在真实沙箱里运行构建的测试。
16. explorer 调用 `git log` 时，命令直接执行，不出卡片。父会话为 `auto` 时，explorer 的生效模式仍是 `accept-edits`。
    - 测试：`openwork-core/tests/session_runtime.rs::acc_36_37_an_unattended_explorer_refuses_escalations_without_a_card`；`openwork-core/src/session_tools.rs::a_sub_agent_never_gets_a_wider_mode_than_its_parent_or_role`；`openwork-core/tests/postgres_core_host_flow.rs::acc_01_35_42_session_modes_persist_and_sub_agents_keep_their_snapshot`
17. 子 Agent 的工具面里没有 `write`、`edit`、`update_plan` 与五个控制工具。
    - 测试：`openwork-core/src/session_tools.rs::the_explorer_exposes_only_the_read_only_role_surface`；`openwork-core/src/session/toolset.rs::a_disabled_control_tool_is_neither_advertised_nor_dispatched`；`openwork-core/tests/postgres_core_host_flow.rs::production_host_persists_and_starts_an_idle_explorer_session`
18. `wait_agent` 超时后，模型可以继续，也可以再次等待；连续 3 次超时时，Turn 以 `doom_loop` 失败。
    - 测试：`openwork-core/tests/session_runtime.rs::wait_agent_can_timeout_then_wait_again_for_a_delivery`；`openwork-core/tests/session_runtime.rs::three_consecutive_wait_timeouts_end_the_turn_as_doom_loop`；`openwork-core/tests/session_runtime.rs::delivered_wait_resets_timeout_streak_before_two_more_timeouts`；`openwork-core/tests/session_runtime.rs::cancelling_parent_interrupts_wait_agent_immediately`；`openwork-core/src/agent/tool.rs::wait_timeout_defaults_to_one_minute_and_stays_inside_the_documented_bounds`
19. 重启后，父的下一个用户 Turn 补发完成与中断通知，且只写入一次；没有 Turn 的子 Session 被删除。
    - 测试：`openwork-core/tests/postgres_core_host_flow.rs::parent_next_turn_reconciles_restart_results_exactly_once`；`openwork-core/tests/postgres_sub_agent_sessions.rs::reconciliation_finds_terminal_results_and_removes_zero_turn_orphans`；`openwork-core/src/core.rs::restart_reconciliation_releases_a_restored_zero_turn_orphan_name`
    - 缺口：`cancelled` 子 Turn 的对账没有测试。
20. 对账不创建父 Turn。
    - 测试：`openwork-core/tests/postgres_sub_agent_sessions.rs::reconciliation_finds_terminal_results_and_removes_zero_turn_orphans`；`openwork-core/tests/postgres_core_host_flow.rs::parent_next_turn_reconciles_restart_results_exactly_once`
21. 右栏列出直属子 Agent 及状态；点开后读取完整 transcript，详情只读。
    - 测试：`desktop/src/features/chat/components/AgentRail.test.tsx › "lists every agent with its status, duration, and token total"`；`desktop/src/features/chat/subAgentStore.test.ts › "stores the listed children and registers them on the runtime store"`；`desktop/src/features/chat/SubAgentDetailPage.test.tsx › "loads the transcript through the existing runtime_session_load bridge"`；`desktop/src/features/chat/SubAgentDetailPage.test.tsx › "is read only: no composer, no re-run, no direct dispatch"`；`desktop/src/features/chat/runtimeStore.test.ts › "routes child updates through the status-only reducer"`
22. 删除父会话时，子 Session 及其 `turns`、`messages`、`trace_spans` 一并删除，子 actor 停止。
    - 测试：`openwork-core/tests/postgres_sub_agent_sessions.rs::deleting_the_parent_cascades_to_its_sub_agents`；`openwork-core/tests/postgres_core_host_flow.rs::production_host_persists_and_starts_an_idle_explorer_session`
23. 子 Agent 的 `trace_id` 与父的不同，父 Turn 的完整度仍是 `Complete`。
    - 状态：无测试。结构上 `trace_id` 取自本 Turn 的 `turn_id`（`TurnRunner::trace_id`），子 Turn 有自己的 `turn_id`；`session_runtime.rs` 的多个测试断言 `trace_id` 等于 `turn_id`，但没有测试带子 Agent 的父 Turn 完整度。
24. 子 Agent 与根会话走同一个 run loop；run loop 中没有按“是否子 Agent”区分的分支。
    - 状态：手动：在 `openwork-core/src/session/run_loop/` 检索 `parent_link`、`is_sub_agent`，应当没有结果。多智能体只在其中增加排空点（`drain_agent_messages`）、控制工具分派（`run_agent_tool`）与 `wait_agent` 的超时计数。
