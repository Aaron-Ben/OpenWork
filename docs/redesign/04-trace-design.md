# OpenWork V1 Trace 设计

> 状态：目标设计，尚未实施。
>
> 结论：Trace 是 `openwork-core` 的 best-effort 内部能力；Turn 行是根，只保存 Model Call 和 Tool Call Span。

## 1. Trace 解决什么

V1 Trace 只回答：

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

## 2. grok-build 的参考边界

关键源码：

- `xai-grok-shell/src/session/acp_session_impl/turn.rs`：`session.handle_prompt` 和 `session.process_conversation_turn` instrumentation；
- `xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`：`tools.execute` span；
- `xai-grok-shell/src/instrumentation.rs`：运行时 instrumentation；
- `xai-grok-telemetry`：Target Filter、日志和 Trace exporter；
- `xai-grok-shell/src/session/signals.rs`：统计 Signal 与 tracing 分开。

Grok 的关键启发不是“把所有节点都落一张业务表”，而是：

1. Runtime 在真实调用边界埋点；
2. Trace 与 Conversation、Update、Event 并存但不互相替代；
3. early return/错误路径也结束计时；
4. Trace exporter 的失败不改变 Agent Loop。

OpenWork 保留这些原则，但 V1 只落最有诊断价值的两类 Span。

## 3. 为什么简化当前 Trace

当前 OpenWork Trace 同时表达：

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

开始点：Core 已从 Chat State 构建完整请求、即将调用 `Model::stream`。

结束点：流被完整消费，或者明确返回错误/取消。

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
started_at
ended_at
error_code/error_message
```

`attempt_count` 是该 Model Call 内 Transport 总尝试数。V1 不为每个尝试建立子 Span；`provider_request_id` 保存 Provider 最终公开或最后一次可观察的请求 ID。

### 5.2 Tool Call

开始点：完整 Provider Tool Call 已解析，Core 即将验证和决策权限。

结束点：Tool Result 已形成，或 Tool Call 明确被拒绝/取消。

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

使用 RAII Guard 保证错误路径结束：

```rust
let span = trace.start_model_call(...);
let result = model.stream(request).await;

match result {
    Ok(response) => span.finish_success(response.usage),
    Err(error) => span.finish_error(error.code(), redact(error.message())),
}
```

Guard 在 Drop 时若尚未显式结束：

- Cancellation Token 已触发：`cancelled`；
- 正常进程内 unwind/early return：`failed`，错误码 `scope_dropped`；
- 进程直接退出时来不及写终态，下一次启动统一修正为 `outcome_unknown`。

Tool Call 也使用相同模式，Permission Wait 用局部计时器累计到 Guard。

## 9. Runtime 埋点位置

```text
session/run_loop.rs
    start Model Call
        models::stream
    finish Model Call

    for each complete Tool Call
        start Tool Call
            tools::validate
            tools::permission_policy
            optional wait for Desktop decision
            tools::invoke
        persist Tool Result Message
        finish Tool Call
```

Tool Span 在 Tool Result 形成时结束，而不是等待下一次模型调用。Message 持久化失败可以记入 `attributes.result_persisted=false`，但 Trace 仍不能代替缺失的 Message。

## 10. attributes 的边界

允许的低风险字段：

```json
{
  "finishReason": "tool_calls",
  "retryReasons": ["timeout"],
  "toolInputBytes": 128,
  "toolOutputBytes": 2048,
  "resultPersisted": true
}
```

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

## 12. 查询 API

Core 对 Desktop 暴露：

```rust
list_turn_traces(filter, cursor, limit)
get_turn_trace(turn_id)
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
| Tool Call `permission_wait_ms` 占大部分 | 用户权限等待 |
| Tool Call failed，下一次 Model Call succeeded | 模型观察错误后完成了替代路径 |
| Turn failed 且 Trace partial | 只能说明诊断数据不完整，不能归因于缺失节点 |
| Tool Call outcome_unknown | 副作用结果不可确认，不得自动重放 |

所有诊断结果必须标注 `inference`，不能伪装成已记录事实。

## 14. 当前实现迁移

| 当前 Span | 目标 |
| --- | --- |
| 旧 `turn` Root Span | 删除，读取 `turns` 业务表作为 Root |
| `step` | 删除 |
| `model_attempt` | `model_call` |
| `transport_attempt` | 聚合到 `attempt_count/provider_request_id` |
| `tool_run` | `tool_call` |
| `approval_wait` | 聚合到 `permission_wait_ms` |
| `recovery` | 删除；V1 不恢复 |

先让新 Runtime 直接产生目标 Signal，不在旧 `TracingTurnRecorder` 上继续叠加转换层。

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
12. 默认 Trace 中不出现 Turn、工具输出和凭证内容。
