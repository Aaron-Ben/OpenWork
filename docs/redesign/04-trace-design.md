# OpenWork Trace 设计 V0.1

> 状态：领域 Trace 主路径及 2026-07-19 确认的 P0/P1 增强已实施。Trace 通过有界队列旁路写入 `trace_spans`，Model/Tool 使用类型化 Guard 记录真实 Transport Attempt、分段耗时和版本化 `attributes`，查询按 Turn 返回 `summary + spans + completeness`，Desktop 展示 P0、折叠 P1 和 Complete/Partial/None。Rust `tracing`/OpenTelemetry 出口、Trace 关闭入口与完整降级验收继续暂缓。
>
> 结论：Trace 是 `openwork-core` 的 best-effort 领域诊断能力；Turn 行是根，只保存 Model Call 和 Tool Call Span。PostgreSQL 领域 Trace 保留为本地产品契约，Rust `tracing` 只承担运行时 instrumentation，OpenTelemetry/OTLP 只作为未来可选出口，三者不能互相替代。

### 2026-07-18 暂缓项

- `OpenWorkCoreConfig` 当前没有关闭 Trace 的配置，生产启动固定创建 PostgreSQL Trace Recorder；`NoopTraceRecorder` 只用于内部运行时/测试装配。
- Recorder 已通过有界队列、`try_send` 和错误计数隔离写入失败，但 Queue 满、数据库不可用和 Flush 超时不改变 Turn 结果的验收矩阵尚未补齐。
- 本轮保持现状。若后续增加 Trace 开关，应通过 Recorder 注入切换 Noop/PostgreSQL 实现，不得在 Agent Loop 内增加 Trace 分支。

### 2026-07-19 已实施增强边界

- 保留 `TraceRecorder -> trace_spans -> Desktop` 的自定义领域 Trace，不改成依赖外部 Collector 的产品数据源；
- Model Call 补真实 Transport Attempt、请求构建/首包/流式阶段耗时、完成原因和标准化错误阶段；
- Tool Call 补验证、权限决策、实际执行、输出形状和 Tool Result 持久化结果；
- P1 只记录请求/响应/工具输入输出的大小和形状，不记录默认内容原文；
- `attributes` 使用版本化 Rust 类型和白名单序列化，不允许调用点任意拼接 JSON；
- Trace 查询返回 `summary + spans + completeness`，明确区分 Complete/Partial/None；
- 当前不新增 Span Kind、数据库表或普通列；只有字段进入高频筛选/聚合后，才通过新 migration 将其提升为专用列；
- 当前不引入 `tracing`/OpenTelemetry 依赖。以后接入时由同一生命周期 Guard 同时更新领域 Trace 与 `tracing::Span`，OTLP 默认关闭且失败不能影响 Turn。

## 1. Trace 解决什么

V0.1 Trace 只回答：

- 一次 Turn 调用了几次模型；
- 每次模型调用耗时、重试、Token 和错误是什么；
- 哪次模型调用产生了哪些工具；
- 工具等待权限和实际执行分别花了多久；
- Turn 慢或失败主要发生在哪一段；
- 采集数据是否缺失。

Trace 不回答：

- Turn 当前应该执行哪一步；
- 工具副作用是否可以安全重试；
- 进程重启后应从哪里恢复；
- Session 历史应显示什么；
- 模型下一次应该看到什么。

## 2. 参考实现与采用边界

关键源码：

- `xai-grok-shell/src/session/acp_session_impl/turn.rs`：`session.handle_prompt` 和 `session.process_conversation_turn` instrumentation；
- `xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`：`tools.execute` span；
- `xai-grok-shell/src/instrumentation.rs`：运行时 instrumentation；
- `xai-grok-telemetry`：Target Filter、日志和 Trace exporter；
- `xai-grok-shell/src/session/signals.rs`：统计 Signal 与 tracing 分开。

参考实现带来的结论不是“把所有节点都落一张业务表”：

- Codex：把安全的语义遥测与含完整输入输出的本地原始诊断 Bundle 分开；
- OpenCode：使用运行时 instrumentation 和 OTLP 出口，但自动遥测的输入输出默认值不适合作为 OpenWork 的隐私默认；
- grok-build：在真实运行边界埋点，使用字段闭集、内容 Gate 和导出前 fail-closed 脱敏；
- LangSmith：以 Trace/Run/Thread 组织数据，并把 TTFT、Token、Cost、Metadata 和 Feedback 作为可查询字段；OpenWork 只借鉴数据模型，不复制默认内容采集和云端依赖。

OpenWork 采用：

1. Runtime 在真实调用边界埋点；
2. Trace 与 Conversation、Update、Event 并存但不互相替代；
3. early return/错误路径也结束计时；
4. Trace exporter 的失败不改变 Agent Loop。

OpenWork 保留这些原则，但 V0.1 只落最有诊断价值的两类 Span。

### 2.1 三层可观测性结构

```text
ModelCallTraceGuard / ToolCallTraceGuard
    ├── TraceSignal
    │   └── PostgresTraceRecorder
    │       └── trace_spans -> Desktop Trace UI
    └── tracing::Span（未来可选）
        └── tracing-opentelemetry -> OTLP exporter（未来可选、默认关闭）
```

职责不能混淆：

| 层 | 职责 | 是否是本地 Trace 事实源 |
| --- | --- | --- |
| 领域 Trace | 稳定的 Turn/Model Call/Tool Call、状态、完整度和 Desktop 查询契约 | 是 |
| Rust `tracing` | 进程日志、内部函数/阶段 Span、开发和运维诊断 | 否 |
| OpenTelemetry/OTLP | 将 `tracing` Span/Log/Metric 发送到外部后端 | 否 |

不得仅依赖 `#[tracing::instrument]` 或 Subscriber 反推 PostgreSQL 领域行。`attempt_count`、`denied`、`outcome_unknown`、Tool Result 是否持久化和 Complete/Partial 都是 OpenWork 领域语义，必须由类型化 Guard/Signal 显式产生。

### 2.2 备选方案与决策后果

| 方案 | 决策 | 原因 |
| --- | --- | --- |
| 只保留 PostgreSQL 领域 Trace | 不采用 | 本地产品语义稳定，但无法复用标准运行时日志、指标和外部可观测后端生态 |
| 只使用 Rust `tracing` 宏 | 不采用 | 函数级 instrumentation 不能稳定表达 Attempt、权限决定、结果持久化和 Completeness 等领域事实 |
| 只使用 OpenTelemetry/OTLP | 不采用 | 会把本地诊断依赖 Collector/外部后端，也无法替代 Desktop 所需的稳定查询契约 |
| 领域 Trace 为主，`tracing`/OTLP 为可选旁路 | 采用 | 保持本地零配置和领域语义，同时为以后接入标准运维工具保留出口 |

该决策的代价是 Guard 需要同时维护“领域字段映射”和未来的运行时 Span 字段映射。两套映射必须来自同一次生命周期事件，不能在不同调用点重复计时或分别推导；否则会出现 PostgreSQL Trace 与外部遥测互相矛盾。收益是即使未安装 Collector、关闭 OTLP 或导出失败，Desktop Trace 和 Turn 结果仍保持可用。

## 3. 为什么简化当前 Trace

重构前的 OpenWork Trace 同时表达：

```text
turn
step
model_attempt
transport_attempt
tool_run
approval_wait
recovery
```

这些节点受旧的 `Turn → Step → ToolRun → Approval` 模型影响，也让 `ChatRuntime`、`Agent`、Transport Observer、Recorder Wrapper 和 Trace Service 都参与同一棵树的拼装。

目标删除：

- Step Span；
- Approval Span；
- Recovery Span；
- 独立 Transport Attempt Span；
- 单独的 Trace Root 行。

保留：

- Model Call Span；
- Tool Call Span；
- Turn 表中的开始、结束、结果和汇总。

## 4. 目标 Trace 结构

```text
Turn (turns 表，不重复建 Span)
├── Model Call #1
│   ├── Tool Call: read_file
│   └── Tool Call: bash
├── Model Call #2
└── Model Call #3
```

关系：

- Model Call Span 的 `parent_span_id = NULL`；
- Tool Call Span 的 Parent 是产生该调用的 Model Call Span；
- `turn_id` 是整棵 Trace 的稳定关联键；
- `sequence` 是 Turn 内 Trace 写入顺序；
- Turn 的 `model_call_count/tool_call_count` 是预期数量，Span 数量是实际采集数量。

Trace 表不对 `parent_span_id` 建自引用外键。若 Model Span 信号丢失但 Tool Span 成功写入，Tool Span 仍有独立诊断价值，应作为 Orphan 保存并计入采集缺口，而不是因 Parent 缺失再次丢弃。

## 5. Span 语义

### 5.1 Model Call

开始点：Core 已从 Chat State 构建完整请求、即将调用 `ModelPort::invoke`。

结束点：流被完整消费，或者明确返回错误/取消。

请求构建发生在 Model Span 开始之前，但必须单独测量为 `attributes.requestBuildMs`。这样 `started_at -> ended_at` 保持 Provider Model Call 生命周期，而本地 Conversation/Tool Definition 组装耗时仍可诊断。

标准字段：

```text
kind                    = model_call
name                    = model.call
resolved_model_name
model_id
status
attempt_count
provider_request_id
input_tokens
output_tokens
cached_input_tokens
reasoning_tokens
total_tokens
started_at
ended_at
error_code/error_message
```

`reasoning_tokens` 是 `output_tokens` 的子集；`total_tokens` 固定为 `input_tokens + output_tokens`，不再额外加上缓存或推理 Token。

`started_at` 与 `ended_at` 在数据库中保存为 UTC 的 `TIMESTAMP WITHOUT TIME ZONE`；API 补回 `Z` 后返回，桌面端统一按 `Asia/Shanghai` 展示。

`attempt_count` 是该 Model Call 内 Transport 总尝试数。V0.1 不为每个尝试建立子 Span；`provider_request_id` 保存 Provider 最终公开或最后一次可观察的请求 ID。

P0 必须增加的版本化属性：

| 属性 | 语义 |
| --- | --- |
| `schemaVersion` | Model 属性结构版本，当前固定为 `1` |
| `modelCallIndex` | 当前 Turn 内第几次 Model Call |
| `requestBuildMs` | 构建完整 Model Request 的耗时 |
| `ttftMs` | 发起首次 Transport Attempt 到第一个有效模型事件的耗时 |
| `streamMs` | 第一个有效模型事件到完整流结束的耗时 |
| `finishReason` | 标准化停止原因，如 `stop/tool_use/length/content_filter/refusal` |
| `responseId` | Provider 返回的响应 ID |
| `actualModel` | Provider 响应中声明的实际模型 |
| `errorPhase` | `request_encode/connect/response_headers/response_body/stream_decode/response_decode/cancelled` |
| `deliveryState` | `not_sent/possibly_sent/accepted_no_semantic_output/semantic_output_emitted` |
| `httpStatus` | 最终失败时的 HTTP 状态码 |
| `providerCode` | 白名单化、限长的 Provider 错误码 |
| `attempts` | 有界 Transport Attempt 摘要，最大元素数不得超过调用配置的最大尝试数 |

每个 `attempts[]` 元素允许：

```text
index
status = started | failed | succeeded
durationMs
errorCode
errorPhase
deliveryState
httpStatus
providerCode
providerRequestId
retryDelayMs
```

`attempts` 是诊断摘要，不建立 `transport_attempt` Span 或数据库表。顶层 `attempt_count` 必须等于实际开始过的 Attempt 数量；不得使用固定值或配置的最大值代替。

若调用在产生语义事件前失败，`ttftMs` 和 `streamMs` 保持为空，不用 `0` 冒充已观察耗时；此时用 `errorPhase`、`deliveryState` 和 `attempts[]` 解释失败位置。第一个有效模型事件指 Text、Reasoning 或 Tool Call 的首个语义事件，不包括内部连接、响应头和心跳信号。

P1 请求/响应形状属性见第 10 节。P1 不改变 Model Call 成败，只用于解释上下文膨胀、工具定义体积、输出规模和不同运行版本之间的差异。

### 5.2 Tool Call

开始点：完整 Provider Tool Call 已组装，Core 即将解析参数、验证 Schema 和决策权限。

结束点：Tool Result Message 的持久化尝试完成，或 Tool Call 在形成可持久化结果前明确失败、拒绝或取消。Tool Span 不等待下一次 Model Call。

标准字段：

```text
kind                    = tool_call
name                    = tool.call
provider_call_id
requested_tool_name
resolved_tool_name
status
permission_wait_ms
started_at
ended_at
error_code/error_message
```

`requested_tool_name` 是模型给出的名称，`resolved_tool_name` 是 Alias/路由后的真实实现。二者不能互相覆盖。

权限不是独立 Span。Tool Span 包围完整生命周期，`permission_wait_ms` 只记录等待用户决定的累计耗时。

P0 必须增加的版本化属性：

| 属性 | 语义 |
| --- | --- |
| `schemaVersion` | Tool 属性结构版本，当前固定为 `1` |
| `inputBytes` | Provider Tool Arguments 的 UTF-8 字节数，不保存原文 |
| `validationMs` | JSON 解析、工具查找和 Schema 验证耗时 |
| `permissionPolicy` | 初始策略结果 `allow/deny/ask` |
| `permissionDecision` | 最终决定 `allow/deny/cancelled` |
| `permissionDecisionSource` | `policy/config/user/system` 等闭集来源 |
| `executionMs` | 仅工具实现实际执行耗时，不含权限等待和结果持久化 |
| `outputBytes` | 返回给 Conversation 的 Tool Result 内容字节数 |
| `outputLines` | 文本输出行数 |
| `artifactCount` | Tool Result Artifact 数量 |
| `errorRetryable` | Tool Error 是否声明为可重试 |
| `resultPersisted` | Tool Result Message 是否提交成功 |
| `resultPersistMs` | Tool Result Message 持久化尝试耗时 |
| `resultPersistErrorCode` | 持久化失败时的标准化、限长错误码；成功时为空 |

`status` 仍表示 Tool Result 的运行结果，持久化失败不得伪装成工具执行失败。例如工具执行成功但 Message 写入失败时，Tool Span 可以保持 `succeeded`，同时记录 `resultPersisted=false` 和 `resultPersistErrorCode`；Turn 的业务失败由 Storage/Runtime 自己记录，Trace 不参与推进。

## 6. 状态

Model Call 允许：

```text
running
succeeded
failed
cancelled
outcome_unknown
```

Tool Call 允许：

```text
running
succeeded
failed
denied
cancelled
outcome_unknown
```

规则：

- `running` 必须没有 `ended_at`；
- terminal 状态必须有 `ended_at`；
- Provider/工具返回的普通失败使用 `failed`；
- 用户权限拒绝使用 `denied`；
- 用户主动取消使用 `cancelled`；
- 进程退出导致结果不可确认时使用 `outcome_unknown`；
- Model Call 不能使用 `denied`。

## 7. Trace Recorder

Core 内部接口：

```rust
trait TraceRecorder: Send + Sync {
    fn record(&self, signal: TraceSignal);
    async fn flush_turn(&self, turn_id: &TurnId) -> TraceFlushResult;
}

enum TraceSignal {
    ModelCallStarted(ModelCallStarted),
    ModelCallFinished(ModelCallFinished),
    ToolCallStarted(ToolCallStarted),
    ToolCallFinished(ToolCallFinished),
}
```

`record` 必须非阻塞：

```text
SessionActor
    -> try_send(TraceSignal)
        -> bounded channel
            -> background writer
                -> batch upsert trace_spans
```

队列满、序列化错误或数据库错误时：

- 丢弃该信号或保留有限重试；
- 增加进程内 `dropped_signals`/`write_failures` 指标；
- 记录不含敏感 Payload 的 warning；
- 不把错误返回给 Agent Loop；
- 不改变 Turn/Message 事务。

`flush_turn` 只用于提高 Trace 页面即时可见性。超时或失败不能把已完成 Turn 改成失败。

## 8. Span 生命周期实现

使用类型化 RAII Guard 保证错误路径结束，并集中维护字段白名单：

```rust
let mut span = trace.start_model_call(..., request_build_ms);
span.record_request_shape(&request);
let options = options.with_transport_observer(span.transport_observer());
let result = model.invoke(request, options).await;

match result {
    Ok(response) => span.finish_success(response),
    Err(error) => span.finish_error(error.code(), redact(error.message())),
}
```

Guard 在 Drop 时若尚未显式结束：

- Cancellation Token 已触发：`cancelled`；
- 正常进程内 unwind/early return：`failed`，错误码 `scope_dropped`；
- 进程直接退出时来不及写终态，下一次启动统一修正为 `outcome_unknown`。

Tool Call 也使用相同模式，Permission Wait 用局部计时器累计到 Guard。

未来引入 Rust `tracing` 时，不在 `run_loop` 再复制一套计时和字段拼装。Guard 内部持有可选 `tracing::Span`，同一次 `record_*`/`finish_*` 同时更新领域 Signal 和运行时 Span；OpenTelemetry 只消费后者。

## 9. Runtime 埋点位置

```text
session/run_loop.rs
    measure build_request
    start Model Call
        model.invoke(request, options.with_transport_observer(...))
        record first semantic event
        consume stream
    finish Model Call

    for each complete Tool Call
        start Tool Call
            measure parse/validate
            record permission policy
            optional measure Desktop permission wait + decision source
            measure tools::invoke
            form Tool Result
            attempt to persist Tool Result Message
        finish Tool Call
```

Tool Span 在 Tool Result 持久化尝试后结束，而不是在 Tool Result 刚形成时结束，也不等待下一次模型调用。这样 `resultPersisted/resultPersistMs` 是已观察事实；Trace 仍不能代替缺失的 Message。

## 10. attributes 的边界

`attributes` 不接受调用点随意构造的 `Map<String, Value>`。Core 必须定义并测试版本化的 `ModelTraceAttributesV1` 与 `ToolTraceAttributesV1`，由 Storage 统一序列化。未知字段默认拒绝，而不是静默透传。

Model Call P1 允许的请求/响应形状字段：

```json
{
  "schemaVersion": 1,
  "appVersion": "0.1.0",
  "modelCallIndex": 1,
  "requestMessageCount": 12,
  "requestSystemMessageCount": 1,
  "requestUserMessageCount": 2,
  "requestAssistantMessageCount": 5,
  "requestToolMessageCount": 4,
  "requestContentBytes": 18340,
  "toolDefinitionCount": 6,
  "toolDefinitionBytes": 9210,
  "maxOutputTokens": 4096,
  "thinkingMode": "enabled",
  "responseTextBytes": 240,
  "responseReasoningBytes": 812,
  "responseToolCallCount": 2,
  "responseToolArgumentsBytes": 386,
  "apiProtocol": "openai_responses"
}
```

Tool Call P1 允许的输入/输出形状字段：

```json
{
  "schemaVersion": 1,
  "inputTopLevelKeyCount": 3,
  "outputBytes": 2048,
  "outputLines": 42,
  "outputTruncated": true,
  "artifactCount": 1,
  "artifactTypes": ["file"],
  "progressEventCount": 4
}
```

P1 是容量、性能与可复现性诊断，不是 P0 成败判定。当前首批已实现 message/content/tool-definition 数量与字节数、response text/reasoning/tool-call 大小、Tool Input 顶层 Key 数、Artifact/Progress 形状、`schemaVersion` 和 `appVersion`。`outputTruncated/apiProtocol` 等字段只有上游存在可靠事实时才补充。

默认禁止：

- System Prompt/User Prompt；
- 完整 Model Request/Response；
- Tool Input/Output 原文；
- 文件内容、Shell 输出；
- HTTP Header；
- API Key、Credential Ref 的解析结果；
- Provider 返回的未脱敏错误 Body。

限制：

- `error_message` 截断到固定长度；
- `attributes` 只允许白名单 Key；
- String/Array 有元素和字节上限；
- `attempts` 元素数不超过 `max_transport_attempts`，单个错误码和 Request ID 必须限长；
- URL 只保留 scheme/host 或稳定 endpoint label；
- 路径按 Session `working_directory` 相对路径或哈希保存。

## 11. 采集完整度

Trace 详情返回：

```rust
struct TraceCompleteness {
    expected_model_calls: u32,
    captured_model_calls: u32,
    expected_tool_calls: u32,
    captured_tool_calls: u32,
    orphan_tool_spans: u32,
    running_spans: u32,
    outcome_unknown_spans: u32,
    state: Complete | Partial | None,
}
```

计算来源：

- expected：`turns.model_call_count/tool_call_count`；
- captured：`trace_spans` 按 kind 计数；
- orphan：`parent_span_id` 指向同一 Turn 中不存在的 Model Span；
- `running/outcome_unknown`：Span 状态。

这能显式说明 Trace 缺口，而不是把缺失节点误判为“没有调用”。

对 terminal Turn，状态计算规则固定为：

- `Complete`：captured 与 expected 分别相等，且 orphan/running/outcome_unknown 均为 0；
- `None`：expected 至少有一个调用，但 Model/Tool Span 均未采集；
- `Partial`：其他所有情况。

运行中的 Turn 可以展示实时计数，但不能提前标记 `Complete`。Completeness 是读取时派生值，不写回 `turns` 或 `trace_spans`，避免异步 Writer 后续补齐后状态过期。

## 12. 查询 API

Core 对 Desktop 暴露：

```rust
list_turn_traces(filter, cursor, limit)
get_trace(turn_id)
```

`get_trace` 的返回结构：

```rust
struct TurnTrace {
    summary: TraceTurnSummary,
    spans: Vec<TraceSpan>,
    completeness: TraceCompleteness,
}
```

Desktop 只消费 Turn Summary 与 `model_call/tool_call` 两类 Span，不复制数据库 Record，也不通过 Trace 更新 Chat Runtime。前端查询、页面和隐私边界见 [06-frontend-architecture.md](06-frontend-architecture.md#19-trace-ui)。

列表行来自 `turns`，左连接 Trace 汇总：

```text
turn_id
session_id
status
resolved_model_name
started_at/ended_at/duration
model_call_count/tool_call_count
failed_span_count
trace_completeness
```

详情：

```text
Turn Summary
Model Call #1
  Tool Call #1
  Tool Call #2
Model Call #2
Capture Completeness
```

Trace 页面读取失败不能影响 Session 页面和普通聊天历史。

## 13. 常见诊断规则

这些是 UI/Query 层推断，不写回业务状态：

| 现象 | 推断 |
| --- | --- |
| Model Call 很慢且 `attempt_count > 1` | Transport Retry 贡献主要延迟 |
| Model Call `ttftMs` 高、`streamMs` 正常 | Provider 排队、连接或首包路径贡献主要延迟 |
| Model Call `requestContentBytes/toolDefinitionBytes` 持续增长 | Conversation 或 Tool Schema 正在膨胀 |
| Tool Call `permission_wait_ms` 占大部分 | 用户权限等待 |
| Tool Call `executionMs` 低但总耗时高 | 验证、权限或结果持久化贡献主要延迟 |
| Tool Call `outputTruncated=true` | 后续模型看到的是截断结果，不能按完整输出推断 |
| Tool Call failed，下一次 Model Call succeeded | 模型观察错误后完成了替代路径 |
| Turn failed 且 Trace partial | 只能说明诊断数据不完整，不能归因于缺失节点 |
| Tool Call outcome_unknown | 副作用结果不可确认，不得自动重放 |

所有诊断结果必须标注 `inference`，不能伪装成已记录事实。

## 14. 当前实现状态与后续边界

| 当前 Span | 目标 |
| --- | --- |
| 旧 `turn` Root Span | 删除，读取 `turns` 业务表作为 Root |
| `step` | 删除 |
| `model_attempt` | `model_call` |
| `transport_attempt` | 聚合到 `attempt_count/provider_request_id` |
| `tool_run` | `tool_call` |
| `approval_wait` | 聚合到 `permission_wait_ms` |
| `recovery` | 删除；V0.1 不恢复 |

先让新 Runtime 直接产生目标 Signal，不在旧 `TracingTurnRecorder` 上继续叠加转换层。

截至 2026-07-19，本增强已完成：

- `run_loop` 已挂载现有 `ModelTransportObserver`，`attempt_count/attempts[]` 来自实际开始过的 Transport Attempt；
- Model Span 在 `build_request` 后开始，并记录 `requestBuildMs/ttftMs/streamMs`、完成原因、响应标识和错误阶段；
- Tool Span 已分别记录 validation/permission/execution/persistence，并在 Tool Result 数据库写入尝试后结束；
- Core 只通过 `ModelTraceAttributesV1/ToolTraceAttributesV1` 产生版本化 `attributes`；
- `get_trace` 已返回完整 `TurnTrace`，Completeness 在读取时派生，Desktop 显示 P0、Attempt 明细和折叠 P1；
- PostgreSQL 真实集成测试验证现有 `attributes JSONB` 和 `TurnTrace` 查询，无新 migration。

仍未实施或保持暂缓：

- `flush_turn` 的 dropped/write failure 计数尚未暴露到 Desktop 或标准运维出口；
- `outputTruncated/apiProtocol` 等只有上游存在可靠事实时才补充，当前首批 P1 实现以数量、字节、版本和 Artifact/Progress 形状为主；
- 当前 Cargo manifests 尚未直接声明 Rust `tracing` 或 OpenTelemetry 依赖；
- Trace 关闭入口，以及 Queue 满、数据库不可用和 Flush 超时的完整降级验收矩阵仍按第 2026-07-18 暂缓项处理。

P0/P1 实施优先使用现有列与 `attributes`，不修改已应用 migration，不新建 `model_attempts`、`transport_attempts`、`trace_events` 或 Payload 表。若未来确有跨 Trace 的高频筛选/聚合需求，必须以查询和性能证据为前提，通过新 migration 将稳定标量提升为专用列。

## 15. 验收测试

1. 一个无工具 Turn 产生一个 Model Span；
2. Model → Tool → Model 产生两个 Model Span 和一个正确挂载的 Tool Span；
3. Tool Name Alias 同时保留 requested/resolved；
4. Permission Wait 只增加 Tool Span 的等待耗时；
5. Provider Retry 只增加 Model Span 的 `attempt_count`；
6. Model Span 不可写入 `denied`；
7. terminal Span 必有 `ended_at`；
8. Trace Queue 满时 Turn 结果不变；
9. 数据库 Trace 写失败时 Message 仍提交；
10. 启动时遗留 Running Span 变为 `outcome_unknown`；
11. Trace API 能识别 Complete/Partial/None；
12. 默认 Trace 中不出现完整 Turn、Prompt、Tool Input/Output、Provider Body 和凭证内容；
13. 真实 Transport Retry 与 `attempt_count/attempts[]` 一致，且不产生独立 Attempt Span；
14. Model Span 的 Provider 耗时不包含 `build_request`，`requestBuildMs/ttftMs/streamMs` 可分别验证；
15. Tool Span 能区分 validation/permission/execution/persistence，持久化失败时仍产生 terminal Span；
16. `attributes` 只接受版本化白名单字段，超长字符串/数组被截断或拒绝；
17. P1 只记录大小、数量、枚举和版本，不记录内容原文；
18. 若启用未来 `tracing`/OTLP 出口，其关闭、丢弃或导出失败不改变 PostgreSQL Trace 和 Turn 结果。
