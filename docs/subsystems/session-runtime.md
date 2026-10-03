# Session 运行时

本页描述一个 Session 怎样接收命令、推进 Turn、执行 Tool Call，以及怎样把进度推给界面。实现在 `openwork-core` 的 `session/`：`actor.rs` 持有状态，`run_loop/` 执行 Agent Loop。Conversation 由 `openwork-chat-state` 持有，Agent 定义与上限来自 `openwork-agent`。

模型输入怎样组装见 [context-window.md](context-window.md)，压缩见 [compaction.md](compaction.md)，权限判定见 [permissions.md](permissions.md)。

## 1. 对象与所有权

```text
OpenWorkCore
└── sessions: SessionId → SessionHandle
    └── SessionActor
        ├── Agent                静态定义（system prompt、工具名、上限）
        ├── ChatStateHandle      Conversation 的唯一写者
        ├── ModelPort
        ├── TurnToolset          FinalizedToolset + Core 控制工具
        ├── SessionStorage
        ├── TraceRecorder
        ├── CompactionStateCollector
        ├── WorldStateBaseline   见 context-window.md §3
        ├── AgentControl?        子 Agent 控制面；根 Session 与直属子 Session 共享
        ├── AgentMailbox         子 Agent 发回的消息，内存队列，不持久化
        ├── 沙箱模式（watch）
        └── ActiveTurn?
```

只有 `SessionActor` 能开始 Turn。Turn 在 Actor 派生的 runner 任务里执行。runner 通过事件通道把 Update 与审批请求交回 Actor，由 Actor 统一分配序号并广播。

子 Agent 也是一个 `SessionActor`，走同一条路径。唯一的差别是它的 `SessionApproval` 是 `NonInteractive`：需要卡片的调用直接拒绝，见 [permissions.md §13.3](permissions.md)。子 Agent 的创建与投递见 [multi-agent.md](multi-agent.md)。

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 当前 Turn 与 Phase | `SessionActor` | 命令、Snapshot |
| 沙箱模式 | `SessionActor` | `SetPermissionMode` 命令 |
| 子 Agent 注册表与并发名额 | `AgentControl` | Core 控制工具 |
| 未消费的 Agent Message | `AgentMailbox` | `DeliverAgentMessage` 命令 |
| Conversation | `ChatStateActor` | `ChatStateHandle` 的追加与读取命令 |
| Agent 定义 | `Agent` | 构建后只读 |
| 工具副作用与路径安全 | `openwork-tools` | `ToolSessionContext` |
| 数据库写入顺序 | runner | `SessionStorage` 调用 |
| Live UI 顺序 | `SessionActor` | broadcast |
| Trace Span | `TraceRecorder` | 旁路信号 |

## 2. 命令

所有状态修改都以命令的形式进入 Actor（`session/actor.rs` 的 `SessionCommand`）：

```rust
enum SessionCommand {
    StartTurn { turn_id, client_request_id, input, disabled_skill_names, turn_slot, respond_to },
    CancelTurn { turn_id, respond_to },
    ResolvePermission { turn_id, tool_call_id, decision, respond_to },
    DeliverAgentMessage { message },
    SetPermissionMode { mode, respond_to },
    CompactConversation { disabled_skill_names, respond_to },
    RewindConversation { compaction_id, respond_to },
    Snapshot { respond_to },
    ReplayUpdates { after_sequence, respond_to },
    AcceptedTurn { client_request_id, respond_to },
    Shutdown { respond_to },
}
```

开始 Turn：

- Desktop 交给 `OpenWorkCore::start_turn` 一个有序的 `Vec<UserInput>`。`UserInput::Text` 是用户草稿，`UserInput::Skill { name, path }` 是显式选择的 Skill。
- 输入中必须至少有一段 Text。只有 Skill 时，返回 `EmptyInput`。
- Core 按 [skills.md §4](skills.md) 把 Skill 解析成 contextual User-role Text。每段正文的估算必须低于 8 000 token，否则拒绝。
- Core 把 Skill 正文与用户 Text 物化为 `PreparedTurnInput`，再由 `SessionHandle` 生成 `turn_id` 并发送 `StartTurn`。Actor 不读 Skill 文件。

Actor 收到 `StartTurn` 时，按这个顺序判断：

1. `reload_required` 已设置时，返回 `ReloadRequired`（见 [compaction.md §7](compaction.md)）。
2. 同一个 `client_request_id` 已被接受时，返回同一个 `TurnAccepted`。
3. 有活动 Turn 时，返回 `Busy(turn_id)`。**新 Turn 不排队。**
4. 登记 `ActiveTurn`，发 `turn_started`，派生 runner，返回 `TurnAccepted`。

`TurnAccepted` 在 runner 写库之前返回。Turn 行与 User Message 由 runner 的第一步写入。这一步失败时，Turn 以 `persistence_error` 结束。`StartTurn` 不等待 Agent Loop。终态从 `turn_finished` Update 或 Snapshot 读取。

其余命令：

- `DeliverAgentMessage` 只把消息放进 Mailbox，**不创建 Turn**。投递前，`SessionHandle` 检查消息估算低于 4 000 token。空闲 Session 收到的消息留到下一个用户 Turn，见 [multi-agent.md §6](multi-agent.md)。
- `ResolvePermission` 必须匹配活动 Turn 与正在等待的 Tool Call。不匹配时返回 `TurnNotActive` 或 `PermissionNotPending`。Session 由调用的 handle 确定。
- `CancelTurn` 只接受活动 Turn 的 ID，其他 ID 返回 `TurnNotActive`。
- `CompactConversation` 与 `RewindConversation` 只在没有活动 Turn 时执行，否则返回 `SessionActive`。
- `SetPermissionMode` 从下一次调用起生效，见 [permissions.md §2](permissions.md)。

## 3. Turn 状态

进程内 Phase（`SessionPhase`）：

```text
starting
  → running_model ⇄ compacting
      → running_tools ⇄ waiting_permission
      → running_model
  → 终态：completed | failed | cancelled
```

- Phase 只服务 Live UI，不是可恢复的 checkpoint。
- `compacting` 只在 Turn 内的自动压缩期间出现。
- 数据库的 `turns.status` 只有 `running | completed | failed | cancelled | interrupted`。`interrupted` 只由启动修正写入（§10）。

`TurnOutcome` 有三种：`Completed { final_text }`、`Failed { code, message }`、`Cancelled`。`Failed` 的 `code` 取值：

| code | 原因 |
|---|---|
| `model_error` | Provider 返回错误，或流中断 |
| `model_protocol_error` | 流没有完成响应、完成两次，或请求组装失败 |
| `persistence_error` | 写库失败 |
| `chat_state_error` | Chat State 拒绝追加 |
| `compaction_error` | Turn 内的自动压缩失败 |
| `permission_denied` | 用户在卡片上拒绝 |
| `doom_loop` | 重复调用达到阈值（§4） |
| `max_model_calls` | Model Call 次数达到上限 |
| `actor_stopped` | Actor 已停止 |
| `user_project_context_error` 等 | World State 来源读取失败，见 [context-window.md §3](context-window.md) |

## 4. Agent Loop

Agent Loop 只有一个实现：`session/run_loop/mod.rs` 的 `TurnRunner`。

```text
写入 Turn 行与 User Message（Skill 正文快照在用户消息之前）→ 追加进 Chat State
解析 System Context（每个 Turn 一次）
循环 model_call_index = 1..=max_model_calls：
    检查取消
    排空 Mailbox，逐条写库并追加（multi-agent.md §6.3）
    phase = running_model
    读取 Conversation，采样 World State（context-window.md §3）
    组装请求；达到压缩线时先修剪，仍达线再压缩（compaction.md §1）
    调用模型；明确的 ContextOverflow 且未产生语义输出时，修剪或压缩后重提交一次
    写入 Assistant Message → 追加进 Chat State
    没有 Tool Call → Turn 完成，最终文本是这次响应的文本
    phase = running_tools；按 Provider 顺序逐个执行 Tool Call
循环用完 → max_model_calls
```

| 上限 | 默认 Agent | explorer |
|---|---|---|
| `max_model_calls` | 20 | 15 |
| `doom_loop_threshold` | 3 | 3 |

这些性质由代码保持：

1. 执行任何工具之前，含 Tool Call 的 Assistant Message 已经写库。写库失败时，不执行工具。
2. 每个 Tool Call 都产生一个 Tool Result。出错、拒绝、取消时，也写一个结果。
3. Tool Result 写库并追加进 Chat State 之后，才开始下一次 Model Call。写库失败时，Turn 失败。
4. 工具执行错误作为结果返回给模型，循环继续。
5. 用户拒绝、取消、doom loop、达到上限都是终态。规则拒绝写成结果，Turn 继续。
6. Trace 写入成功与否不进入任何分支判断。
7. **响应不含 Tool Call 时，Turn 立即完成，之后不再发起 Model Call。** [multi-agent.md §6.3](multi-agent.md) 依赖这一条：最终回答之后到达的 Agent Message 留给下一个用户 Turn。理由见 [Agent Note：子 Agent 结果经 mailbox 投递](../../.agents/notes/implemented/architecture/2026-08-08-sub-agent-mailbox-delivery.md)。

一个 Tool Call 让 Turn 终止时，同一响应里剩余的 Tool Call 按原顺序得到 `cancelled` 结果。结果文本是 `tool was not executed because the turn already terminated`，Trace 记 `cancelled` + `system`。

doom loop 的判定：同名、规范化参数相同的 Tool Call 连续出现，次数达到 `doom_loop_threshold` 时，这一次调用不执行。它得到结果 `doom loop detected for tool '<name>'`，Turn 以 `doom_loop` 失败。`wait_agent` 不参与这项计数；它连续超时达到阈值时，Turn 同样以 `doom_loop` 失败。

## 5. Tool Call 生命周期

`TurnRunner::run_tool_call` 按这个顺序处理一个 Tool Call：

1. 生成 `tool_call_id`，开启 Tool Span。
2. 把参数解析为 JSON。失败时写 `InvalidArguments` 结果。
3. 发 `tool_call_started`，状态是 `validating`。
4. 解析工具名：`update_plan`、子 Agent 控制工具，或注册表里的工具。未知名称写 `ToolNotFound` 结果。
5. 注册表工具按 schema 校验参数。失败时写 `InvalidArguments` 结果。
6. 检查 doom loop（§4）。
7. 控制工具直接执行，不经沙箱，Trace 来源记 `control_tool`。`update_plan` 见 [update-plan.md §4](update-plan.md)。
8. 注册表工具先经授权（[permissions.md §1](permissions.md)），再在这一次调用的策略下执行。执行中的输出以 `tool_call_progress` 推给界面。
9. 写结果：写库 → 追加进 Chat State → 发 `tool_call_finished`。

授权的三种结论：

| 结论 | 处理 |
|---|---|
| 执行 | 按会话模式的策略执行 |
| 规则拒绝 | 写 `denied` 结果，Turn 继续 |
| 需要卡片 | 发 `permission_requested`，phase 变为 `waiting_permission`，等待决定 |

等待卡片的结果：

| 决定 | 处理 |
|---|---|
| 允许一次 | 带着这次批准的路径授权执行 |
| 拒绝 | 写 `denied` 结果，Turn 以 `permission_denied` 失败 |
| Turn 被取消 | 写 `cancelled` 结果，Turn 取消 |
| Actor 已停止 | 写 `outcome_unknown` 结果，Turn 以 `actor_stopped` 失败 |

工具返回 `cancelled` 状态时，Turn 取消。结果状态到模型可见状态的映射：`succeeded` → `success`，`failed` 与 `outcome_unknown` → `error`，`denied` → `denied`，`cancelled` → `interrupted`。

| 阶段 | Owner |
|---|---|
| 解析 Provider Tool Call | `openwork-models` + Core |
| 查找定义、校验参数 | `openwork-tools` + `TurnToolset` |
| 授权 | `openwork-tools`（`prepare`）+ Core（`approval.rs`） |
| 等待用户决定 | `SessionActor` |
| 执行并强制路径策略 | `openwork-tools` + `ToolSessionContext` |
| 形成 Tool Result Message | runner + Chat State |
| 记录耗时与错误 | Trace |

一次响应里的多个 Tool Call 按 Provider 顺序**串行**执行，审批也串行出现。理由见 [Agent Note：串行执行 Tool Call](../../.agents/notes/implemented/architecture/2026-07-27-serial-tool-calls.md) 与 [Agent Note：两个按钮与串行审批](../../.agents/notes/implemented/architecture/2026-08-01-two-button-cards-and-serial-approvals.md)。

## 6. 流式草稿与 Message

```text
begin_draft（Chat State 内存）
  → text_delta / reasoning_delta：追加草稿，同时发 Live Update
  → response_completed
      → finish_draft
      → 不可变 Assistant Message：thinking、text、provider opaque、tool calls 依次排列
      → messages 表 → Chat State
      → draft_cleared
```

- **Delta 不写数据库。** Assistant Message 只从完整响应构造。
- Tool Call 只来自完整响应。**不执行不完整的 Tool Call。**
- 流出错、被取消、没有完成响应、完成两次时，丢弃草稿。Turn 失败或取消。
- 第一个文本、推理或 Tool Call 事件到达后，错误带上“已产生语义输出”的标记。带这个标记的 overflow 不触发压缩（[compaction.md §1](compaction.md)）。

## 7. SessionUpdate

Desktop 的进程内 Live 协议（`session/updates.rs`）：

```text
turn_started
phase_changed
text_delta / reasoning_delta / draft_cleared
tool_call_started / tool_call_progress / tool_call_finished
permission_requested / permission_resolved
plan_updated
turn_finished
```

- Envelope 带 `version / sessionId / turnId / sequence / occurredAtMs / update`。当前 `version` 是 7。
- Actor 按 Session 单调分配 `sequence`，从 1 开始。只有活动 Turn 的 Update 会发出。
- Actor 保留最近 512 条 Update 的 ring buffer。`ReplayUpdates { after_sequence }` 返回 buffer 里更新的部分。超出 buffer 时，Desktop 读 Snapshot。
- 每个 Session 一个容量 512 的 broadcast，Core 另有一个容量 4 096 的全局 broadcast。
- `plan_updated` 只在计划写库成功后发出，携带完整快照（[update-plan.md](update-plan.md)）。
- **没有 `session_updates` 表。Update 缺失不改变 Turn 状态。**

前端怎样消费 Update，见 [desktop.md](../desktop.md)。

## 8. SessionSnapshot

Snapshot 只存在于 Actor 的内存里：

```rust
struct SessionSnapshot {
    version, session_id, last_update_sequence,
    permission_mode,   // 当前沙箱模式
    sandbox,           // 启动自检的结论
    runtime,           // Idle | Running {..} | Terminal {..}
}
```

- `Running` 带 `turn_id`、`client_request_id`、`phase`、草稿文本与推理、Live Tool Call、等待中的审批、当前计划。
- `Terminal` 带最后一个 Turn 的结果与计划，供 Turn 刚结束时重连。
- `tool_call_progress` 只在 Update 里，不折进 Snapshot。

Snapshot 用于**同进程**的界面重连，不用于进程重启后的恢复。数据库不保存运行时 Phase，也不保存等待中的审批。

## 9. 真相来源

| 问题 | 真相来源 |
|---|---|
| Session 元数据 | `sessions` |
| Turn 终态 | `turns` |
| 下一次模型可见历史 | `messages` + 最新 checkpoint（[compaction.md §6](compaction.md)），载入后由 Chat State 持有 |
| 当前活动 Phase | `SessionActor`（内存） |
| Desktop Live 内容 | `SessionUpdate`、Snapshot |
| Model 与 Tool 的耗时与错误 | `trace_spans` |

系统不做这些事：从 Trace 恢复 Tool Call；从 Live Update 推导 Turn 终态；从数据库的 `running` 状态继续 Agent Loop；从 Tool Span 的失败状态推断副作用没有发生。

## 10. 进程中断

Core 启动时（`OpenWorkCore::from_storage`），`mark_running_interrupted` 在一个事务里修正遗留状态：

```text
trace_spans.status = running → outcome_unknown（error_code = process_restart）
turns.status       = running → interrupted    （error_code = process_restart）
```

修正之后，系统不重建活动 Turn，不恢复审批等待，不重发 Provider 请求，不执行缺少结果的 Tool Call。

已写库但没有结果的 Tool Call 在数据库里保持原样。每次组装请求时，请求副本为它补一条 `interrupted` 结果，正文是固定文本：

```text
Tool result unavailable: the previous turn ended before a durable result was recorded.
```

补结果只修复请求的协议完整性，不恢复执行。规则见 [context-window.md §4](context-window.md)。

修正与部分唯一索引 `uq_turns_one_running_per_session` 配套。不做修正，遗留的 `running` 行会让这个 Session 无法开始新 Turn。子 Agent 在重启后的处理见 [multi-agent.md](multi-agent.md)。

理由见 [Agent Note：重启只修正，不恢复](../../.agents/notes/implemented/architecture/2026-07-27-restart-correction-not-recovery.md)。

## 11. 降级规则

| 失败 | 行为 |
|---|---|
| Turn 行或 User Message 写入失败 | 不调用模型，Turn 以 `persistence_error` 结束 |
| Assistant Message 写入失败 | **不执行工具**，Turn 失败 |
| Tool Result 写入失败 | 不发起下一次 Model Call，Turn 失败 |
| Trace 队列满或数据库失败 | 主循环继续，`dropped_signals` 计数增加 |
| Live Update 接收者断开 | 主循环继续，Snapshot 仍可读 |
| Model 流中断 | 丢弃草稿，Turn 失败 |
| 工具返回错误 | 写错误结果，模型继续决策 |
| 工具副作用之后进程崩溃 | 下次启动时标记 `interrupted`，**不自动重试** |
| 压缩已写库但 Chat State 安装失败 | 设置 `reload_required`，拒绝新 Turn（[compaction.md §7](compaction.md)） |

## 12. 验收

编号沿用原设计文档。测试路径相对 `crates/`。带 Postgres 的测试需要 `TEST_DATABASE_URL`。

1. 模型不返回 Tool Call 时，一次 Model Call 完成 Turn。
   - 测试：`openwork-core/tests/session_runtime.rs::no_tool_turn_completes_after_one_model_call`
2. 单个 Tool Call：Model → Tool Result → Model → Final。
   - 测试：`openwork-core/tests/session_runtime.rs::tool_result_is_in_the_next_model_request`
3. 一次响应含多个 Tool Call 时，结果按原顺序回填。
   - 测试：`openwork-core/tests/session_runtime.rs::multiple_tool_results_keep_provider_order_in_the_next_request`
   - 缺口：只断言下一次请求里的顺序，没有断言写库顺序。
4. 未知工具与无效参数写成 Tool Result，模型可以继续。
   - 测试：`openwork-core/tests/session_runtime.rs::unknown_tool_becomes_a_result_and_the_model_continues`；`openwork-core/tests/session_runtime.rs::an_invalid_plan_fails_the_call_without_changing_stored_state`
   - 缺口：普通工具的无效参数没有测试；未知工具一条没有断言结果正文。
5. 工具出错之后仍发生下一次 Model Call。
   - 测试：`openwork-core/tests/session_runtime.rs::tool_failure_is_returned_to_the_model_instead_of_stopping_the_loop`
   - 缺口：没有断言错误结果出现在下一次请求里。
6. 允许后执行工具；用户在卡片上拒绝后，Turn 终止，工具不执行；规则拒绝写成结果，Turn 继续。
   - 测试：`openwork-core/tests/session_runtime.rs::acc_18_19_an_escalation_asks_with_its_paths_and_applies_to_that_call_only`；`openwork-core/tests/session_runtime.rs::acc_24_a_user_denial_stops_the_turn_without_running_the_tool`；`openwork-core/tests/session_runtime.rs::acc_09_39_a_protected_write_target_is_refused_and_the_turn_continues`
7. 可以取消等待中的 Model Call 或工具。
   - 测试：`openwork-core/tests/session_runtime.rs::cancelling_a_turn_cancels_the_active_tool_call`；`openwork-core/tests/session_runtime.rs::cancelling_parent_interrupts_wait_agent_immediately`
   - 缺口：取消等待中的 Model Call、取消等待中的审批，都没有测试。
8. 同名同参的 Tool Call 连续达到阈值时，Turn 以 doom loop 终止。
   - 测试：`openwork-core/tests/session_runtime.rs::three_identical_read_calls_still_end_the_turn_as_doom_loop`；`openwork-core/tests/session_runtime.rs::three_consecutive_wait_timeouts_end_the_turn_as_doom_loop`
9. 达到最大 Model Call 次数后，Turn 失败。
   - 状态：无测试。`openwork-agent/src/builder.rs` 只断言默认值 20 与 explorer 的 15。
10. Trace Recorder 全部报错时，主循环的结果不变。
    - 状态：无测试。测试用的 recorder 不会失败。
11. **Assistant Message 未写库时，工具绝不执行。**
    - 测试：`openwork-core/tests/session_runtime.rs::assistant_persistence_failure_prevents_tool_execution`
12. 启动时，`running` Turn 变为 `interrupted`，`running` Span 变为 `outcome_unknown`，旧 Turn 不再调度。缺少结果的 Tool Call 在请求副本里得到 `interrupted` 结果，数据库不变。
    - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`；`openwork-core/src/context/normalize.rs::missing_tool_result_is_synthesized_in_call_order_with_the_original_call_id`
    - 缺口：没有断言 Span 变为 `outcome_unknown`；没有把重启与补结果连起来的端到端测试；“不再调度旧 Turn”只对子 Agent 有间接测试（`openwork-core/src/core.rs::restarted_core_restores_persisted_sub_agent_identities_without_starting_turns`）。
13. 前一个 Tool Call 终止 Turn 时，同批剩余的 Tool Call 按原顺序得到 `cancelled` 结果。
    - 状态：无测试。所有终止类测试的响应都只有一个 Tool Call。

原文没有编号、由代码事实补充的条目：

14. 同一个 `client_request_id` 重复提交时返回同一个 Turn；另一个请求得到 `Busy`。
    - 测试：`openwork-core/tests/session_runtime.rs::duplicate_client_request_is_idempotent_and_a_different_turn_is_busy`
15. Turn 运行时，手动压缩返回 `SessionActive`。
    - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_is_rejected_while_a_turn_is_active`
    - 缺口：rewind 的同一条规则、`ReloadRequired` 拒绝新 Turn、审批决定的 ID 匹配，都没有测试。
