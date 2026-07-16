# OpenWork Trace Bata Run/Event 升级设计

> 状态：架构决策已确认，代码与数据库迁移待实现
>
> 最后核对：2026-07-16
>
> 定位：本地、语义化、按需深入的 Agent Trace 调试器
>
> 数据边界：默认只捕获诊断元数据；不持久化原始 Token/SSE、完整 Provider Request/Response、System Prompt 或 Tool Schema

本文是 OpenWork 唯一的 Trace 设计说明。它同时记录当前七类 Span 基线、已经确认的 Run/Event 目标模型、数据库迁移和验收边界，避免继续维护 Trace v1、v1.1 或其他并行设计稿。

文中使用以下状态标记：

- **当前**：工作区代码已经实现；
- **目标**：本次设计已确认，但尚未实现；
- **不做**：当前阶段明确排除。

## 1. 决策摘要

本次升级采用接近 LangSmith 的统一模型：

```text
Trace
└─ Run tree
   ├─ chain
   ├─ llm
   └─ tool
      └─ Events
```

关键决定：

1. Trace 是一次完整 Agent Turn 的诊断集合，根 Run 的 `run_id` 等于 `trace_id`；
2. Trace 树只允许 `chain`、`llm`、`tool` 三类 Run；
3. `Step` 从 Trace 删除，但继续保留在 Agent Loop 和 Event Journal；
4. Transport Attempt、Approval、Recovery 不再占据树节点，统一建模为所属 Run 的 Event；
5. Trace 公共合同只使用 Run 术语，不再长期保留 `Span`/`Run` 两套命名；
6. 新 `trace_id`、`run_id`、`event_id` 使用 UUIDv7；
7. Recorder 合同升级为 `StartRun`、`UpdateRun`、`AppendRunEvent`、`CloseTrace`；
8. Run 保存当前状态和最终摘要，Event 以追加方式保存执行过程；
9. Runtime Capture Health 按 `trace_id` 归属，明确区分完整、部分和未知 Trace；
10. Tree 与 Waterfall 分开，默认展示可折叠 Tree，耗时紧跟 Run 名称，不单独设置 Duration 列；
11. 当前不记录逐 Token 流式 Event，只记录审批、重试、恢复和 Provider 阶段里程碑；
12. 当前不实现 Cost、原始 SSE、分布式 Trace、团队 Dashboard、告警和公开分享。

## 2. 当前基线与目标差异

### 2.1 当前实现

当前代码已经具备真实 Trace 链路：

- `TraceSpanKind` 包含 `Turn`、`Step`、`ModelAttempt`、`TransportAttempt`、`ToolRun`、`Approval`、`Recovery`；
- `TraceSignal` 只有 `Start` 和 `Update`；
- PostgreSQL 使用 `trace_spans`；
- Runtime 使用有界队列和单 Worker 归并 Span；
- Desktop 已有列表、会话内入口、可折叠 Tree、独立 Waterfall 和类型化详情；
- Trace 是 Best effort，不参与 Turn 恢复决策。

当前事实以这些位置为准：

- `crates/openwork-protocol/src/trace/mod.rs`；
- `crates/openwork-observability/src/runtime.rs`；
- `crates/openwork-persistence/src/postgres/migrations/trace_spans.rs`；
- `crates/openwork-persistence/src/postgres/trace_span/repository.rs`；
- `crates/openwork-app/src/trace_service.rs`；
- `apps/desktop/src/components/trace/TurnTracePanel.tsx`。

### 2.2 目标映射

| 当前 | 目标 | 处理方式 |
| --- | --- | --- |
| `TraceSpan` | `TraceRun` | 直接改名，不保留公共别名 |
| `span_id` | `run_id` | 新数据使用 UUIDv7 |
| `parent_span_id` | `parent_run_id` | 指向父 Run |
| `span_kind` | `run_type` | 只允许 `chain/llm/tool` |
| `span_name` | `name` | Run 稳定名称 |
| `Turn` | `chain` | Trace Root |
| `ModelAttempt` | `llm` | 一次逻辑模型调用 |
| `ToolRun` | `tool` | 一次工具请求及其终态 |
| `Step` | 不进入 Trace | 保留在 Agent/Journal |
| `TransportAttempt` | LLM Run Event | Provider 尝试、失败与重试 |
| `Approval` | Tool Run Event | 请求、决策和等待耗时 |
| `Recovery` | Root Chain Event | 恢复开始、完成和失败 |
| `trace_spans` | `trace_runs` | 新 Run 主表 |
| 无独立 Event 表 | `trace_run_events` | Append-only Run Event |
| 全局 Runtime 统计 | `trace_capture_state` | 按 Trace 记录采集健康度 |

## 3. 数据责任边界

三类数据必须继续分离：

| 数据 | 载体 | 用途 | 可靠性 |
| --- | --- | --- | --- |
| Recorded Event | `recorded_events` | Turn 重建、Step、Message、Tool、Approval 和恢复事实 | 持久化失败会阻止依赖它的业务动作 |
| Trace Run/Event | `trace_runs`、`trace_run_events` | 树、状态、耗时、重试、审批等待、Usage 和错误诊断 | Best effort，允许缺失，不参与恢复决策 |
| Live Event | Tauri `chat-stream-event` | 当前页面流式文本和即时进度 | 页面刷新或进程退出后允许丢失 |

目标数据流：

```text
recorded_events ── Durable Step / Message / Tool / Approval facts ─┐
trace_runs ─────── Tree / Status / Timing / Usage / Error ─────────┼─> TraceApplicationService
trace_run_events ─ Retry / Approval wait / Recovery / Milestones ──┤
trace_capture_state ─ Capture completeness ─────────────────────────┘

chat-stream-event ── Token delta / current progress，不进入历史 Trace
```

Approval 可以同时出现在 Journal 和 Trace，但两者含义不同：

- Journal Approval 是恢复和审计所需的业务事实；
- Trace Approval Event 是用于耗时和诊断展示的投影；
- Trace Event 丢失不能改变审批结果，也不能阻止 Turn 恢复。

## 4. Trace、Run 与 Event

### 4.1 Trace

一个 Turn 对应一个 Trace。Trace 本身不额外成为第四种运行节点；根 `chain` Run 就是 Trace Root：

```text
root.run_id = root.trace_id
root.parent_run_id = null
root.run_type = chain
```

`turn_id` 继续是 Agent/Journal 的领域 ID，`trace_id` 是观测模型 ID。二者可以互相关联，但不再要求字符串完全相同。

### 4.2 三类 Run

| `run_type` | 含义 | 创建时机 | 典型子 Run |
| --- | --- | --- | --- |
| `chain` | 一段组合流程；当前根节点是 Agent Turn | Turn 开始 | `llm`、`tool` |
| `llm` | 一次逻辑模型调用，包括内部 Provider 重试 | 准备调用模型 | 无；尝试过程使用 Event |
| `tool` | 一次工具请求、审批等待和实际执行 | Tool request 形成 | 当前无；审批使用 Event |

增加新 Run 类型必须满足：它是可以独立命名、计时、结束、失败，并且可能拥有子 Run 的执行单元。仅用于说明阶段变化、等待或重试的信息应优先成为 Event。

### 4.3 Step

Step 不再进入 Trace：

- 不创建 Step Run；
- 不创建 Step Event；
- Trace Tree、Waterfall、摘要和详情不展示 Step；
- Agent Loop 和 `TurnRecordedEvent::StepStarted/Completed/Failed` 保持不变；
- 如需排查业务状态，可通过 `turn_id` 回到 Journal，而不是恢复 Step Span。

这样可以避免把 Agent 内部循环控制结构误当成用户需要理解的调用节点。

### 4.4 Event

Event 是 Run 内部发生的不可变时间点记录。目标最小合同：

```text
TraceRunEvent
  event_id       UUIDv7
  trace_id       UUIDv7
  run_id         UUIDv7
  event_type     string
  occurred_at    timestamp
  sequence_no    integer
  attributes     object
```

约束：

- 每个 Event 必须归属于一个 Run；
- Event 使用 `event_id` 幂等追加，不能覆盖旧 Event；
- `sequence_no` 在一个 Trace 内单调递增，用于同毫秒事件的稳定排序；
- UUIDv7 提供大致时间顺序，但不能代替 `sequence_no`；
- `attributes` 必须是白名单对象，不允许任意 payload；
- Run 的开始、终态和错误由 `StartRun/UpdateRun` 表达，不重复追加通用 `start/end/error` Event。

### 4.5 目标示例

```text
Trace 019...
└─ Agent Turn                                      chain
   ├─ Model: decide whether to call weather        llm
   │  ├─ provider_attempt_started       attempt=1
   │  ├─ provider_attempt_failed        timeout
   │  ├─ retry_scheduled                delay=500ms
   │  ├─ provider_attempt_started       attempt=2
   │  ├─ provider_first_output_received attempt=2
   │  └─ provider_attempt_succeeded      attempt=2
   ├─ Weather Tool                                  tool
   │  ├─ approval_requested
   │  └─ approval_resolved             allowed, wait=3.2s
   └─ Model: answer from weather result             llm

Root chain events when applicable:
  recovery_started
  recovery_completed
```

## 5. LangSmith 参考与 OpenWork 取舍

[LangSmith Run data format](https://docs.langchain.com/langsmith/run-data-format) 将每次执行保存为结构化 Run，包含 ID、父子关系、类型、输入输出、时间、状态、错误、Usage、额外元数据和 Events。根 Run 的 ID 同时是 Trace ID。

[LangSmith Trace with API](https://docs.langchain.com/langsmith/trace-with-api) 使用创建 Run 后再更新同一个 Run 的方式补充输出和结束时间，并建议 Run ID 使用 UUIDv7。

当前 [LangChain tracer 源码](https://github.com/langchain-ai/langchain/blob/7bf8fe22163e5dadce365169e2df6b91233de9c4/libs/core/langchain_core/tracers/core.py) 会在 Run 内记录 `start`、`new_token`、`retry`、`end` 和 `error` 等事件。`new_token` 可能携带 token/chunk 内容。

OpenWork 不复制全部行为：

- 采用 Trace/Run/Events 的清晰数据形状；
- 采用 UUIDv7、父子 Run、创建后更新同一 Run；
- 当前不持久化逐 Token `new_token` Event；
- 当前不复制 LangSmith 的全部 Run 类型，只保留 `chain/llm/tool`；
- 当前不实现 Cost、Feedback、Dataset、Eval、团队协作或远程平台能力；
- LangSmith 对外暴露 `events` 数组不代表其物理数据库一定使用单行 JSON 数组，OpenWork 独立设计 Event 表。

## 6. ID 与顺序

### 6.1 ID

| ID | 类型 | 作用 |
| --- | --- | --- |
| `trace_id` | UUIDv7 | 整个 Trace；等于根 Run ID |
| `run_id` | UUIDv7 | 一个 `chain/llm/tool` Run |
| `parent_run_id` | UUIDv7 NULL | 直接父 Run |
| `event_id` | UUIDv7 | 一条 Run Event |
| `session_id` | TEXT | Session 领域 ID，不在本次迁移 |
| `turn_id` | TEXT | Turn 领域 ID，不在本次迁移 |
| `source_tool_run_id` | TEXT NULL | 与 Journal ToolRun 的关联 ID |

如果未来接入 OpenTelemetry，应额外生成或映射 OTel 规定的 Trace/Span ID，不能直接把 UUIDv7 `run_id` 当作 OTel Span ID。

### 6.2 Run 顺序

当前阶段不复制 LangSmith `dotted_order`。树由 `trace_id + parent_run_id` 构建，同级 Run 使用：

```text
started_at ASC, run_id ASC
```

UUIDv7 只作为相同时间下的稳定补充排序。若以后出现跨进程批量摄取或无法从父关系稳定重建顺序，再单独评估 `dotted_order`。

## 7. Recorder 合同

目标信号合同：

```text
TraceCommand
  StartRun(TraceRunStart)
  UpdateRun(TraceRunUpdate)
  AppendRunEvent(TraceRunEvent)
  CloseTrace(TraceClose)
```

### 7.1 `StartRun`

创建 Run 初始记录：

- 必须携带 `trace_id`、`run_id`、`parent_run_id`、`run_type`、`name`、`started_at`；
- 初始状态通常是 `running`，等待审批的 Tool 可以更新为 `waiting`；
- 同一 `run_id` 重复 Start 必须幂等，保留更早的 `started_at`；
- 根 Run 必须先于正常子 Run 创建，但 Trace 仍需容忍 Best-effort 丢失造成的孤儿记录。

### 7.2 `UpdateRun`

更新同一 Run 的当前摘要，不创建新节点：

- 必须同时携带 `trace_id` 和 `run_id`；
- 可以更新状态、结束时间、输出、Usage、首输出时间和错误；
- 不允许把已结束 Run 重新改回 `running`；
- 相同终态 Update 必须幂等；
- 失败的 Provider Attempt 不直接把 LLM Run 置为失败，只有重试耗尽或最终模型失败才结束 LLM Run。

### 7.3 `AppendRunEvent`

追加 Run 的过程历史：

- `event_id` 是幂等键；
- 已存在同一 `event_id` 时不覆盖；
- Event 不直接决定 Agent 业务状态；
- Event 可以促使 Application 更新 Run 摘要，例如首次输出 Event 同时更新 `first_output_at`。

### 7.4 `CloseTrace`

`CloseTrace` 表示 Trace Runtime 已看到该 Trace 的业务终态，并准备完成当前有界 Flush：

- 单 Worker 必须先处理队列中该 Trace 更早的命令，再确认 Close；
- 成功处理后写入 `trace_capture_state.closed_at`；
- Close/Flush 超时只影响 Capture Health，不能改变 Turn 业务结果；
- 根 Run 已结束但长期没有 `closed_at` 时，Trace 应判为 `partial`；
- `CloseTrace` 不是新的 Run，也不是 Recovery Event。

## 8. Event 类型

### 8.1 Provider 与重试

| `event_type` | 所属 Run | 最小属性 |
| --- | --- | --- |
| `provider_attempt_started` | `llm` | `attempt`、`providerId` |
| `provider_first_output_received` | `llm` | `attempt` |
| `provider_attempt_failed` | `llm` | `attempt`、`failurePhase`、`deliveryState`、`retryable`、受限错误 |
| `retry_scheduled` | `llm` | `attempt`、`nextAttempt`、`delayMs`、`reason` |
| `provider_attempt_succeeded` | `llm` | `attempt`、`providerRequestId` |

规则：

- 每次真实网络尝试用 Started 与 Succeeded/Failed Event 表达；
- Waterfall 根据事件对在同一个 LLM Run 行内绘制尝试子区间；
- `provider_first_output_received` 只记录时间，不保存 token；
- `provider_attempt_failed` 后仍可能重试成功，因此它不是 Trace 顶层错误；
- 最终失败仍写入 LLM Run 的 `status/error_*`。

### 8.2 Approval

| `event_type` | 所属 Run | 最小属性 |
| --- | --- | --- |
| `approval_requested` | `tool` | `approvalId`、`toolName`、`reason`、`policy` |
| `approval_resolved` | `tool` | `approvalId`、`decision`、`waitMs` |

`decision` 允许：

```text
allowed | denied | cancelled | expired
```

Tool Run 在等待期间使用 `waiting`；允许后回到 `running`，拒绝后进入 `denied`，取消或无法确认结果时使用对应终态。

### 8.3 Recovery

| `event_type` | 所属 Run | 最小属性 |
| --- | --- | --- |
| `recovery_started` | Root `chain` | `reason`、可选 `approvalId` |
| `recovery_completed` | Root `chain` | `resumed`、可选 `approvalId` |
| `recovery_failed` | Root `chain` | 归一化错误 |

Recovery Event 只描述恢复编排阶段。恢复后真实发生的模型和工具调用仍创建正常 `llm/tool` Run，不嵌套 Recovery Run。

## 9. Run 数据合同

目标 `TraceRun` 至少包含：

```text
identity
  run_id / trace_id / parent_run_id
  run_type / name / status

correlation
  session_id / turn_id / source_tool_run_id

timing
  started_at / ended_at / first_output_at

model summary
  provider_id / model
  input_tokens / output_tokens

payload
  inputs_json / outputs_json / metadata_json

error
  error_type / error_code / error_message
```

Run 状态沿用 OpenWork 现有诊断语义：

```text
running
waiting
succeeded
failed
cancelled
denied
outcome_unknown
```

不强制复制 LangSmith 的状态字符串，因为 `waiting`、`denied` 和 `outcome_unknown` 对本地 Tool/Approval 调试仍有价值。

### 9.1 Inputs 与 Outputs

`inputs_json` 和 `outputs_json` 允许为 NULL：

- `captureMode=metadata_only` 时默认不复制完整模型或工具内容；
- Tool input/Observation 仍可通过 `source_tool_run_id` 从 Journal 延迟读取；
- 以后增加脱敏 Payload 时不需要再次修改 Run API 形状；
- 输入输出是否持久化必须经过单独的白名单和隐私评审。

### 9.2 Metadata

`metadata_json` 只保存不适合成为高频查询列的白名单字段，例如：

```text
traceSchemaVersion
instrumentationVersion
appVersion
captureMode
approvalPolicy
finishReason
rawFinishReason
providerRequestId
request shape summary
```

经常用于列表过滤或排序的 `provider_id`、`model`、Token 和时间字段应成为明确列，不继续全部埋在 JSON 中。

## 10. PostgreSQL 目标 Schema

### 10.1 `trace_runs`

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `run_id` | UUID PK | UUIDv7 Run ID |
| `trace_id` | UUID | 根 Run ID |
| `parent_run_id` | UUID NULL | 直接父 Run 的逻辑引用 |
| `run_type` | TEXT | `chain/llm/tool` |
| `name` | TEXT | 稳定内部名称 |
| `status` | TEXT | 诊断状态 |
| `session_id` | TEXT | Session 关联 |
| `turn_id` | TEXT | Turn 关联 |
| `source_tool_run_id` | TEXT NULL | Journal ToolRun 关联 |
| `started_at` | TIMESTAMP | 开始时间 |
| `ended_at` | TIMESTAMP NULL | 结束时间 |
| `first_output_at` | TIMESTAMP NULL | 首次语义输出时间 |
| `provider_id` | TEXT NULL | Provider |
| `model` | TEXT NULL | Model |
| `input_tokens` | BIGINT NULL | 输入 Token |
| `output_tokens` | BIGINT NULL | 输出 Token |
| `inputs_json` | JSONB NULL | 可选、脱敏输入 |
| `outputs_json` | JSONB NULL | 可选、脱敏输出 |
| `metadata_json` | JSONB | 白名单元数据对象 |
| `error_type` | TEXT NULL | 归一化错误类型 |
| `error_code` | TEXT NULL | 归一化错误码 |
| `error_message` | TEXT NULL | 截断后的错误信息 |
| `created_at` | TIMESTAMP | 创建时间 |
| `updated_at` | TIMESTAMP | 更新时间 |

目标约束：

- 根 Run：`run_id = trace_id AND parent_run_id IS NULL AND run_type = 'chain'`；
- 子 Run：`parent_run_id IS NOT NULL`；
- `ended_at >= started_at`；
- `first_output_at >= started_at`；
- Token 不能为负数；
- JSON 字段为 NULL 或 object；
- 不对 `parent_run_id` 建立硬外键：Best-effort 丢失可能先留下子 Run，Repository 必须允许保存孤儿证据并把 Trace 标记为 `partial`。

### 10.2 `trace_run_events`

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `event_id` | UUID PK | UUIDv7 幂等键 |
| `trace_id` | UUID | 所属 Trace |
| `run_id` | UUID | 所属 Run 的逻辑引用 |
| `event_type` | TEXT | 白名单 Event 类型 |
| `occurred_at` | TIMESTAMP | 发生时间 |
| `sequence_no` | BIGINT | Trace 内稳定顺序 |
| `attributes_json` | JSONB | 白名单事件属性 |
| `created_at` | TIMESTAMP | 入库时间 |

Event 独立成表，而不是在 `trace_runs` 中维护不断增长的 JSON 数组，原因是：

- Append 不重写 Run 行；
- 支持 `hasRetry`、Approval 和 Recovery 过滤；
- 支持稳定时间线和 Waterfall 子区间；
- 可以用 `event_id` 去重；
- Retention 可以在同一事务中按 `trace_id` 清理全部 Event。

Event 表同样不对 `run_id` 建立硬外键。若 `StartRun` 丢失而 Event 已到达，保留孤立 Event 比让数据库约束再次丢弃诊断证据更有价值；Application 将其标记为 `missing_run` 并把 Trace 判为 `partial`。

### 10.3 `trace_capture_state`

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `trace_id` | UUID PK | Trace ID |
| `dropped_signal_count` | BIGINT | 未进入 Worker 的命令数量 |
| `persistence_error_count` | BIGINT | Repository 写入失败数量 |
| `orphan_update_count` | BIGINT | 找不到对应 Start Run 的 Update 数量 |
| `flush_timeout_count` | BIGINT | Close/Flush 超时数量 |
| `closed_at` | TIMESTAMP NULL | Runtime 成功处理 CloseTrace 的时间 |
| `updated_at` | TIMESTAMP | 最后更新时间 |

Capture State 不以 Root Run 外键为前提：即使 Root Run 写入失败，也要尽可能留下“这个 Trace 发生过采集损失”的证据。

### 10.4 索引

最小索引：

```text
trace_runs(session_id, started_at DESC, run_id)
trace_runs(turn_id, started_at, run_id)
trace_runs(trace_id, parent_run_id, started_at, run_id)
trace_runs(model, started_at DESC)
trace_runs(status, started_at DESC)

trace_run_events(trace_id, sequence_no) UNIQUE
trace_run_events(run_id, occurred_at, event_id)
trace_run_events(event_type, occurred_at DESC)
```

Root Trace 列表可增加 `WHERE parent_run_id IS NULL` 的部分索引，避免扫描全部子 Run。

## 11. 数据库迁移

当前 `trace_spans` 使用 TEXT ID，并存在无法转换为 UUID 的组合 ID，例如 Transport/Recovery Span ID。因此不采用原表原地类型转换。

目标迁移策略：

1. 新增更高版本 migration，不修改已经执行过的 `create_trace_spans` 版本；
2. 创建 `trace_runs`；
3. 创建 `trace_run_events`；
4. 创建 `trace_capture_state`；
5. 更新 `REQUIRED_TABLES` 和 schema verification；
6. 切换 Repository、Application 和 Desktop 合同；
7. 删除旧 `trace_spans`，不保留兼容 View 或双写；
8. 保留 `recorded_events` 及所有 Agent/Journal 数据；
9. 更新 `docs/local-postgres.md`、架构说明和测试。

当前阶段推荐不回填旧 Trace：

- Trace 是可丢弃的 Best-effort 诊断数据；
- 旧七类树到新 Run/Event 不是一对一字段改名；
- 旧组合 ID 需要额外 UUID 映射；
- 回填会引入只服务于开发历史数据的兼容逻辑；
- 保留 Journal 即可保证 Session、Turn、Step、Tool 和 Approval 事实不丢失。

如果必须保留旧 Trace，应另行设计一次性导出/转换工具，而不是让生产代码长期兼容 `TraceSpan`。

## 12. Capture Health 与完整性

当前 Runtime 统计是进程全局计数，无法证明某一条 Trace 是否完整。目标实现需要在命令入队、处理和持久化失败时尽可能携带 `trace_id` 并更新对应 Capture State。

API 返回：

```text
captureStatus
  capturing
  complete
  partial
  unknown

captureReasons
  signal_dropped
  persistence_error
  orphan_update
  flush_timeout
  trace_not_closed
  missing_root
  missing_parent
  missing_run
```

推导规则：

- 根 Run 仍为 `running/waiting`：`capturing`；
- 根 Run 已终态、`closed_at` 存在、四项计数为 0、父子关系完整：`complete`；
- 存在已知丢失、写入失败、孤儿、超时、缺根、缺父，或根已终态但未 Close：`partial`；
- Run 存在但没有足够 Capture 数据解释，或 Trace 数据整体不存在：`unknown`。

`active_runs` 继续作为 Runtime 进程指标，不持久化到每个 Trace。

## 13. 查询与 Application API

目标 Tauri/Application 查询：

| Command | 返回内容 |
| --- | --- |
| `trace_session` | Session 下的 Trace 摘要 |
| `trace_list` | 跨 Session 的游标分页 Trace 摘要 |
| `trace_turn` | 一条 Trace 的轻量 Run 树和 Event 标记 |
| `trace_run_detail` | 选中 Run 的详情和 Event 时间线 |

删除 `trace_span_detail` 命名，不保留长期别名。

`TraceListQuery`：

```text
query
sessionId / project
model / status
startedAfter / startedBefore
hasError / hasRetry
limit / cursor
```

查询规则：

- 先筛选 Root Chain，再分页，再加载入选 Trace 的子 Run；
- 使用基于 `started_at + trace_id` 的游标，不继续扩大 offset 分页；
- `hasRetry` 查询 `retry_scheduled` Event；
- `hasError` 查询最终失败 Run，不把后来恢复的 Provider Attempt Failure 当作 Trace 失败；
- Model、Status、时间等高频条件使用明确列；
- 关键字搜索继续限制在安全摘要和现有 Session/Message 投影，不扫描原始 Provider payload。

目标摘要字段：

```text
traceId / turnId / sessionId
sessionTitle / workingDir / inputPreview
status / model / startedAt / endedAt / durationMs
runCount / modelRunCount / toolRunCount
providerAttemptCount / approvalCount / retryCount / errorCount
inputTokens / outputTokens / recovered
diagnosis / captureStatus / captureReasons
```

Trace 摘要不再包含 `stepCount` 和 `transportSpanCount`。

## 14. Desktop 展示

### 14.1 Tree

Tree 是默认视图：

- 只显示 `chain/llm/tool` Run；
- 按真实 `parent_run_id` 缩进；
- 默认展开全部节点；
- 有子 Run 的行可以独立折叠；
- 折叠只隐藏后代，不改变当前选择；
- 耗时紧跟 Run 名称，不设置独立 Duration 列；
- 不显示 Step、Transport Attempt、Approval 或 Recovery 节点。

### 14.2 Waterfall

Waterfall 与 Tree 是独立视图，共享 Run 选择：

- 一行对应一个 Run；
- LLM 行内使用子区间表示 Provider Attempt；
- Tool 行内使用等待区间表示 Approval；
- Root Chain 行使用标记表示 Recovery；
- 条的起点和宽度表达真实开始时间与持续时间；
- 失败的最终 Run 优先使用错误色，已恢复的 Attempt Failure 使用低强度警示标记；
- 保留亮色和暗色语义色适配。

### 14.3 Detail

Run Detail 按稳定分组展示：

- **概览**：类型、状态、名称、耗时、Provider/Model；
- **输入与输出**：脱敏 Run payload，或从 Journal 延迟加载的 Tool 内容；
- **Events**：按 `sequence_no` 展示重试、审批、恢复和 Provider 里程碑；
- **Usage**：输入/输出 Token 和首输出耗时；
- **元数据**：稳定 ID、时间、版本和白名单 Metadata；
- **错误**：归一化错误类型、错误码和截断信息。

Tool input 中字符串形式的 `content` 继续按正文或源码展示，不显示成 JSON 转义字符串。

## 15. Best-effort Runtime 与 Retention

保留以下原则：

- `record()` 不阻塞 Agent；
- 有界队列拥塞时允许丢 Trace 命令；
- 单 Worker 维持本地命令顺序；
- Repository 错误不能返回给 Agent 业务控制流；
- Close/Flush 有时间上限；
- Event 通过 `event_id` 幂等，Run 通过 `run_id` 幂等；
- Retention 在同一事务中按 `trace_id` 删除 Root、子 Run、Events 和 Capture State；
- 自动 Retention 调度和产品默认保留期仍不在本次范围。

## 16. 隐私与安全

默认禁止写入 `trace_runs` 和 `trace_run_events`：

- API Key、Authorization、Cookie；
- 数据库连接串和完整环境变量；
- 完整 System Prompt、模型输入和模型输出；
- Tool Schema、完整文件正文和无限制命令输出；
- 原始 Provider Header、Request/Response Body；
- 原始 SSE、逐 Token 或逐 Chunk 内容。

安全要求：

- `metadata_json` 和 Event `attributes_json` 使用类型化白名单；
- 错误信息继续限制长度；
- Provider Request ID 可以保存，认证 Header 不允许保存；
- `first_output_at` 只保存时间；
- `inputs_json/outputs_json` 默认 NULL，仅在后续明确开启脱敏 Capture 时写入；
- Tool input、Observation 和 Message 默认仍由 Journal 按需读取，不复制到 Trace。

## 17. 模块所有权

| 模块 | 目标责任 |
| --- | --- |
| `openwork-protocol` | Run/Event/Status、四类 Recorder Command、Repository、Filter 和 Retention 合同 |
| `openwork-core` | LLM Run、Provider Attempt/Retry Event 和 Request shape 摘要 |
| `openwork-providers` | 真实 Provider Attempt、首输出、Retry 和归一化错误信号 |
| `openwork-observability` | Journal 后置投影、有界队列、Run 更新、Event 追加、CloseTrace 和 Capture Health |
| `openwork-persistence` | 三张 Trace 表的 migration、Repository、查询、游标和 Retention |
| `openwork-app` | Root Chain/Recovery Event、摘要、Diagnosis、Capture Status、Journal 关联和 Run Detail |
| `apps/desktop/src-tauri` | 薄 Tauri Trace command |
| `apps/desktop/src` | 列表、Chat 入口、Tree、Waterfall、Run Detail 和 Event Timeline |

Core、Provider 和 Application 不依赖 PostgreSQL 具体实现；Trace 失败不得改变 Agent 结果。

## 18. 备选方案与取舍

### 18.1 保留七类 Span

- 优点：迁移工作少，当前实现可以继续使用；
- 缺点：Step、Transport、Approval、Recovery 与真实调用节点混在同一树中，用户难以理解；
- 结论：不采用。只保留三类 Run，过程信息进入 Event。

### 18.2 只保留三类 Run，删除其他信息

- 优点：模型最简单；
- 缺点：失去重试、审批等待和恢复诊断；
- 结论：不采用。删除的是树节点，不是诊断证据。

### 18.3 把 Events 保存为 Run 行内 JSON 数组

- 优点：读取一个 Run 时结构直观；
- 缺点：每次 Append 重写 Run 行，难以去重、筛选和稳定分页；
- 结论：数据库使用独立 Event 表，API 可以聚合成 Run 的 `events` 数组。

### 18.4 长期兼容 Span 与 Run 两套命名

- 优点：旧调用方迁移容易；
- 缺点：协议、数据库、Application 和 UI 会持续出现两套词汇；
- 结论：不采用。一次性完成命名切换。

### 18.5 回填旧 Trace

- 优点：保留开发历史；
- 缺点：需要转换树结构、生成 UUID 映射并将三个旧 Span 类型拆成 Event；
- 结论：默认不做。只保留 Durable Journal。

## 19. 后果与风险

### 19.1 正向后果

- Tree 只表达用户能理解的真实调用单元；
- Retry、Approval 和 Recovery 仍保留完整诊断信息，但不会使调用树膨胀；
- Run 当前摘要与 Event 过程历史分工明确；
- UUIDv7、明确列和游标查询为后续数据增长提供稳定基础；
- Capture Health 不再把结构看起来正常误判为绝对完整；
- Agent/Journal 与 Trace 的事实边界更清楚。

### 19.2 成本

- Protocol、Runtime、Persistence、Application 和 Desktop 都需要同步迁移；
- 数据库从一张 Trace 表增加到三张；
- Waterfall 需要根据 Event 对计算 Attempt 和 Approval 子区间；
- 当前开发 Trace 历史默认不保留；
- 实施期间必须同时维护旧代码可运行和新合同可验证，最终再一次性删除旧实现。

### 19.3 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| Event 数量失控 | 使用封闭 Event 白名单，不记录逐 Token/SSE |
| Run Start 丢失而 Update/Event 存在 | 不使用硬外键丢弃证据，按 Trace 标记 `partial` |
| Journal 与 Trace Approval 不一致 | Journal 永远是事实源，Trace 只在 Recorded Event 成功后投影 |
| UUIDv7 与旧组合 ID 无法转换 | 不回填旧 Trace，只保留 Durable Journal |
| Migration 误伤业务数据 | Migration 只删除 `trace_spans`，为 `recorded_events` 增加保留断言和集成测试 |
| Provider Event 泄漏内容 | 只记录阶段、计数、状态和受限错误，不保存 token/payload |
| Event 时间顺序相同 | 使用 Trace 内 `sequence_no`，不只依赖时间戳或 UUIDv7 |

## 20. 实施顺序

1. 在 `openwork-protocol` 定义 `TraceRun`、`TraceRunEvent` 和四类 Command；
2. 启用 UUIDv7，并统一新 Trace ID 生成；
3. 新增三张表和 PostgreSQL Repository；
4. 把 Core/Provider/Application 的七类 Span instrumentation 改成三类 Run 和 Event；
5. 实现 Trace 级 Capture Health 与 `CloseTrace`；
6. 改造 Application 查询、游标分页、Diagnosis 和详情；
7. 改造 Tauri/Desktop 的 Run 类型、Tree、Waterfall 和 Event Timeline；
8. 删除旧 Span 代码、旧表、旧测试命名和过时文档；
9. 执行全量单元、集成、前端和 Docker PostgreSQL 验证。

在第 3–7 步完成前，不应删除旧实现；最终合并时不保留双写或兼容 View。

## 21. 验收标准

- 数据库只接受 `chain/llm/tool` 三类 Run；
- 新公共协议、Repository、Application API 和 Desktop 类型不再使用 Span 命名；
- 新 Trace/Run/Event ID 都是有效 UUIDv7；
- 根 `run_id` 等于 `trace_id`；
- Step 仍能从 Journal 重建，但 Trace 中没有 Step Run/Event；
- Provider 重试完整显示 Attempt、失败、延迟和最终结果；
- Approval 等待区间与 Tool 执行区间可以区分；
- Recovery 以 Root Event 显示，恢复后的模型和工具仍是正常 Run；
- 重复 `StartRun/UpdateRun/AppendRunEvent` 不产生重复节点或 Event；
- Tree 默认展示、可以折叠，Waterfall 独立展示；
- Trace List 使用游标并支持 Model、Status、时间、Error 和 Retry 过滤；
- Capture Status 能区分 `capturing/complete/partial/unknown`；
- Trace 写入失败、队列丢弃或 Flush 超时不改变 Agent 业务结果；
- `recorded_events` 和恢复语义不因 Trace migration 受损；
- 默认数据库中不存在原始 Token/SSE 或未脱敏 Provider payload。

建议验证命令：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
cargo run -p openwork-persistence --bin openwork-migrate
```

数据库集成测试必须连接本地 PostgreSQL，并覆盖 migration、Run upsert、Event 幂等追加、Capture Health、游标查询和整条 Trace Retention。

## 22. 当前不实现

- 逐 Token/Chunk Event 和原始 SSE 持久化；
- Exact Provider Request/Response、System Prompt、Tool Schema 和原始 Header；
- 独立 Artifact/Blob、相邻 Attempt Payload Diff 和自包含 HTML 导出；
- Cost 与价格版本；
- Feedback、Dataset、Annotation 和 Eval；
- 团队 Dashboard、告警和公开分享；
- OpenTelemetry Export 和分布式 Trace；
- 多进程/多主机 Trace ingestion；
- 自动 Retention 调度和产品默认保留期；
- 全文索引和面向大规模 SaaS 的搜索基础设施。

如果这些边界变化，应直接更新本文和对应 migration/测试，不再新增并行 Trace 版本文档。
