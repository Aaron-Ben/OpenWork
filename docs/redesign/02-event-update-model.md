# Session Runtime、Agent Loop 与数据流

> 状态：已实施。SessionActor 已拥有 Turn 主循环、Live Update、Snapshot 与 Sequence Replay；V1 仍不恢复进程退出前未完成的 Turn。
>
> V1 边界：完成 Model → Tool → Model 主循环、完整会话历史、Live Update 与 Trace；不恢复进程退出前未完成的 Turn。

## 1. 核心问题不是 Trace 或恢复

“工具调用完成后不知道下一步做什么”说明控制流没有被一个稳定 Runtime 完整拥有。正确主链必须是：

```text
Model Response
  -> parse Tool Call
  -> authorize
  -> execute
  -> append Tool Result to Conversation
  -> build next Model Request
  -> call Model again
```

Trace 只能记录这条链，数据库只能保存已经提交的结果。两者都不能代替最后一步的 Runtime 决策。

## 2. grok-build 的实际做法

### 2.1 Grok 的 Prompt 是 Session 命令

`xai-grok-shell/src/session/commands.rs` 定义 `SessionCommand::Prompt`，携带 `prompt_id`、内容、模式、取消语义和完成通道。OpenWork 不照搬这个领域名，目标模型把完整生命周期称为 Turn。

`xai-grok-shell/src/session/acp_session_impl/run_loop.rs` 的 Session Loop 接收命令，并确保活动 Prompt、排队输入、取消和完成都经过同一个 `SessionActor`。

### 2.2 Agent Loop 在 Session Runtime 中

`xai-grok-shell/src/session/acp_session_impl/turn.rs` 的 `process_conversation_turn` 使用一个明确的循环：

1. 从 Chat State 构建 Model Request；
2. 调用模型并消费响应；
3. 把 Assistant 内容写回 Chat State；
4. 没有 Tool Call 时结束；
5. 有 Tool Call 时调用 `execute_tool_calls`；
6. Tool Result 写回 Chat State；
7. 增加本地循环计数并继续。

这里没有通过 Trace 或数据库查询来决定“是否再调用一次模型”。

### 2.3 多条数据流各自有用途

Grok 同时存在：

- Conversation/`chat_history.jsonl`：模型下一次看到的内容；
- `updates.jsonl`：UI/协议回放；
- `events.jsonl`/Signals：统计和产品事件；
- tracing span：性能与故障诊断。

OpenWork 借用“语义分离”，但 V1 不复制所有持久化流。PostgreSQL 只保存目标业务数据和 Trace；Session Update 先保持进程内 Live 数据。

## 3. 目标运行时对象

```text
OpenWorkCore
└── ActiveSessions
    └── SessionActor
        ├── Agent
        ├── ChatStateHandle
        ├── Model
        ├── ToolCatalog
        ├── ToolContext
        ├── SessionStorage
        ├── TraceRecorder
        └── ActiveTurn?
```

所有权：

| 状态 | 唯一 Owner | 其他组件如何访问 |
| --- | --- | --- |
| 当前 Turn/Phase | `SessionActor` | Command/Snapshot |
| Conversation | `ChatStateActor` | Append/BuildRequest Command |
| Agent Definition | `Agent` | 构建后只读 |
| Tool Catalog | `openwork-tools` | Core 查询和调用 |
| 工具副作用与路径安全 | `openwork-tools` | Tool Invocation + ToolContext |
| 数据库写入顺序 | `SessionActor` | 调用内部 Storage |
| Live UI 顺序 | `SessionActor` | Broadcast/内存缓冲 |
| Trace Span | `TraceRecorder` | Core 旁路发信号 |

## 4. Session 命令

最小命令集：

```rust
enum SessionCommand {
    StartTurn {
        turn_id: TurnId,
        input: Vec<ContentBlock>,
        accepted_to: oneshot::Sender<TurnAccepted>,
    },
    CancelTurn {
        turn_id: TurnId,
    },
    ResolvePermission {
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    },
    Snapshot {
        respond_to: oneshot::Sender<SessionSnapshot>,
    },
    Shutdown,
}
```

规则：

- 所有状态修改都通过命令进入 Actor；
- `turn_id` 由 Core 在接受请求时生成；Turn 和 User Message 提交后通过 `accepted_to` 返回 `TurnAccepted`；
- Turn 的运行终态通过 `SessionUpdate`/`SessionSnapshot` 获取，不让 Start Command 等待完整 Agent Loop；
- V1 同一 Session 只运行一个 Turn；
- 新 Turn 到来时可排队或显式 `send_now` 取消旧 Turn，不能隐式并发；
- Permission Decision 必须同时匹配 Session、Turn 和 Tool Call。

## 5. Turn 状态机

```text
accepted
  -> running_model
      -> running_tools
          -> waiting_permission
          -> running_tools
      -> running_model
  -> completed

任意活动状态
  -> cancelled
  -> failed
  -> interrupted (仅进程重启后的数据库修正)
```

数据库状态只保存终局和粗粒度活动状态：

```text
running | completed | failed | cancelled | interrupted
```

`running_model`、`running_tools`、`waiting_permission` 是进程内 Phase，用于 Live UI；V1 不把它们当作可恢复 Checkpoint。

## 6. 唯一 Agent Loop

目标伪代码：

```rust
async fn run_turn(&mut self, turn: Turn) -> TurnOutcome {
    self.persist_turn_started(&turn).await?;
    self.chat.append_user(turn.input).await?;

    for model_call_index in 1..=self.agent.max_model_calls() {
        self.check_cancelled()?;

        let request = self.chat.build_request(
            self.agent.system_prompt(),
            self.agent.tool_definitions(),
        ).await?;

        let response = self.call_model(model_call_index, request).await?;
        self.chat.append_assistant(response.message()).await?;

        if response.tool_calls().is_empty() {
            return self.complete_turn(response.final_text()).await;
        }

        for tool_call in response.tool_calls() {
            self.run_tool_call(model_call_index, tool_call).await?;
        }

        // Tool Result 已进入 Chat State，循环自然构建下一次 Model Request。
    }

    self.fail_turn(MaxModelCalls).await
}
```

必须保持：

1. Assistant Tool Call Message 在执行副作用前形成完整记录；
2. 每个 Tool Call 必须产生一个 Tool Result，哪怕结果是错误；
3. Tool Result 写入 Chat State 并持久化成功后，才能开始下一次 Model Call；
4. Tool 执行错误通常作为结果返回给模型，不直接让 Runtime 丢失上下文；
5. Permission Deny、用户取消和达到循环上限是明确终态；
6. Trace 写入成功与否不进入任何分支判断。

Permission Deny/Cancel 也要写入合成的错误 Tool Result，再结束 Turn。若一次响应包含多个 Tool Call，而前一个调用导致 Turn 终止，其余未执行调用也必须按原顺序写入 `cancelled` Tool Result，避免 Conversation 中留下无法发送给下一次模型的悬空 Tool Call。

## 7. 为什么删除 Step

当前 `StepId` 同时被用于：

- Model Call 次序；
- ToolRun 父节点；
- Approval Recovery；
- Journal Replay；
- Trace 关联；
- Desktop Live Event。

但运行时真实结构只是循环中的第 N 次 Model Call。目标改为：

- `model_call_index: u32`：Turn 内局部顺序；
- `ModelCallSpanId`：仅 Trace 关联；
- `provider_call_id`：Provider 返回的 Tool Call ID；
- `ToolCallId`：Core 内部 Tool Call 关联键。

删除 Step 后，工具是否继续执行由代码控制流决定，不再依赖 Step 投影或 Replay 状态机。

## 8. Tool Call 生命周期

```text
received
  -> validated
  -> allowed -----------------> executing -> succeeded/failed
  -> waiting_permission
       -> allowed ------------> executing -> succeeded/failed
       -> denied/cancelled
  -> invalid/unknown_tool ----> failed result
```

职责拆分：

| 阶段 | Owner |
| --- | --- |
| 解析 Provider Tool Call | `openwork-models` + Core |
| 查找定义、校验参数 | `openwork-tools` |
| 根据 Tool Definition/Input/Context 判断 Allow/Ask/Deny | `openwork-tools` + Core |
| 等待用户决定 | `SessionActor` |
| 执行文件/进程工具并强制路径策略 | `openwork-tools` + `ToolContext` |
| 形成 Tool Result Message | `SessionActor` + Chat State |
| 保存耗时与错误 | Trace |

V1 多 Tool Call 按 Provider 顺序执行，优先保证消息顺序和副作用可理解。后续若并行化，只能并行已通过权限且互不冲突的调用，结果仍按原始 Tool Call 顺序写回。

V1 没有独立 Workspace Runtime。`ToolContext` 只携带 `working_directory`、Permission Profile 和 Cancellation Token；用户授权等待仍由 `SessionActor` 托管。

## 9. Message 与流式草稿

模型流中可以出现：

- Text Delta；
- Reasoning Delta；
- Tool Call 参数 Delta；
- Usage/Finish Reason；
- Provider Opaque Block。

处理分两层：

```text
Streaming Draft (内存)
  -> Live SessionUpdate
  -> 完整响应结束
      -> immutable Assistant Message
      -> Chat State
      -> messages 表
```

不把每个 Delta 写入数据库。这样避免 Session history、Trace 和 UI 更新混在同一套表里。

Tool Call 只有在 ID、名称和完整参数可解析后才进入 Assistant Message。若流中断，草稿可以丢弃，Turn 失败；不能执行不完整 Tool Call。

## 10. SessionUpdate

`SessionUpdate` 是 Desktop 的进程内 Live 协议：

```text
turn_started
phase_changed
text_delta
reasoning_delta
tool_call_started / tool_call_input_delta / tool_call_finished
tool_result
permission_requested / permission_resolved
turn_completed / turn_failed / turn_cancelled
```

规则：

- Wire Envelope 包含 `version/sessionId/turnId/sequence/occurredAt/update`；
- `sequence` 由 SessionActor 按 Session 单调分配；
- 高频 Delta 可以在内存中合并；
- 保留一个有界 Ring Buffer，支持 Desktop 在同一进程内短暂重连；
- 超出缓冲区后，Desktop 读取 `SessionSnapshot` 和已完成 Message；
- V1 不建立 `session_updates` 表；
- Update 缺失不能改变 Turn 状态。

React 侧用纯 Reducer 按 `sequence` 去重和发现缺口；Canonical Message 与 Live Draft 不放进同一个 Store。Host Contract 和前端状态设计见 [06-frontend-architecture.md](06-frontend-architecture.md)。

## 11. SessionSnapshot 保存在哪里

V1 的运行快照只存在于活动 `SessionActor` 内存中：

```rust
struct SessionSnapshot {
    session_id: SessionId,
    active_turn_id: Option<TurnId>,
    phase: SessionPhase,
    draft_text: String,
    draft_reasoning: String,
    tool_calls: Vec<LiveToolCall>,
    pending_permission: Option<PermissionRequest>,
    last_update_sequence: u64,
}
```

它用于同进程 UI 重连，不用于进程重启恢复。数据库不会保存 `runtime_state` JSON，也不会保存 Pending Permission。

## 12. 持久化数据与真相来源

| 问题 | 真相来源 |
| --- | --- |
| Session 元数据 | `sessions` |
| 一次 Turn 的终态 | `turns` |
| 下一次模型可见历史 | Chat State + `messages` |
| 当前进程中的活动 Phase | `SessionActor` |
| Desktop Live 内容 | `SessionUpdate`/Snapshot |
| Model/Tool 的耗时与错误 | `trace_spans` |

禁止：

- 从 Trace 恢复 Tool Call；
- 从 Live Update 推导 Turn 终态；
- 从数据库中的 `running` 状态自动继续 Agent Loop；
- 从 Tool Span 的失败状态判断副作用一定没有发生。

## 13. 进程中断语义

启动时执行一次修正：

```text
turns.status = running
    -> interrupted

trace_spans.status = running
    -> outcome_unknown
```

之后：

- 不重建活动 Turn；
- 不恢复 Permission Waiter；
- 不重新发送 Provider Request；
- 不自动执行缺失结果的 Tool Call；
- 对已经持久化但没有 Tool Result 的 Tool Call，追加一个合成的 `outcome_unknown` Tool Result，明确说明副作用可能已经发生、不得自动重试；
- 用户继续时创建新的 Turn，并可在新输入中说明上次被中断。

追加 `outcome_unknown` 只是修复 Conversation 的协议完整性，不会继续旧 Turn，也不是恢复执行。这样下一次 Model Request 不会携带“只有 Tool Call、没有 Tool Result”的非法历史。

## 14. 降级规则

| 失败 | Runtime 行为 |
| --- | --- |
| User Message/Turn Start 写入失败 | 不调用模型 |
| Assistant Tool Call Message 写入失败 | 不执行工具 |
| Tool Result Message 写入失败 | 不进行下一次模型调用；Turn 失败 |
| Trace 队列满/数据库失败 | 主循环继续，增加采集丢弃计数 |
| Live Update 接收者断开 | 主循环继续，Snapshot 仍可读取 |
| Model 流中断 | 丢弃未完成草稿，Turn 失败 |
| Tool 返回错误 | 写入错误 Tool Result，继续让模型决策 |
| 工具副作用后进程崩溃 | 下次启动标记中断，不自动重试 |

## 15. 最小行为测试

1. 模型无 Tool Call，一次调用完成；
2. 单 Tool Call：Model → Tool Result → Model → Final；
3. 一次响应多个 Tool Call，结果按原顺序回填；
4. Unknown Tool 和 Invalid Input 被写成 Tool Result，模型可继续；
5. Tool Error 后仍发生下一次 Model Call；
6. Permission Allow 后执行；Deny 后终止且不执行；
7. 用户取消等待中的模型或工具；
8. 连续同名同参达到阈值后 doom-loop 终止；
9. 达到最大 Model Call 次数后失败；
10. Trace Recorder 全部报错时主循环结果不变；
11. Assistant Message 未持久化时工具绝不执行；
12. 启动时 `running` Turn 变为 `interrupted`，悬空 Tool Call 获得 `outcome_unknown` Tool Result，且旧 Turn 不会被调度。
