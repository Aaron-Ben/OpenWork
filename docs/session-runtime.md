# Session 运行时

一个活动 Session 对应一个 `SessionActor`，它是**唯一**推进 Turn 的地方。Trace、Storage、Desktop 都不能推进 Turn。

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
        └── ActiveTurn?
```

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 当前 Turn / Phase | `SessionActor` | Command / Snapshot |
| Conversation | `ChatStateActor` | Append / Snapshot Command |
| Agent 定义 | `Agent` | 构建后只读 |
| 工具副作用与路径安全 | `openwork-tools` | `ToolSessionContext` |
| 数据库写入顺序 | `SessionActor` | 内部 Storage 调用 |
| Live UI 顺序 | `SessionActor` | broadcast |
| Trace Span | `TraceRecorder` | 旁路信号 |

## 2. 命令

所有状态修改都经命令进入 Actor：

```rust
enum SessionCommand {
    StartTurn { turn_id, input, accepted_to },
    CancelTurn { turn_id },
    ResolvePermission { turn_id, tool_call_id, decision },
    CompactConversation { respond_to },
    RewindConversation { compaction_id, respond_to },
    Snapshot { respond_to },
    Shutdown,
}
```

规则：

- `turn_id` 由 Core 在接受请求时生成；Turn 与 User Message 提交后通过 `accepted_to` 返回 `TurnAccepted`；
- **`StartTurn` 不等待整个 Agent Loop**，终态通过 `SessionUpdate` / `SessionSnapshot` 获取；
- 同一 Session 同时只运行一个 Turn，新 Turn 排队或显式取消旧 Turn，**不能隐式并发**；
- Permission Decision 必须同时匹配 Session、Turn 和 Tool Call 三者；
- 压缩与 rewind 只在 Session 空闲时允许，有活跃 Turn 时返回 `SessionActive`。

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

`running_model`、`running_tools`、`waiting_permission` 是**进程内 Phase**，只服务 Live UI，不是可恢复的 checkpoint。

## 4. Agent Loop

唯一实现在 `session/run_loop.rs`：

```text
持久化 Turn 开始 → 追加 User Message
循环 1..=max_model_calls:
    检查取消
    检查压缩阈值 → 必要时压缩（见 compaction.md）
    组装请求 → 调用模型
        若 ContextOverflow 且未产生语义输出 → 压缩一次并重提交
    追加 Assistant Message
    若无 Tool Call → 完成 Turn
    按 Provider 顺序逐个执行 Tool Call
到达上限 → 失败
```

**六条必须保持的性质：**

1. Assistant 的 Tool Call Message 在执行任何副作用**之前**形成完整记录；
2. 每个 Tool Call 必须产生一个 Tool Result，哪怕是错误结果；
3. Tool Result 写入 Chat State 并持久化成功后，才能开始下一次 Model Call；
4. Tool 执行错误通常作为结果返回给模型，而不是直接丢失上下文；
5. Permission Deny、用户取消、达到循环上限都是明确终态；
6. **Trace 写入成功与否不进入任何分支判断。**

Permission Deny 与 Cancel 也要写入合成的错误 Tool Result。若一次响应含多个 Tool Call 而前一个导致 Turn 终止，**其余未执行的调用必须按原顺序写入 `cancelled` Tool Result**——否则 Conversation 里会留下无法发送给下一次模型的悬空 Tool Call。

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

多个 Tool Call 按 Provider 顺序**串行**执行，优先保证消息顺序与副作用可理解。若将来并行化，只能并行已通过权限且互不冲突的调用，且结果仍按原始顺序写回。

## 6. 流式草稿与 Message

```text
Streaming Draft（内存）
  → Live SessionUpdate
  → 响应完整结束
      → 不可变 Assistant Message
      → Chat State
      → messages 表
```

**不把每个 Delta 写入数据库。** Tool Call 只有在 ID、名称和完整参数都可解析后才进入 Assistant Message；流中断则丢弃草稿、Turn 失败——**绝不执行不完整的 Tool Call**。

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
- `sequence` 由 SessionActor 按 Session 单调分配；
- 高频 Delta 可在内存合并；
- 保留有界 ring buffer 支持同进程内短暂重连，超出后 Desktop 读 `SessionSnapshot`；
- **不建 `session_updates` 表**；
- **Update 缺失不能改变 Turn 状态。**

前端消费方式见 [desktop.md](desktop.md)。

## 8. SessionSnapshot

只存在于活动 Actor 的内存里：

```rust
struct SessionSnapshot {
    session_id, active_turn_id, phase,
    draft_text, draft_reasoning,
    tool_calls, pending_permission,
    last_update_sequence,
}
```

用于**同进程** UI 重连，不用于进程重启恢复。数据库不保存 `runtime_state`，也不保存 pending permission。

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

启动时执行一次修正（不是恢复）：

```text
turns.status = running       → interrupted
trace_spans.status = running → outcome_unknown
```

之后**不**重建活动 Turn、**不**恢复 Permission Waiter、**不**重发 Provider Request、**不**自动执行缺结果的 Tool Call。

对已持久化但没有 Tool Result 的 Tool Call，追加一个合成的 `outcome_unknown` Tool Result，明确说明副作用可能已经发生、不得自动重试。**这只是修复 Conversation 的协议完整性**——避免下一次请求携带"只有 Tool Call、没有 Tool Result"的非法历史——不是恢复执行。

清扫与 `uq_turns_one_running_per_session` 部分唯一索引是一对：不清扫，遗留的 `running` 行会让该 Session 再也插不进新 Turn。

## 11. 降级规则

| 失败 | 行为 |
|---|---|
| User Message / Turn Start 写入失败 | 不调用模型 |
| Assistant Tool Call Message 写入失败 | **不执行工具** |
| Tool Result Message 写入失败 | 不进行下一次模型调用，Turn 失败 |
| Trace 队列满 / 数据库失败 | 主循环继续，增加丢弃计数 |
| Live Update 接收者断开 | 主循环继续，Snapshot 仍可读 |
| Model 流中断 | 丢弃草稿，Turn 失败 |
| Tool 返回错误 | 写入错误 Tool Result，让模型继续决策 |
| 工具副作用后进程崩溃 | 下次启动标记中断，**不自动重试** |

## 12. 验收

1. 模型无 Tool Call，一次调用完成 Turn；
2. 单 Tool Call：Model → Tool Result → Model → Final；
3. 一次响应多个 Tool Call，结果按原顺序回填；
4. Unknown Tool 与 Invalid Input 被写成 Tool Result，模型可继续；
5. Tool Error 之后仍发生下一次 Model Call；
6. Permission Allow 后执行，Deny 后终止且不执行；
7. 可取消等待中的模型调用或工具；
8. 连续同名同参达到阈值后 doom-loop 终止；
9. 达到最大 Model Call 次数后失败；
10. Trace Recorder 全部报错时主循环结果不变；
11. **Assistant Message 未持久化时工具绝不执行**；
12. 启动时 `running` Turn 变为 `interrupted`，悬空 Tool Call 获得 `outcome_unknown` Result，且旧 Turn 不会被调度；
13. 前一个 Tool Call 终止 Turn 时，同批剩余调用按原顺序获得 `cancelled` Result。
