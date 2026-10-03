# Session 运行时

一个活动 Session 对应一个 `SessionActor`。`SessionActor` 是**唯一**推进 Turn 的地方。Trace、Storage 与 Desktop 都不能推进 Turn。

## 1. 对象与所有权

```text
OpenWorkCore
└── Session Registry
    └── SessionActor
        ├── Agent              静态定义（prompt、工具集、上限）
        ├── ChatStateHandle    Conversation 的唯一写者
        ├── ModelPort
        ├── FinalizedToolset
        ├── SessionStorage
        ├── TraceRecorder
        ├── AgentControl       子 Agent 的创建、注册、限额；父子共享同一实例
        ├── Mailbox            子 Agent 回传消息的内存队列，不持久化
        └── ActiveTurn?
```

子 Agent 本身也是一个 `SessionActor`，走完全相同的路径。**它不是这张表的例外。唯一的差别是它的 `SessionApproval` 是 `NonInteractive`。** 通常会出卡片的越界请求与危险命令，在子 Agent 中立即 Deny，见 [permissions.md §6.6](permissions.md)。完整设计见 [multi-agent.md](multi-agent.md)。

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 当前 Turn / Phase | `SessionActor` | Command / Snapshot |
| 子 Agent 注册表与并发名额 | `AgentControl` | 五个 Core 控制工具 |
| 未消费的 Agent Message | `SessionActor` 的 Mailbox | `DeliverAgentMessage` 命令 |
| Conversation | `ChatStateActor` | Append / Snapshot Command |
| Agent 定义 | `Agent` | 构建后只读 |
| 工具副作用与路径安全 | `openwork-tools` | `ToolSessionContext` |
| 数据库写入顺序 | `SessionActor` | 内部 Storage 调用 |
| Live UI 顺序 | `SessionActor` | broadcast |
| Trace Span | `TraceRecorder` | 旁路信号 |

## 2. 命令

所有状态修改都以命令的形式进入 Actor：

```rust
enum SessionCommand {
    StartTurn { turn_id, input, accepted_to },
    CancelTurn { turn_id },
    ResolvePermission { turn_id, tool_call_id, decision },
    CompactConversation { respond_to },
    RewindConversation { compaction_id, respond_to },
    DeliverAgentMessage { task_name, kind, body },
    Snapshot { respond_to },
    Shutdown,
}
```

规则：

- Desktop 的公开 Turn 输入是有序的 `Vec<UserInput>`。显式选择产生 `UserInput::Skill { name, path }`，原始草稿产生 `UserInput::Text { text }`。Core 先按 [skills.md §4.2](skills.md) 把 Skill 解析为 contextual User-role Text。Core 再把它与用户可见的 Text 一起物化为 `PreparedTurnInput`。这一步成功后，Core 才生成 `turn_id` 并发送 `StartTurn`。SessionActor 不读取 Skill 文件，也不按名称解析 Skill；
- Core 在接受请求时生成 `turn_id`。Turn 与 User Message 提交后，经 `accepted_to` 返回 `TurnAccepted`；
- **`StartTurn` 不等待整个 Agent Loop**。从 `SessionUpdate` / `SessionSnapshot` 获取终态；
- 同一 Session 同时只运行一个 Turn。新 Turn 排队，或者显式取消旧 Turn，**不能隐式并发**；
- **`StartTurn` 不再是唯一的输入来源，但仍是唯一能创建 Turn 的命令。** 子 Agent 的 SessionActor 发来 `DeliverAgentMessage`。这条命令**只入队，永不创建 Turn**。Turn 的定义不变，仍是"一次用户输入触发的完整 Agent Loop"。父 Session 空闲时收到的消息留在队列里，等下一个用户 Turn。详见 [multi-agent.md §6](multi-agent.md)；
- Permission Decision 必须同时匹配 Session、Turn 和 Tool Call 三者；
- 只在 Session 空闲时允许压缩与 rewind。有活跃 Turn 时返回 `SessionActive`。

## 3. Turn 状态机

```text
accepted
  → running_model
      → running_tools
          → waiting_permission → running_tools
      → running_model
  → completed

任意活动状态 → cancelled | failed
                → interrupted（仅进程重启后的数据库修正）
```

数据库只保存粗粒度状态：`running | completed | failed | cancelled | interrupted`。

`running_model`、`running_tools`、`waiting_permission` 是**进程内 Phase**。它们只服务 Live UI，不是可恢复的 checkpoint。

## 4. Agent Loop

Agent Loop 只有一个实现，位于 `session/run_loop.rs`：

```text
持久化 Turn 开始 → 追加已物化的 User Message（可含 Skill 正文快照）
循环 1..=max_model_calls:
    检查取消
    排空 Mailbox → 有 Agent Message 就追加进 Conversation（见 multi-agent.md §6.3）
    检查压缩阈值 → 必要时压缩（见 compaction.md）
    组装请求 → 调用模型
        若 ContextOverflow 且未产生语义输出 → 压缩一次并重提交
    追加 Assistant Message
    若无 Tool Call → 完成 Turn
    按 Provider 顺序逐个执行 Tool Call
到达上限 → 失败
```

**七条必须保持的性质：**

1. 执行任何副作用**之前**，Assistant 的 Tool Call Message 必须已经形成完整记录；
2. 每个 Tool Call 必须产生一个 Tool Result。出错时也要产生一个错误结果；
3. Tool Result 写入 Chat State 并持久化成功后，才能开始下一次 Model Call；
4. Tool 执行错误通常作为结果返回给模型，不直接丢失上下文；
5. Permission Deny、用户取消、达到循环上限都是明确的终态；
6. **Trace 写入成功与否不进入任何分支判断。**
7. **"无 Tool Call → 立即完成 Turn"之后不得再发起任何 Model Call。** 这条性质过去只是循环的自然形状，现在其他设计依赖它。[multi-agent.md §6.3](multi-agent.md) 靠它保证"父输出最终回答后到达的 Agent Message 自然留给下一个用户 Turn"。因此不需要投递阶段状态机。将来如果引入"Turn 结束前再问一次模型"之类的逻辑，这道门控就会失效。那时必须同时补上状态机。

Permission Deny 与 Cancel 也要写入合成的错误 Tool Result。一次响应可能含多个 Tool Call。如果前一个 Tool Call 导致 Turn 终止，**其余未执行的 Tool Call 必须按原顺序写入 `cancelled` Tool Result**。否则 Conversation 里会留下悬空的 Tool Call，下一次 Model Call 无法发送它们。

## 5. Tool Call 生命周期

```text
received → validated → allowed ──────────────→ executing → succeeded/failed
                     → waiting_permission
                          → allowed ─────────→ executing → succeeded/failed
                          → denied/cancelled
                     → invalid/unknown_tool ─→ failed result
```

| 阶段 | Owner |
|---|---|
| 解析 Provider Tool Call | `openwork-models` + Core |
| 查找定义、校验参数 | `openwork-tools` |
| 判断 Allow / Ask / Deny | `openwork-tools` + Core |
| 等待用户决定 | `SessionActor` |
| 执行并强制路径策略 | `openwork-tools` + `ToolSessionContext` |
| 形成 Tool Result Message | `SessionActor` + Chat State |
| 记录耗时与错误 | Trace |

多个 Tool Call 按 Provider 顺序**串行**执行。这样做优先保证消息顺序与副作用可以理解。将来如果并行化，只能并行已通过权限判定且互不冲突的 Tool Call。结果仍按原始顺序写回。

## 6. 流式草稿与 Message

```text
Streaming Draft（内存）
  → Live SessionUpdate
  → 响应完整结束
      → 不可变 Assistant Message
      → Chat State
      → messages 表
```

**不把每个 Delta 写入数据库。** Tool Call 的 ID、名称和完整参数都可以解析后，它才进入 Assistant Message。流中断时，丢弃草稿，Turn 失败。**绝不执行不完整的 Tool Call**。

## 7. SessionUpdate

Desktop 的进程内 Live 协议：

```text
turn_started
phase_changed
text_delta / reasoning_delta
tool_call_started / tool_call_input_delta / tool_call_finished
tool_result
permission_requested / permission_resolved
turn_completed / turn_failed / turn_cancelled
```

- Envelope 携带 `version / sessionId / turnId / sequence / occurredAt / update`；
- SessionActor 按 Session 单调分配 `sequence`；
- 可以在内存中合并高频 Delta；
- 保留一个有界 ring buffer，支持同进程内的短暂重连。超出 buffer 后，Desktop 读 `SessionSnapshot`；
- **不建 `session_updates` 表**；
- **Update 缺失不能改变 Turn 状态。**

前端怎样消费 Update，见 [desktop.md](desktop.md)。

## 8. SessionSnapshot

SessionSnapshot 只存在于活动 Actor 的内存里：

```rust
struct SessionSnapshot {
    session_id, active_turn_id, phase,
    draft_text, draft_reasoning,
    tool_calls, pending_permission,
    last_update_sequence,
}
```

它用于**同进程**内的 UI 重连，不用于进程重启后的恢复。数据库不保存 `runtime_state`，也不保存 pending permission。

## 9. 真相来源

| 问题 | 真相来源 |
|---|---|
| Session 元数据 | `sessions` |
| Turn 终态 | `turns` |
| 下一次模型可见历史 | Chat State + `messages` + 压缩投影 |
| 当前活动 Phase | `SessionActor`（内存） |
| Desktop Live 内容 | `SessionUpdate` / Snapshot |
| Model/Tool 耗时与错误 | `trace_spans` |

**禁止：** 从 Trace 恢复 Tool Call；从 Live Update 推导 Turn 终态；从数据库的 `running` 状态自动继续 Agent Loop；从 Tool Span 的失败状态推断副作用一定没有发生。

## 10. 进程中断

启动时执行一次修正。这是修正，不是恢复：

```text
turns.status = running       → interrupted
trace_spans.status = running → outcome_unknown
```

修正之后：**不**重建活动 Turn，**不**恢复 Permission Waiter，**不**重发 Provider Request，**不**自动执行缺少结果的 Tool Call。

有的 Tool Call 已经持久化，但没有 Tool Result。对这类 Tool Call，追加一个合成的 `outcome_unknown` Tool Result。这个结果明确说明：副作用可能已经发生，不得自动重试。**这只是修复 Conversation 的协议完整性**，不是恢复执行。它让下一次请求不携带"只有 Tool Call、没有 Tool Result"的非法历史。

启动时的修正与 `uq_turns_one_running_per_session` 部分唯一索引配套使用。如果不做修正，遗留的 `running` 行会让该 Session 再也插不进新 Turn。

## 11. 降级规则

| 失败 | 行为 |
|---|---|
| User Message / Turn Start 写入失败 | 不调用模型 |
| Assistant Tool Call Message 写入失败 | **不执行工具** |
| Tool Result Message 写入失败 | 不发起下一次 Model Call，Turn 失败 |
| Trace 队列满 / 数据库失败 | 主循环继续，增加丢弃计数 |
| Live Update 接收者断开 | 主循环继续，Snapshot 仍可读 |
| Model 流中断 | 丢弃草稿，Turn 失败 |
| Tool 返回错误 | 写入错误 Tool Result，让模型继续决策 |
| 工具副作用后进程崩溃 | 下次启动时标记中断，**不自动重试** |

## 12. 验收

1. 模型不返回 Tool Call 时，一次 Model Call 完成 Turn；
2. 单 Tool Call：Model → Tool Result → Model → Final；
3. 一次响应含多个 Tool Call 时，结果按原顺序回填；
4. Unknown Tool 与 Invalid Input 写成 Tool Result，模型可以继续；
5. Tool Error 之后仍发生下一次 Model Call；
6. Permission Allow 后执行工具；Deny 后终止，不执行工具；
7. 可以取消等待中的 Model Call 或工具；
8. 连续同名同参的 Tool Call 达到阈值后，以 doom-loop 终止；
9. 达到最大 Model Call 次数后，Turn 失败；
10. Trace Recorder 全部报错时，主循环的结果不变；
11. **Assistant Message 未持久化时工具绝不执行**；
12. 启动时，`running` Turn 变为 `interrupted`，悬空 Tool Call 获得 `outcome_unknown` Result，且不再调度旧 Turn；
13. 前一个 Tool Call 终止 Turn 时，同批剩余的 Tool Call 按原顺序获得 `cancelled` Result。
