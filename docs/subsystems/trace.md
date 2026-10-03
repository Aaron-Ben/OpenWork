# Trace

本页描述 Trace：Model Call、Tool Call 与压缩留下的记录。它回答模型看到了什么、回复了什么、用了多少 token。Span 类型与 Guard 在 `openwork-core` 的 `src/session/trace.rs`，写入与查询在 `src/storage/`，界面在 `desktop/src/features/traces/`。

Trace 写入是 best-effort。丢失一条 Span 只让排查更难，不改变任何业务结果（§6）。

## 1. 范围

| 问题 | 数据来源 |
|---|---|
| 模型收到的完整请求 | `request`、`system_context`、`tool_definitions` 三个正文槽位（§5） |
| 模型回复了什么 | `response_message_id` 指向的 Message；没有 Message 时用 `response` 槽位 |
| 用了多少 token | Span 的四个 token 列；Turn 的同名列 |
| 请求参数 | `temperature`、`topP`、`maxOutputTokens`、`thinkingMode`、`toolChoice` |
| 哪里慢、哪里出错 | 分段耗时、`errorPhase`、`deliveryState`、`error_code` |
| 压缩为什么触发、回收了多少 | Compaction Span 的触发证据与前后 token（§7.3） |

Trace 不做这些事：恢复未完成的 Turn；推导 Tool Call 的副作用是否发生；决定是否重试；让 Turn 失败。

领域 Trace 存在 PostgreSQL，是本地产品数据。`openwork-core` 依赖 Rust `tracing`，只用来输出运行日志。`attempt_count`、`denied`、`outcome_unknown`、完整度都由类型化 Guard 显式写入，不从日志推导。项目没有 OpenTelemetry 依赖，出口的设计见 [Agent Note：tracing 与 OTLP 出口](../../.agents/notes/proposed/architecture/2026-07-27-trace-tracing-otlp-export.md)。

理由见 [Agent Note：Trace 是质量追踪](../../.agents/notes/implemented/architecture/2026-07-27-trace-quality-tracking.md)。

## 2. 标识

| 标识 | 作用 | 外键 | 可空 |
|---|---|---|---|
| `trace_id` | 结构根。一次用户请求的全部 Span 共享它；一次无 Turn 的独立操作也一样 | 无 | 否，且不能是空白 |
| `id` | Span 自身 | 主键 | 否 |
| `parent_span_id` | 发起关系（§3.1） | 无 | 是 |
| `session_id` | 业务标签 | `sessions(id)`，级联删除 | 否 |
| `turn_id` | 业务标签：操作是否在某个 Turn 内 | `(turn_id, session_id)` → `turns`，级联删除 | 是 |

`trace_id` 的取值：

| 场景 | 取值 | 分配者 |
|---|---|---|
| Turn 内的任何 Span | 等于该 Turn 的 `turn_id` | `TurnRunner::trace_id` |
| 手动压缩、rewind | 新生成的 `trace-<uuid>`（`new_trace_id`） | `SessionActor` |
| 子 Span | 继承父 Span 的 `trace_id` | 父 Span 的执行体 |

手动压缩与 rewind 的 `turn_id` 为空。一次操作中途不更换 `trace_id`。`trace_annotations.trace_id` 与 `sessions.spawn_span_id` 也没有外键。

理由见 [Agent Note：Trace 的标识与外键](../../.agents/notes/implemented/architecture/2026-07-27-trace-identifiers-and-foreign-keys.md)。

## 3. Span 关系、行数与排序

### 3.1 发起关系

`parent_span_id` 表示“谁发起了谁”，不表示“谁包含谁”。

| 关系 | 规则 |
|---|---|
| Tool Call → Model Call | Tool Span 的父是请求它的那次 Model Call |
| 摘要采样 → Compaction | 摘要采样的 Model Span 的父是 Compaction Span |
| Compaction | 没有父 |
| overflow 的失败 Model Call | 不是父。它的 Span id 记在 Compaction 的 `triggerModelSpanId` |

Tool Span 在 Model Span 结束之后才开始。只有摘要采样在时间上嵌在父 Span 之内。

```text
trace_id = turn-abc                    ← 一次用户请求
├── Compaction（threshold，turn_id = turn-abc）
│   └── Model Call（摘要采样，parent = compaction）
├── Model Call #1（parent = NULL）
│   ├── Tool Call: read（parent = Model Call #1）
│   └── Tool Call: bash（parent = Model Call #1）
├── Model Call #2（因 overflow 失败）
├── Compaction（overflow，triggerModelSpanId → Model Call #2）
└── Model Call #3

trace_id = trace-xyz                   ← 一次手动压缩，没有 Turn
└── Compaction（turn_id = NULL）
    └── Model Call（摘要采样）
```

一条 Trace 没有代表整次请求的根 Span。用户的输入与最终回答在 `turns` 行与它的 Message 里。

### 3.2 一次 Turn 的行

| 产生一行 | 不产生一行 |
|---|---|
| 每次顶层 Model Call，包括 overflow 后的重新提交 | Turn 本身（`turns` 已有一行） |
| 每次 Tool Call | Transport 重试（只增加 `attempt_count`） |
| 每次压缩 | 权限等待（记在 `permission_wait_ms`） |
| 压缩内每次摘要采样 | |

例：2 次 Model Call、3 次 Tool Call、1 次 threshold 压缩且摘要一次成功，共 7 行。摘要尝试 3 次则是 9 行。

`kind = 'model_call'` 的行包括摘要采样。只数 Turn 的 Model Call 时，加 `parent_span_id IS NULL`。

### 3.3 排序

Span 按 `started_at` 排序，`id` 作为同一时刻的次序。`trace_spans` 没有序号列，也没有 `(turn_id, sequence)` 唯一约束。

理由见 [Agent Note：发起关系与 Span 粒度](../../.agents/notes/implemented/architecture/2026-07-27-trace-causal-parent-and-span-granularity.md)。不设序号的理由见 [Agent Note：Trace 的标识与外键](../../.agents/notes/implemented/architecture/2026-07-27-trace-identifiers-and-foreign-keys.md)。

## 4. 表

表在 `crates/openwork-core/migrations/202607260001_initial_schema.sql`。时间列存东八区墙上时间，口径见 [data-model.md](../data-model.md)。

`trace_spans` 的约束：

| 约束 | 内容 |
|---|---|
| `trace_spans_kind_valid` | `kind` 只有 `model_call`、`tool_call`、`compaction` |
| `trace_spans_status_valid` | 状态见 §8 |
| `trace_spans_tool_columns_scoped` | `provider_call_id`、`requested_tool_name`、`resolved_tool_name`、`permission_wait_ms` 只出现在 `tool_call` 上 |
| `trace_spans_response_message_scoped` | `response_message_id` 只出现在 `model_call` 上 |
| `trace_spans_terminal_time_valid` | `running` 没有 `ended_at`；其他状态必有 `ended_at` |
| `trace_spans_end_after_start` | `ended_at >= started_at` |
| `trace_spans_tokens_non_negative` | 四个 token 列非负 |
| `trace_spans_attributes_is_object` | `attributes` 是 JSON 对象 |

`response_message_id` 引用 `messages(id)`，删除时置空。索引：`(trace_id, started_at)`、`(session_id, started_at DESC)`、`(turn_id, started_at)`。

| 表 | 内容 |
|---|---|
| `trace_payloads` | 正文。主键是内容哈希；`body`、`byte_size`、`created_at`。没有 `session_id` |
| `trace_span_payloads` | 正文挂到 Span。主键 `(span_id, slot)`；`span_id` 级联删除；`payload_hash` 为 `ON DELETE RESTRICT` |
| `trace_annotations` | 人工标注（§13） |

`trace_span_payloads` 的 `slot` 只有四个取值（§5）。`truncated = TRUE` 时，`original_byte_size` 必须非空且非负；否则必须为空。`redacted_count` 恒为 `0`，删除它的提议见 [Agent Note：删除 redacted_count](../../.agents/notes/proposed/simplification/2026-07-27-drop-trace-redacted-count.md)。

## 5. 正文

已经写进 `messages` 或 `conversation_compactions` 的内容，Trace 只留指针。

| 内容 | 位置 | Trace 的做法 |
|---|---|---|
| 用户输入 | `messages`（role=user） | 不记 |
| 成功调用的响应 | `messages`（role=assistant） | 记 `response_message_id` |
| 工具参数 | Assistant Message 的 tool_use 块 | 不记 |
| 工具结果 | `messages`（role=tool），由 `(turn_id, provider_call_id)` 定位 | 不记 |
| 成功的摘要 | `conversation_compactions.summary` | 记 `checkpointId` |
| 组装后的请求 | 只在 Trace | `request` 槽位：provider-neutral 的消息数组 |
| System Context | 只在 Trace | `system_context` 槽位：System Context 的各部分 |
| 工具定义 | 只在 Trace | `tool_definitions` 槽位 |
| 没有产生 Message 的响应 | 只在 Trace | `response` 槽位 |

槽位写入规则：

- Model Span 开始时写 `request`、`system_context`、`tool_definitions`。Recorder 始终处理全部槽位，没有记录档位。
- Model Call 结束时没有 `response_message_id`，就写 `response`。来源依次是完整响应、流里的完成事件、已收到的部分文本、推理与 Tool Call。三者都没有时不写。
- 摘要采样的成功响应也写 `response`，因为它不产生 Message。
- Tool Span 在两种情况写 `response`：结果为 `denied`，或结果 Message 没有持久化。内容是序列化的 `ToolResult`。
- Compaction Span 没有正文槽位。

正文不包含 API Key、解密后的凭证、HTTP Header 和 Provider 错误 Body。`TracePayloads` 只取 `ModelRequest.messages`、`ModelRequest.tools` 与 System Context 的各部分。

去重：哈希是截断后正文的 SHA-256。哈希前先按 `serde_json` 序列化，对象键按字典序排列。同一份工具定义在多次调用间只存一行。

截断：单个槽位上限默认 1 MiB，在 `OpenWorkCoreConfig.trace_content`（`TraceContentConfig`）配置，最小 2 字节。超限时，正文改为 `{"truncatedPreview": "<前缀>"}`，序列化后不超过上限。同时写 `truncated = TRUE` 与 `original_byte_size`。

理由见 [Agent Note：正文只记 Message 回答不了的内容](../../.agents/notes/implemented/architecture/2026-07-27-trace-payload-pointers.md)。

## 6. 写入

`PostgresTraceRecorder`（`crates/openwork-core/src/storage/trace.rs`）是生产中唯一的 Recorder。`NoopTraceRecorder` 只用于测试。

| 项 | 值 |
|---|---|
| 队列 | `mpsc`，容量 1,024。`record` 用 `try_send`，队列满时丢弃信号，计入 `dropped_signals` |
| 批次 | 一个写入任务，每批最多 64 个信号，一批一个事务 |
| 正文 | 批次含正文时，先取事务级 advisory lock。每个信号的正文写在一个 savepoint 里 |
| 正文失败 | 回滚这个 savepoint，Span 照常提交，计入 `write_failures` |
| 批次失败 | 整批丢弃，`write_failures` 加上整批信号数 |
| Flush | 发送与等待各有 2 秒超时。结果带 `flushed`、`dropped_signals`、`write_failures` |

- 开始信号用 `ON CONFLICT (id) DO NOTHING` 插入 `running` 行。结束信号插入或更新同一行，只缺结束信号时 Span 停在 `running`。
- Tool Span 的 `session_id` 取自 `turns`。Turn 行不存在时，Tool Span 不会写入。
- Turn 结束后调用 `flush_turn`。手动压缩与 rewind 结束后调用 `flush_session`。调用方不读结果。
- `dropped_signals` 与 `write_failures` 不在界面上显示，见 [Agent Note：显示采集损失](../../.agents/notes/proposed/feature/2026-07-27-trace-capture-loss-visibility.md)。

有损写入下的两条规则：

1. **`parent_span_id` 不建外键。** 父 Span 丢失时，子 Span 照常写入，读取时计入采集缺口（§8）。
2. **Trace 写入失败不改变业务结果。** 队列满、数据库不可用、Flush 超时，都不让 Turn 失败，也不回滚压缩。正文写入失败时，Span 本身仍落库。

理由见 [Agent Note：Trace 的标识与外键](../../.agents/notes/implemented/architecture/2026-07-27-trace-identifiers-and-foreign-keys.md)。

## 7. Span 语义

Guard（`ModelCallTraceGuard`、`ToolCallTraceGuard`、`CompactionTraceGuard`）在开始时发出开始信号，结束时发出结束信号。没有显式结束就被 drop 时，Guard 自己结束 Span：取消令牌已触发时记 `cancelled`，否则记 `failed` + `scope_dropped`。字符串列与属性按 §12 的上限截断。

### 7.1 Model Call

`name = model.call`。**开始**：请求已构建，即将调用 `ModelPort::invoke`。**结束**：流已完整消费，或调用返回错误或被取消。请求构建在 Span 开始之前，单独记为 `requestBuildMs`。

标准列：`model_id`、`resolved_model_name`、`status`、`attempt_count`、`provider_request_id`、四个 token 列、`response_message_id`、`started_at`、`ended_at`、`error_code`、`error_message`。

- `attempt_count` 等于 Transport Observer 实际看到开始的尝试数，上限是 `max_transport_attempts`。一次也没有开始时为空。
- `provider_request_id` 优先取响应或错误里的值，否则取最后一个带 ID 的尝试。
- 不记录逐次尝试的明细，不为尝试建子 Span。

| 属性 | 语义 |
|---|---|
| `modelCallIndex` | 当前 Turn 内第几次 Model Call；摘要采样里是第几次尝试 |
| `temperature`、`topP` | 取自本次 `ModelRequest` |
| `toolChoice` | 请求带工具时为 `auto`，否则不写 |
| `maxOutputTokens`、`thinkingMode` | `thinkingMode` 为 `enabled` 或 `disabled` |
| `requestBuildMs` | 构建请求的耗时 |
| `ttftMs` | 第一次 Transport 尝试开始到第一个语义事件 |
| `streamMs` | 第一个语义事件到流结束 |
| `finishReason` | `stop`、`tool_use`、`length`、`content_filter`、`refusal`、`cancelled`、`incomplete`、`unknown` |
| `responseId`、`actualModel`、`responseToolCallCount` | 取自响应 |
| `errorPhase` | `request_encode`、`connect`、`response_headers`、`response_body`、`stream_decode`、`response_decode`、`cancelled` |
| `deliveryState` | `not_sent`、`possibly_sent`、`accepted_no_semantic_output`、`semantic_output_emitted` |
| `httpStatus`、`providerCode` | 最后一次尝试的传输结果 |
| `requestMessageCount`、`toolDefinitionCount` | 请求规模，不加载正文就能显示 |
| `requestEstimated{SystemContext,Conversation,ToolSurface,Input}Tokens` | 发送前的估算，口径见 [context-window.md](context-window.md) |
| `requestTruncatedToolResults`、`requestOriginalToolResultTokens`、`requestProjectedToolResultTokens` | 投影截断了 Tool Result 时才写 |
| `summaryChars`、`summaryRetryDelayMs` | 只在摘要采样上写（§7.4） |

语义事件指 Text、Reasoning、Tool Call 的开始或增量，以及带内容的完成事件。连接、响应头与心跳不算。调用在语义事件之前失败时，`ttftMs` 与 `streamMs` 为空，不写 `0`。取消时 `errorPhase = cancelled`，`deliveryState` 为 `possibly_sent` 或 `semantic_output_emitted`。

### 7.2 Tool Call

`name = tool.call`。**开始**：完整的 Provider Tool Call 已组装，即将解析参数。**结束**：结果 Message 的持久化尝试完成，或调用在形成结果前失败、被拒或被取消。结束不等待下一次 Model Call。

标准列：`provider_call_id`、`requested_tool_name`、`resolved_tool_name`、`status`、`permission_wait_ms`。

- `requested_tool_name` 是模型给出的名称。`resolved_tool_name` 是解析到的工具：控制工具的名称，或注册表中的工具 id。工具未知时，`resolved_tool_name` 为空。
- `status` 是工具执行的结果。工具成功、结果 Message 写入失败时，`status` 仍是 `succeeded`，`resultPersisted = false`。
- 权限不是独立 Span。`permission_wait_ms` 记录等待用户决定的耗时，在 Tool Span 的列上。
- Trace 不记录未截断的工具输出。

| 属性 | 语义 |
|---|---|
| `permissionDecision`、`permissionDecisionSource`、`sandboxMode`、`sessionMode`、`sessionModeOrigin`、`escalationPaths`、`escalationJustification`、`dangerMatch`、`sandboxDenied` | 定义见 [permissions.md](permissions.md) §14.2 |
| `executionMs` | 工具执行耗时 |
| `artifactCount` | 结果中的 artifact 个数 |
| `artifactTypes` | artifact 类型，排序去重，最多 16 个 |
| `errorRetryable` | 失败结果的 `retryable` |
| `resultPersisted` | 结果 Message 是否写入 |
| `outputTruncated` | 类型中有这个字段，Core 不写入它 |

时间线上的权限类别见 [Agent Note：Trace 时间线的权限类别](../../.agents/notes/implemented/architecture/2026-09-24-trace-permission-categories.md)。

### 7.3 Compaction

`name = session.compact`。**开始**：Core 已接受一次压缩，即将读取当前 Conversation。**结束**：checkpoint 已持久化且新投影已安装，或任一步失败。压缩流程见 [compaction.md](compaction.md)。

```text
trace_id       = threshold/overflow 用所在 Turn 的 trace_id；manual/rewind 新生成
turn_id        = threshold/overflow 必填；manual/rewind 为空
parent_span_id = NULL
model_id       = rewind 为空
attempt_count  = 摘要尝试次数；rewind 与空 Conversation 为 0
input_tokens / output_tokens = 成功摘要响应的 usage；失败时为空
```

| 组 | 属性 |
|---|---|
| 触发证据 | `trigger`（`manual`、`threshold`、`overflow`、`rewind`）；threshold 与 overflow 记 `contextWindowTokens`、`triggerEstimatedInputTokens`、`triggerPercent`；overflow 另记 `triggerModelSpanId`、`triggerErrorCode` |
| 压缩效果 | `conversationTokensBefore`、`conversationTokensAfter`、`reclaimedConversationTokens` |
| 摘要请求 | `summaryRequestMessageCount`、`summaryEstimated{SystemContext,Conversation,ToolSurface}Tokens`、`summaryMaxOutputTokens` |
| 耗时与结果 | `prepareMs`、`summaryMs`、`persistenceMs`、`installMs`、`sourceMessageCount`、`summaryChars`、`checkpointId` |

- manual 与 rewind 不记任何触发证据。
- `triggerPercent` 是估算占窗口的百分比，四舍五入，不截到 100。overflow 前没有已提交的 Model Call 时，没有估算与百分比。
- `thresholdPercent` 在类型中存在，Core 不写入它。
- 前后 token 只度量 Conversation 区域，用发送前估算的口径。估算失败时不写这一对值。`reclaimed = max(before − after, 0)`。
- rewind 记 `prepareMs`、`persistenceMs`、`installMs`、`summaryChars`、前后 token 与 `checkpointId`，没有摘要子 Span。

### 7.4 摘要采样

每次摘要尝试是 Compaction Span 的一个子 Model Span。它有自己的 `provider_request_id`、token 列，以及 `request`、`system_context`、`tool_definitions`（空数组）与 `response` 槽位。父 Span 不存尝试明细，也不存聚合计数。各类尝试的次数用一条查询得到：

```sql
SELECT status, count(*) FROM trace_spans WHERE parent_span_id = $1 GROUP BY status
```

子 Span 的 `status` 是这次尝试的分类：

| 状态 | 含义 | 来源 |
|---|---|---|
| `succeeded` | 产出可用摘要 | — |
| `degenerate` | 有响应但不可用：过短、缺标题、截断、请求了工具 | `InvalidResponse` |
| `deterministic` | 同样的输入重发无用：鉴权、请求非法 | `RetryHint::Never`；重复的完成事件 |
| `input_overflow` | 输入超出窗口 | `ContextOverflow` |
| `transient` | 网络、过载、5xx；流结束时没有完成事件 | 其他模型错误 |
| `timeout` | 超出单次尝试的时限 | `SummaryAttemptTimeout`；模型 `Timeout` |
| `cancelled` | 压缩被取消 | 取消令牌；模型错误为 `Cancelled` |

- 一次压缩最多 3 次尝试，间隔 3 秒，每次时限 120 秒。
- 分类只用于诊断。无论分类如何，重试循环都跑到 3 次或成功为止。分类驱动重试的提议见 [Agent Note：压缩失败的处理](../../.agents/notes/proposed/architecture/2026-07-27-compaction-failure-handling.md)。
- `summaryRetryDelayMs` 记下一次尝试前的等待，最后一次尝试不记。`summaryChars` 只在成功的尝试上写。
- 摘要采样不增加 `turns.model_call_count` 与 `turns.model_submission_count`。

理由见 [Agent Note：发起关系与 Span 粒度](../../.agents/notes/implemented/architecture/2026-07-27-trace-causal-parent-and-span-granularity.md)。

## 8. 状态与完整度

| kind | 状态 |
|---|---|
| `model_call` | `running`、`succeeded`、`failed`、`cancelled`、`outcome_unknown` |
| `tool_call` | `running`、`succeeded`、`failed`、`denied`、`cancelled`、`outcome_unknown` |
| `compaction` | `running`、`succeeded`、`failed`、`cancelled`、`outcome_unknown` |
| 摘要采样的 `model_call` | 另有 `degenerate`、`deterministic`、`input_overflow`、`transient`、`timeout` |

- 数据库只允许有父 Span 的 `model_call` 使用这五个分类状态。
- 普通失败用 `failed`，权限拒绝用 `denied`，取消用 `cancelled`。会话 actor 停止时，Model Span 记 `outcome_unknown`。
- Core 不给 Model Span 写 `denied`。数据库约束不禁止这个组合。
- 启动时，`mark_running_interrupted` 把全部 `running` Span 改为 `outcome_unknown`，`error_code` 为 `process_restart`。理由见 [Agent Note：进程重启只修正状态](../../.agents/notes/implemented/architecture/2026-07-27-restart-correction-not-recovery.md)。

完整度在读取时派生（`derive_trace_completeness`，`storage/postgres/trace_query.rs`），不写回数据库：

```rust
pub struct TraceCompleteness {
    pub expected_model_calls: u32,   // turns.model_submission_count
    pub captured_model_calls: u32,   // kind = model_call 且 parent_span_id 为空
    pub expected_tool_calls: u32,    // turns.tool_call_count
    pub captured_tool_calls: u32,    // kind = tool_call
    pub orphan_tool_spans: u32,      // 父不是本 Trace 中某个 Model Span 的 Tool Span
    pub running_spans: u32,
    pub outcome_unknown_spans: u32,
    pub state: TraceCompletenessState, // Complete | Partial | None
}
```

- `None`：expected 大于 0，且没有采集到任何 Model 或 Tool Span。
- `Complete`：Turn 不在 `running`；两类 captured 分别等于 expected；orphan、running、outcome_unknown 都是 0。
- 其他情况为 `Partial`。运行中的 Turn 不会是 `Complete`。
- 无 Turn 的 Trace，expected 都是 0。
- 正文有无不参与完整度。界面上缺少正文时，只显示“无正文记录”，不推测原因。

理由见 [Agent Note：完整度对账 Turn 计数](../../.agents/notes/implemented/architecture/2026-07-27-trace-completeness-from-turn-counters.md)。

## 9. 查询

查询在 `PostgresStorage`（`storage/postgres/trace_query.rs`、`compaction.rs`）。`OpenWorkCore` 原样转发，Desktop 经 Tauri 命令调用。

| 方法 | Tauri 命令 | 返回 |
|---|---|---|
| `list_traces(session_id?, limit)` | `runtime_trace_list` | Trace 列表，`limit` 为 1–500 |
| `get_trace(turn_id)` | `runtime_trace_get` | 一个 Turn 的 Trace：摘要、全部 Span、完整度，不含正文 |
| `get_trace_by_id(trace_id)` | `runtime_trace_get_by_id` | 同上；也能打开无 Turn 的 Trace |
| `get_span_payload(span_id, slot)` | `runtime_trace_payload_get` | 一个槽位的正文与大小信息；没有时为空 |
| `list_compaction_spans(session_id, limit)` | `runtime_trace_compactions` | 一个 Session 的全部 Compaction Span |

- `list_traces` 合并两路来源（`UNION ALL`）。第一路是 `turns` 的每一行。第二路是 `turn_id IS NULL AND parent_span_id IS NULL` 的根 Span，每个根 Span 自成一条 Trace。
- 第二路的 `turn_id`、`turn_sequence` 为空，三个调用计数为 0。状态映射 `succeeded → completed`、`outcome_unknown → interrupted`，其他原样返回。
- 列表按开始时间倒序，再按 `trace_id` 排序。`total_tokens` 是各 Span `input_tokens + output_tokens` 之和，缺 token 的 Span 不计入。
- `get_trace` 与 `get_trace_by_id` 按 `started_at, id` 返回 Span。`span_count` 与 `total_tokens` 按实际加载的 Span 重算。
- `get_trace_by_id` 先找以该值为 id 的 Turn，或带 `turn_id` 的成员 Span。找到就按 Turn 返回，否则读无 Turn 的根 Span。
- `list_compaction_spans` 按 `started_at DESC, id` 排序，包括 threshold 与 overflow 的 Span。
- 正文只经 `get_span_payload` 按需读取。界面展开某个槽位时才请求它。

## 10. 保留、清扫与删除

保留：

- 正文按天保留，默认 30 天，由 `TraceContentConfig.retention_days` 配置，取值 1 到 `i32::MAX`。
- 启动时执行一次 `purge_expired_trace_payloads`。顺序是迁移、`mark_running_interrupted`、清理、启动 Recorder。清理出错时启动失败。
- 过期按 `trace_spans.started_at` 计算，不按 `trace_payloads.created_at`。
- 过期时只删除 `trace_span_payloads` 行。Span 与 token 列保留。
- 同一 Session 中，同一 `trace_id` 下有任何标注时，这条 Trace 的正文不过期。

删除 Session（`delete_session`）在一个事务里完成：

1. 取 advisory lock，锁住 Session 行、子 Agent Session 行与它们的 Span 行。
2. 取出这些 Span 引用的全部正文哈希。
3. 删除 Session。Span、正文挂载、标注、子 Agent Session 随之级联删除。
4. 对每个候选哈希，在 savepoint 里删除已经没有挂载的正文。

保留清理复用同一路径：删除过期挂载时用 `RETURNING payload_hash` 得到候选，再交给同一个清扫函数。清扫只检查候选哈希，不扫全表。

两条防线：

| 情形 | 防线 |
|---|---|
| 正文已插入、挂载还没写入 | 正文挂载与清扫共享同一把事务级 advisory lock |
| 挂载已存在 | `payload_hash` 的 `ON DELETE RESTRICT`；冲突的候选在 savepoint 里回滚，不影响其他候选 |

理由见 [Agent Note：正文的存储与清扫](../../.agents/notes/implemented/architecture/2026-07-27-trace-payload-storage-and-cleanup.md)。

## 11. Token 口径

Trace 记录 token，不记录金额。四个 token 列是 `input_tokens`、`output_tokens`、`cached_input_tokens`、`reasoning_tokens`。查询返回的 `total_tokens` 是 `input + output`，不另加缓存或推理 token。`reasoning_tokens` 是 `output_tokens` 的子集。

`cached_input_tokens` 与 `input_tokens` 的关系因 Provider 而不同：

| Provider kind | `input` 含 `cached` | 依据 |
|---|:---:|---|
| `anthropic` | 否 | `input_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens` 是三个独立的桶 |
| `openai` | 是 | `prompt_tokens_details.cached_tokens` |
| `deepseek` | 是 | `prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens` |
| `qwen` | 是 | OpenAI 兼容的 `cached_tokens` |
| `glm` | 是 | OpenAI 兼容的 `cached_tokens` |
| `kimi` | 未验证 | 用 OpenAI 兼容解析；集合关系没有官方资料确认 |

- 跨 Provider 时，`sum(input_tokens)` 不可比。计算缓存命中率或总输入量前，先按 `resolved_provider_kind` 分组。
- Anthropic 的 `cache_creation_input_tokens` 解析进 `TokenUsage`，不进 Trace 列。adapter 不发送 `cache_control`，所以这个值总是 0。
- Anthropic adapter 不解析推理 token，`reasoning_tokens` 为空。

理由见 [Agent Note：Trace 只记 token，不记成本](../../.agents/notes/implemented/simplification/2026-07-27-trace-records-tokens-not-cost.md)。

## 12. 属性

`attributes` 只接受三个版本化类型：`ModelTraceAttributesV1`、`ToolTraceAttributesV1`、`CompactionTraceAttributesV1`。三者都带 `schemaVersion = 1`，键用 camelCase。三者都是 `deny_unknown_fields`，反序列化时拒绝未知字段。

| 内容 | 位置 |
|---|---|
| 标量、枚举、耗时、计数 | `attributes` |
| 正文 | `trace_span_payloads` |
| 跨 Trace 需要聚合或过滤的量 | 列 |

上限：`error_code`、`provider_request_id`、`resolved_tool_name` 与 Guard 写入的自由文本属性最多 256 个字符；`error_message` 最多 512 个字符；`artifactTypes` 最多 16 项。`escalationPaths` 中的路径不截断。

Desktop 的属性白名单是 `TRACE_ATTRIBUTE_KEYS`（`desktop/src/features/traces/traceViewModel.ts`）。`TRACE_ATTRIBUTE_PLACEMENT` 把每个键归到一个 kind 的主字段，或归到“详细”折叠区。编译期检查它覆盖全部键，三个语言包由 `desktop/src/i18n/i18n.test.ts` 检查。白名单外的键不显示，`requestTruncatedToolResults`、`requestOriginalToolResultTokens`、`requestProjectedToolResultTokens` 不在白名单里。

| kind | 主字段属性 |
|---|---|
| `model_call` | `temperature`、`finishReason` |
| `tool_call` | `permissionDecision`、`permissionDecisionSource`、`sandboxMode`、`escalationPaths`、`escalationJustification`、`dangerMatch`、`sandboxDenied`、`executionMs` |
| `compaction` | `trigger`、`conversationTokensBefore`、`conversationTokensAfter`、`reclaimedConversationTokens` |

模型、耗时、token、尝试次数、权限等待来自 Span 的列，详情面板另行显示。

新增 kind 与新增属性的门槛见 [Agent Note：Trace 结构增长的门槛](../../.agents/notes/implemented/architecture/2026-07-27-trace-schema-growth-gates.md)。

## 13. 标注

`trace_annotations` 保存人对一次运行的评价。它是业务数据，不是 best-effort 数据。

| 列 | 内容 |
|---|---|
| `trace_id` | 被评价的 Trace，没有外键 |
| `span_id` | 为空表示评价整条 Trace；非空表示评价一个 Span，级联删除 |
| `rating` | `good`、`bad`、`unsure` |
| `note` | 可空，不能是空白 |

- 唯一索引 `uq_trace_annotations_target` 保证每个 `(trace_id, span_id)` 最多一条标注。
- 删除 Session 时级联删除它的标注。
- 带标注的 Trace 不参与正文过期（§10）。

Core 与 Desktop 都没有写入标注的路径。写入与界面的提议见 [Agent Note：标注的写入与界面](../../.agents/notes/proposed/feature/2026-07-27-trace-annotation-writes.md)。

## 14. 子 Agent

子 Agent 自己是一个 Session，设计见 [multi-agent.md](multi-agent.md)。Trace 没有为子 Agent 新增 kind 或列。

- 子 Turn 的 `trace_id` 等于子 `turn_id`，不继承父 Turn 的 `trace_id`。
- 子 Agent 的 Span 没有指向父 Trace 的 `parent_span_id`。
- `sessions.parent_session_id` 记录父 Session。`sessions.spawn_span_id` 记录发起它的 `spawn_agent` Tool Span，没有外键。
- 父侧的 `spawn_agent`、`wait_agent`、`list_agents`、`followup_task`、`interrupt_agent` 各产生一个普通的 `tool_call` Span。
- 删除父 Session 时，子 Agent Session 的 Span 与正文一起清扫（§10）。

Trace 页面不读这两列。跳转的提议见 [Agent Note：从 Trace 跳到子 Agent](../../.agents/notes/proposed/feature/2026-08-08-trace-sub-agent-navigation.md)。理由见 [Agent Note：子 Agent 的 Trace 在自己的 Session 里](../../.agents/notes/implemented/architecture/2026-08-08-sub-agent-trace-in-own-session.md)。

## 15. 常用推断

下表是人读 Trace 时的推断，不写回任何业务状态。

| 现象 | 推断 |
|---|---|
| 回答质量突然变化 | 先对比 `temperature`、`topP`、`resolved_model_name`，再看 `request` 正文 |
| 压缩后回答开始跑题 | 读摘要采样子 Span 的 `response`，摘要可能丢了约束 |
| `request` 正文里没有某条用户消息 | 它在 checkpoint 的摘要区间内，被摘要替换 |
| Model Call 很慢且 `attempt_count > 1` | 延迟主要来自 Transport 重试 |
| `ttftMs` 高、`streamMs` 正常 | Provider 排队、连接或首包慢 |
| 估算与真实 `input_tokens` 长期偏差大 | 每 4 字节 1 token 的估算不适合这个模型 |
| Tool Call 的 `permission_wait_ms` 占大部分时间 | 时间花在等用户决定 |
| Tool Call `failed`，下一次 Model Call `succeeded` | 模型看到错误后换了路径 |
| Turn `failed` 且完整度为 `Partial` | 只说明记录不完整，不能归因于缺失的 Span |
| Tool Call `outcome_unknown` | 副作用不可确认，不要自动重放 |

## 16. 验收

编号沿用原设计文档，代码与测试注释按编号引用。原文档没有第 11、12 条。测试路径相对 `crates/`，前端测试写出文件与用例名。带 Postgres 的测试需要 `TEST_DATABASE_URL`。

### 正文

1. Model Call 的 `request` 槽位完整还原当时提交的 provider-neutral 消息数组。
   - 测试：`openwork-core/tests/session_runtime.rs::no_tool_turn_completes_after_one_model_call`；`openwork-core/tests/postgres_trace_payloads.rs::payloads_are_deduplicated_loaded_on_demand_and_kept_out_of_trace_reads`
2. 压缩之后，`request` 正文是摘要加边界后的原始消息，不是全部 `messages`。
   - 测试：`openwork-core/tests/session_runtime.rs::context_budget_threshold_compacts_before_the_first_provider_submission`
   - 缺口：只断言 `request` 槽位等于压缩后实际提交的消息数组；没有直接断言其中有摘要、没有被替换的消息。
3. 成功的 Model Call 不写 `response` 槽位，用 `response_message_id` 指向 Assistant Message。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::model_and_tool_response_storage_follows_message_pointer_rules`；`openwork-core/tests/session_runtime.rs::no_tool_turn_completes_after_one_model_call`
4. 失败的 Model Call 在收到部分响应时写 `response` 槽位，且没有 `response_message_id`。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::model_and_tool_response_storage_follows_message_pointer_rules`；`openwork-core/src/session/trace.rs::model_guard_keeps_semantic_delivery_when_stream_decode_fails`
5. Tool Call 默认没有正文槽位。结果为 `denied`，或结果 Message 没有持久化时，写 `response`。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::model_and_tool_response_storage_follows_message_pointer_rules`；`openwork-core/tests/session_runtime.rs::tool_trace_records_result_persistence_failure_without_changing_tool_status`
6. 多次调用的相同 `tool_definitions` 在 `trace_payloads` 中只有一行。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::payloads_are_deduplicated_loaded_on_demand_and_kept_out_of_trace_reads`；`openwork-core/tests/postgres_trace_payloads.rs::deleting_a_session_sweeps_unique_payloads_but_restrict_preserves_shared_payloads`
7. 正文超过上限时截断，`truncated = TRUE` 且 `original_byte_size` 非空。数据库拒绝只置 `truncated`、不给原始大小的行。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::oversized_payloads_are_truncated_and_the_database_enforces_truncation_metadata`；`openwork-core/src/storage/trace.rs::truncation_produces_valid_json_with_bounded_serialized_bytes`
8. 正文写入失败时，Span 本身仍落库。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::payload_write_failure_keeps_the_span`
9. 正文中不出现 API Key、凭证、HTTP Header。
   - 测试：`openwork-core/src/session/trace.rs::content_payload_whitelist_excludes_transport_credentials_headers_and_errors`；`openwork-core/src/session/trace.rs::model_guard_keeps_semantic_delivery_when_stream_decode_fails`
   - 缺口：测试的输入本身不含这些值。保证来自结构：`TracePayloads` 只取消息、工具定义与 System Context。
10. `get_trace` 不返回正文。正文只经 `get_span_payload` 按需读取。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::payloads_are_deduplicated_loaded_on_demand_and_kept_out_of_trace_reads`；`openwork-core/tests/postgres_trace_payloads.rs::model_and_tool_response_storage_follows_message_pointer_rules`

### 标注

13. 每个 `(trace_id, span_id)` 最多一条标注，唯一索引拒绝第二行。
   - 状态：无测试。只由 `uq_trace_annotations_target` 保证。upsert 的写入路径不存在（§13）。
14. 标注既能挂到整条 Trace（`span_id IS NULL`），也能挂到单个 Span。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`
   - 缺口：测试只插入 Span 级标注；整条 Trace 的标注没有测试。
15. 正文过期清理不清理带标注的 Trace。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`
16. 删除 Session 时级联删除它的标注。
   - 状态：无测试。只由外键 `ON DELETE CASCADE` 保证。

### 保留与隐私

17. 正文过期后，Span 与 token 仍在，过期的 `trace_span_payloads` 行消失；由此产生的无人引用正文按第 18 条清扫。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`
18. **删除 Session 或过期正文挂载后，立即执行孤儿清扫，`trace_payloads` 中不残留无人引用的独有内容。**
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::deleting_a_session_sweeps_unique_payloads_but_restrict_preserves_shared_payloads`；`openwork-core/tests/postgres_trace_payloads.rs::retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`
19. 清扫无法删除仍有引用的正文（`RESTRICT` 生效）。
   - 测试：`openwork-core/tests/postgres_trace_payloads.rs::deleting_a_session_sweeps_unique_payloads_but_restrict_preserves_shared_payloads`；`openwork-core/tests/postgres_trace_payloads.rs::retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`

### 结构

20. 同一次用户请求产生的全部 Span 共享一个 `trace_id`。
   - 测试：`openwork-core/tests/session_runtime.rs::compacted_tool_turn_records_the_seven_documented_spans`；`openwork-core/tests/session_runtime.rs::context_overflow_compacts_and_resubmits_once_in_the_same_turn`
21. 手动压缩产生的 Span 有自己的 `trace_id`，且 `turn_id IS NULL`。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/tests/postgres_session_storage.rs::postgres_trace_recorder_persists_a_session_scoped_compaction`
22. rewind 同上，且 `model_id` 为空、没有子 Span。
   - 状态：无测试。`compaction/recovery.rs::rewind_conversation` 没有 Trace 断言。
23. 数据库拒绝缺少 `trace_id` 或 `trace_id` 为空白的写入。
   - 状态：无测试。只由 `NOT NULL` 与 `trace_spans_trace_not_blank` 保证。
24. `trace_spans` 没有 `sequence` 列，也没有 `(turn_id, sequence)` 唯一约束。
   - 状态：手动：读 `202607260001_initial_schema.sql` 的 `trace_spans` 定义，并确认后续迁移没有修改这张表。
25. **并发写入同一 Trace 的多个 Span 全部落库，不因排序键冲突丢失任何一条。**
   - 状态：无测试。表上只有主键唯一约束。
26. 排序只由 `started_at` 决定，`id` 是稳定的次序，同一时刻的顺序可重现。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
   - 缺口：只验证不同时刻的顺序；同一时刻按 `id` 排序没有测试。
27. 父 Span 未落库时，子 Span 照常写入。父 Model Span 缺失的 Tool Span 计为 orphan。
   - 测试：`openwork-core/src/storage/postgres/trace_query.rs::derives_complete_partial_and_none_without_persisting_another_status`；`desktop/src/features/traces/traceViewModel.test.ts › "builds a model-to-tool tree and keeps orphan tool calls visible"`
   - 缺口：没有测试在父 Span 缺失时向数据库写子 Span。
28. 数据库拒绝两种行：带 Tool 独占列的非 `tool_call` 行，以及非 `model_call` 上的 `response_message_id`。
   - 状态：无测试。只由 `trace_spans_tool_columns_scoped` 与 `trace_spans_response_message_scoped` 保证。

### 压缩

29. 四类触发各产生且只产生一个 Compaction Span。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/tests/session_runtime.rs::context_budget_threshold_compacts_before_the_first_provider_submission`；`openwork-core/tests/session_runtime.rs::context_overflow_compacts_and_resubmits_once_in_the_same_turn`
   - 缺口：rewind 没有测试。
30. threshold 与 overflow 用所在 Turn 的 `trace_id` 并带 `turn_id`；manual 与 rewind 新生成 `trace_id`，`turn_id` 为空。
   - 测试：`openwork-core/tests/session_runtime.rs::context_budget_threshold_compacts_before_the_first_provider_submission`；`openwork-core/tests/session_runtime.rs::context_overflow_compacts_and_resubmits_once_in_the_same_turn`；`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`
   - 缺口：rewind 没有测试。
31. 摘要采样是子 Span，每次有自己的 `provider_request_id`、token 列和正文。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/tests/postgres_session_storage.rs::postgres_trace_recorder_persists_a_session_scoped_compaction`
32. 失败的摘要采样的 `response` 正文可读。它不进任何业务表，Trace 是唯一落点。
   - 状态：无测试。不合格的响应经 `finish_response_failure` 写入 `response`；`compaction/summary.rs` 的测试只断言状态。
33. `SELECT sum(input_tokens) ... WHERE kind = 'model_call'` 包含压缩的开销。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_trace_recorder_persists_a_session_scoped_compaction`
   - 缺口：只断言摘要子 Span 行带有 token 列，没有执行求和查询。
34. 摘要采样不增加 `turns.model_call_count` 与 `turns.model_submission_count`。
   - 测试：`openwork-core/tests/session_runtime.rs::context_budget_threshold_compacts_before_the_first_provider_submission`；`openwork-core/tests/session_runtime.rs::compacted_tool_turn_records_the_seven_documented_spans`
35. 有摘要子 Span 的 Turn，完整度仍为 `Complete`。
   - 测试：`openwork-core/src/storage/postgres/trace_query.rs::derives_complete_partial_and_none_without_persisting_another_status`
36. overflow 的 `triggerModelSpanId` 指向那次失败的 Model Span，该 Span 的状态为 `failed`。
   - 状态：无测试。`context_overflow_compacts_and_resubmits_once_in_the_same_turn` 不检查这个属性。
37. manual 压缩不记录窗口与触发估算。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/src/session/trace.rs::compaction_attributes_serialize_without_the_unset_trigger_evidence`
38. 估算成功时，每次压缩记录前后 token，且 `reclaimed = before − after`，不小于 0。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/src/session/trace.rs::compaction_attributes_derive_what_the_replacement_reclaimed`；`openwork-core/src/session/trace.rs::compaction_attributes_do_not_report_a_negative_reclaim`
   - 缺口：threshold、overflow、rewind 没有断言前后 token。
39. 摘要尝试按 §7.4 分类，分类结果就是子 Span 的 `status`。父 Span 不存任何聚合。
   - 测试：`openwork-core/src/session/compaction/summary.rs::classifies_each_attempt_on_the_child_span_status`；`openwork-core/src/session/compaction/summary.rs::separates_provider_rejections_that_a_retry_cannot_clear`；`openwork-core/tests/postgres_session_storage.rs::postgres_trace_recorder_persists_a_session_scoped_compaction`
40. 摘要连续失败时，Compaction Span 为 `failed` 且带错误码，checkpoint 没有安装，Conversation 不变。
   - 测试：`openwork-core/src/session/compaction/summary.rs::stops_after_the_configured_summary_attempt_limit`；`openwork-core/tests/session_runtime.rs::failed_compaction_persistence_keeps_the_previous_conversation`
   - 缺口：第一个测试只到摘要层；第二个测试的失败来自持久化，不来自摘要。
41. `list_compaction_spans` 返回该 Session 的全部压缩，按 `started_at DESC` 排序，`limit` 生效。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::session_compaction_query_orders_newest_first_and_excludes_other_kinds`
42. **手动压缩在 Desktop 上可见，并能打开详情。** 从 `/compact` 到界面看到摘要正文有端到端用例。
   - 测试：`desktop/src/features/traces/manualCompactionPayloadFlow.test.tsx › "goes from /compact to a clickable turnless Trace and visible summary payload"`；`desktop/src/features/traces/components/TurnTraceDrawer.test.tsx › "opens a manual /compact trace by trace id and then loads its summary payload"`；`openwork-core/tests/postgres_session_storage.rs::trace_list_includes_turnless_compaction_traces`
   - 缺口：前端用例 mock 了 bridge 命令。

### 基础

43. 无工具的 Turn 产生一个 Model Span。
   - 测试：`openwork-core/tests/session_runtime.rs::no_tool_turn_completes_after_one_model_call`
   - 缺口：断言一次请求与一个 Model 结束信号，没有断言 Span 总数。
44. Model → Tool → Model 产生两个 Model Span 和一个挂在第一个 Model Span 下的 Tool Span。
   - 测试：`openwork-core/tests/session_runtime.rs::compacted_tool_turn_records_the_seven_documented_spans`；`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
   - 缺口：运行时测试没有断言 `parent_span_id`；存储测试的信号是手工构造的。
45. Tool Span 分别保存 `requested_tool_name` 与 `resolved_tool_name`；工具未知时，`resolved_tool_name` 为空。
   - 状态：无测试。
46. 权限等待只增加 Tool Span 的 `permission_wait_ms`。
   - 状态：无测试。等待时间在 `run_loop/authorization.rs::ask_user` 记录。
47. Provider 重试只增加 `attempt_count`，不产生 Attempt Span，也不写逐次明细数组。`httpStatus` 与 `providerCode` 反映最后一次尝试。
   - 测试：`openwork-core/src/session/trace.rs::model_guard_counts_retries_without_serializing_attempt_details`；`openwork-core/src/session/trace.rs::model_guard_keeps_only_the_final_transport_failure`
48. Core 不给 Model Span 写 `denied`。
   - 状态：手动：检查 `run_loop/mod.rs::trace_status_for_error` 与 `compaction/summary.rs`，确认它们不产生 `TraceStatus::Denied`。数据库不禁止这个组合。
49. terminal Span 必有 `ended_at`，且 `ended_at >= started_at`。
   - 状态：无测试。只由 `trace_spans_terminal_time_valid` 与 `trace_spans_end_after_start` 保证。
50. 启动时遗留的 `running` Span 变为 `outcome_unknown`。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
   - 缺口：测试只断言 Turn 变为 `interrupted`，没有断言 Span 的状态。
51. Trace API 区分 `Complete`、`Partial`、`None`。正文有无不参与这个判断。
   - 测试：`openwork-core/src/storage/postgres/trace_query.rs::derives_complete_partial_and_none_without_persisting_another_status`；`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
52. Model Span 的 Provider 耗时不含请求构建；`requestBuildMs`、`ttftMs`、`streamMs` 可分别验证。
   - 测试：`openwork-core/src/session/trace.rs::model_guard_counts_retries_without_serializing_attempt_details`；`openwork-core/tests/session_runtime.rs::runtime_records_versioned_model_and_tool_trace_attributes`
   - 缺口：只断言 `ttftMs` 存在；`requestBuildMs` 与 `streamMs` 的取值没有测试。
53. Tool Span 区分权限等待与执行耗时，并用 `resultPersisted` 区分结果是否进入 Conversation；持久化失败时仍有 terminal Span。
   - 测试：`openwork-core/tests/session_runtime.rs::tool_trace_records_result_persistence_failure_without_changing_tool_status`
   - 缺口：`executionMs` 与 `permission_wait_ms` 没有断言。
54. 记录 `temperature` 与 `topP`；参数修改后，新 Span 反映新值。
   - 测试：`openwork-core/src/session/trace.rs::model_guard_counts_retries_without_serializing_attempt_details`
   - 缺口：“修改后反映新值”没有测试。两个值每次取自当次 `ModelRequest`。

### 降级

**Trace 可以丢，业务不能受影响。**

55. Trace 队列满时，Turn 结果不变。
   - 状态：无测试。`record` 用 `try_send`，不返回错误。
56. 数据库 Trace 写入失败时，Message 仍提交。
   - 状态：无测试。写入在独立任务里进行，Turn 不读 Flush 结果。
57. 压缩的 Trace 写入失败时，checkpoint 仍安装，Conversation 仍替换。
   - 状态：无测试。
58. `attributes` 只接受白名单字段；超长字符串与数组被截断。
   - 测试：`openwork-core/src/session/trace.rs::model_guard_counts_retries_without_serializing_attempt_details`
   - 缺口：只断言未知字段被拒绝；长度上限没有测试。
59. tracing 或 OTLP 出口关闭、丢弃或导出失败时，PostgreSQL Trace 与 Turn 结果不变。
   - 状态：不适用：没有这个出口。条件移到 [Agent Note：tracing 与 OTLP 出口](../../.agents/notes/proposed/architecture/2026-07-27-trace-tracing-otlp-export.md)。

### 时间口径

60. 时间列存东八区墙上时间，见 [data-model.md](../data-model.md) 开头。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`；`openwork-core/src/storage/trace.rs::stores_span_timestamps_as_beijing_wall_clock`
61. **出库字符串带 `+08:00`，不带 `Z`。** 标错时区不会报错，只会让界面整体偏 8 小时。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::trace_list_includes_turnless_compaction_traces`；`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`；`openwork-core/src/storage/time.rs::serializes_stored_values_with_an_east_eight_offset`
62. 一个时刻经过“落库 → 序列化 → 前端解析”后仍等于原时刻。
   - 测试：`openwork-core/src/storage/time.rs::a_round_trip_through_storage_preserves_the_instant`
   - 缺口：测试用 Rust 解析；前端解析一侧没有测试。
