# Agent Note: 从 Trace 跳到子 Agent

Status: proposed

## 问题

子 Agent 是独立的 Session，它的 Turn 有自己的 Trace。父 Trace 里只有一个 `spawn_agent` Tool Span，看不到子 Agent 做了什么。

关联信息已经存在：`sessions.parent_session_id` 记录父 Session，`sessions.spawn_span_id` 记录发起它的 Tool Span。但 Desktop 的 Trace 页面不读这两列。排查一次用了子 Agent 的 Turn 时，用户要自己找到子 Session，再打开它的 Trace。

## 提议

- 在 `spawn_agent` Tool Span 的详情里，列出 `spawn_span_id` 等于该 Span id 的子 Agent Session，并能打开它们的 Trace。
- 在子 Agent 的 Trace 详情里，显示父 Session，并能跳回发起它的 Tool Span。
- Core 提供按 `spawn_span_id` 查询子 Agent Session 的方法。
- 不改变 Trace 的结构。现状见 [trace.md §14](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**让子 Agent 的根 Span 用 `parent_span_id` 指向父 Trace。** 这样时间线可以直接画出父子关系。没有采用：完整度会把它判成 orphan，理由见 [子 Agent 的 Trace 在自己的 Session 里](../../implemented/architecture/2026-08-08-sub-agent-trace-in-own-session.md)。

## 验收条件

- 打开父 Turn 的 Trace，在 `spawn_agent` Tool Span 上能跳到子 Agent 的 Trace。
- 打开子 Agent 的 Trace，能跳回父 Turn 的 Trace，并定位到发起它的 Tool Span。
- `spawn_span_id` 指向的 Span 没有落库时，仍能从子 Session 跳到父 Session，界面说明找不到发起它的 Span。

## 风险

- `spawn_span_id` 没有外键，可能指向不存在的 Span。跳转必须处理找不到的情况。
- 一个子 Agent Session 可以有多个 Turn，`followup_task` 会在同一 Session 里开新 Turn。跳转目标要列出全部 Turn，而不是只给一个。
