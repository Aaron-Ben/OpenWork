# Agent Note: 发起关系与 Span 粒度

Status: implemented

## 问题

一条 Trace 里有 Model Call、Tool Call、压缩与摘要采样。需要定两件事：Span 之间的父子关系表示什么；什么事情值得单独一行。

两个问题都影响统计。父子关系决定能不能找到“哪次模型响应请求了这次 Tool Call”。粒度决定 `sum(input_tokens)` 能不能算出真实开销。

## 决策

- `parent_span_id` 表示“谁发起了谁”。Tool Span 的父是请求它的 Model Span；摘要采样的父是 Compaction Span；Compaction 没有父；overflow 的失败 Model Span 记在 `triggerModelSpanId`，不当父。
- 一条 Trace 没有根 Span。请求与最终回答在 `turns` 行与 Message 里。
- 重发同一个请求是属性，发起一个新请求是新行。Transport 重试只增加 `attempt_count`；每次摘要尝试是一个子 Model Span。
- 父 Compaction Span 不存尝试明细，也不存聚合计数。
- 规则见 [trace.md §3、§7.4](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**按 OTel 惯例，父 Span 在时间上包含子 Span。** 没有采用。`started_at` 已经表达时间关系，但“哪次响应请求了这次 Tool Call”没有别的地方可记。并发 Tool Call 的时间区间互相重叠，无法从区间反推。Tool Span 在 Model Span 结束后才开始：run loop 写入 Assistant Message 后结束 Model Span，之后才执行 Tool Call（`crates/openwork-core/src/session/run_loop/mod.rs`）。

**把 Compaction 挂到触发它的 Model Call 下。** 没有采用：压缩是 Core 的策略决定，模型和工具都没有请求它。挂上去会给出错误的发起关系。

**加一个代表整次请求的根 Span。** 没有采用：用户输入与最终回答已经是业务数据，Trace 不复制它们。

**把摘要尝试压成父 Span 上的 JSON 数组。** 没有采用。`SELECT sum(input_tokens) FROM trace_spans WHERE kind = 'model_call'` 会漏掉压缩的开销，失败尝试的 usage 也没有地方落库。每次尝试各有自己的 `provider_request_id` 与 usage，所以各占一行。

**在父 Span 上存尝试计数 `attemptRollup`。** 曾经实现过。删除的原因：对子 Span 的 `status` 做一次 `GROUP BY` 就得到它。两者是同一个事实的两种写法，存下来只多一处可能不一致的地方。`summaryAttemptOutcome` 同理，它与子 Span 的 `status` 是同一个枚举值。

**为每个 Transport 尝试建子 Span。** 没有采用，理由见 [Trace 结构增长的门槛](2026-07-27-trace-schema-growth-gates.md) 中的 `attempts[]`。

## 后果

- `COUNT(*) WHERE kind = 'model_call'` 包括摘要采样。完整度必须只数顶层 Model Span，见 [完整度对账 Turn 计数](2026-07-27-trace-completeness-from-turn-counters.md)。
- 摘要质量差时，可以读出每次尝试的请求与响应。失败的尝试不进任何业务表，Trace 是唯一落点。
- 一次 threshold 压缩加 2 次 Model Call、3 次 Tool Call 是 7 行，测试 `compacted_tool_turn_records_the_seven_documented_spans` 固定了这个形状。
