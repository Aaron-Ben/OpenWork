# Agent Note: 子 Agent 的 Trace 在自己的 Session 里

Status: implemented

## 问题

父 Turn 通过 `spawn_agent` 派生子 Agent。子 Agent 有自己的上下文窗口、对话与压缩，并运行自己的 Turn。

它的 Span 要放在哪条 Trace 里，又怎样与父侧关联？Trace 的完整度用父 Turn 的计数对账，任何混进父 Trace 的 Span 都会影响这个对账。

## 决策

- 子 Agent 自己是一个 Session。`sessions` 有 `parent_session_id` 与 `spawn_span_id`（迁移 `202608080001_add_subagent_sessions.sql`）。
- 子 Turn 的 `trace_id` 等于子 `turn_id`，不继承父 Turn 的 `trace_id`。
- 子 Agent 的 Span 没有跨 Trace 的 `parent_span_id`。
- `spawn_span_id` 记录发起它的 Tool Span（`crates/openwork-core/src/session/run_loop/mod.rs` 把 `tool_trace.span_id()` 传给 `spawn`），不建外键。
- Trace 没有新增 kind 或列。父侧的子 Agent 工具各产生一个普通 `tool_call` Span。
- 规则见 [trace.md §14](../../../../docs/subsystems/trace.md)，子 Agent 的设计见 [multi-agent.md](../../../../docs/subsystems/multi-agent.md)。

## 考虑过的方案

**子 Agent 的 Span 共享父 `trace_id`。** 没有采用。完整度的 expected 来自父 Turn 的 `model_submission_count`，captured 是顶层 `model_call` 数。子 Agent 的 Span 混进来后，captured 恒大于 expected，每个用了子 Agent 的正常 Turn 都会被判成 `Partial`。摘要采样当年就是这个问题。

**在父 Session 下给子 Agent 单独插入 `turns` 行。** 没有采用：它占用父 Session 的轮次编号，并与“一个 Session 只有一个活跃 Turn”冲突。

**子 Agent 的根 Span 用 `parent_span_id` 指向父 Trace 的 Tool Span。** 没有采用：orphan 的定义是父 Span 不在同一 Trace 中。跨 Trace 的父会让完整度计算永远把它判成 orphan。

**`spawn_span_id` 建外键。** 没有采用：Trace 是 best-effort，队列满时那个 Span 可能没有落库。这与“结构标识不建外键”一致，见 [Trace 的标识与外键](2026-07-27-trace-identifiers-and-foreign-keys.md)。

## 后果

- 子 Agent 有合法的 `turn_id`，不影响父 Turn 的调用计数。父 Turn 与子 Turn 的完整度各自对账，互不干扰。
- 子 Agent 复用同一套压缩与 Trace 写入路径。它的压缩 Span 在自己的 Trace 里，不出现在父 Trace 中。
- 跨 Session 汇总要沿 `sessions.parent_session_id` 做递归查询。
- 删除父 Session 时，子 Agent Session 的 Span 与正文在同一事务里清扫。
- Trace 页面还不用这两列跳转，提议见 [从 Trace 跳到子 Agent](../../proposed/feature/2026-08-08-trace-sub-agent-navigation.md)。
