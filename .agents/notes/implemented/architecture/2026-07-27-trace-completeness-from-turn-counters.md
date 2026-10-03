# Agent Note: 完整度对账 Turn 计数

Status: implemented

## 问题

Trace 是有损的。看一条 Trace 时，需要知道它是不是完整的：时间线上缺一个节点，是因为没有发生，还是因为没有记录下来。

判断完整需要一个参照物。参照物如果来自 Trace 自己，对账就永远相等。

## 决策

- 完整度在读取时派生，不写回数据库（`crates/openwork-core/src/storage/postgres/trace_query.rs` 的 `derive_trace_completeness`）。
- expected 来自 `turns.model_submission_count` 与 `turns.tool_call_count`。业务写入路径维护这两列，与 Trace 写入路径互不依赖。
- captured 只数 `parent_span_id` 为空的 Model Span，以及全部 Tool Span。
- orphan、running、outcome_unknown 任一非零时，状态不是 `Complete`。
- 正文有无不参与判断。
- 规则见 [trace.md §8](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**从 Span 计数派生 expected。** 没有采用：那样 captured 与 expected 永远相等，对账失去意义。`turns` 表的迁移注释写明这两列是有意的冗余。

**captured 计入全部 Model Span。** 没有采用：摘要采样也是 `model_call`。计入后 captured 恒大于 expected，正常的 Turn 会被判成 `Partial`。

**统计缺失正文的 Span（`spans_missing_payload`）。** 早先的设计有这一项，统计没有 `request` 槽位的 Model Span。已删除：读取时无法可靠判断缺失原因。记录可能来自旧版本；正文写入可能单独失败；部分槽位按指针规则有意不写。

**把完整度存成一列。** 没有采用：它完全由 Span 与 Turn 计数决定，存下来只多一处可能不一致的地方。

## 后果

- 界面上的“这里没有正文”只陈述事实，不声称原因。
- `Partial` 只说明记录不完整。Turn 失败且完整度为 `Partial` 时，不能把失败归因于缺失的节点。
- 运行中的 Turn 可以显示实时计数，但不会被标为 `Complete`。
- 无 Turn 的 Trace 没有参照物，expected 为 0。
- 子 Agent 的 Span 如果混进父 Trace，会造成与摘要采样同样的误判，见 [子 Agent 的 Trace 在自己的 Session 里](2026-08-08-sub-agent-trace-in-own-session.md)。
