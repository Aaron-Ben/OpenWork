# Agent Note: tracing 与 OTLP 出口

Status: proposed

## 问题

领域 Trace 存在本地 PostgreSQL，只能在 OpenWork 的 Trace 页面查看。用户如果已经有自己的 observability 工具，无法把 OpenWork 的调用接进去。

另一方面，`openwork-core` 依赖 Rust `tracing`，但只用它输出运行日志。Model Call、Tool Call 与压缩没有对应的 `tracing::Span`。项目没有 OpenTelemetry 依赖。

## 提议

- 用同一个生命周期 Guard 同时更新领域 Trace 与 `tracing::Span`。`ModelCallTraceGuard`、`ToolCallTraceGuard`、`CompactionTraceGuard` 在开始与结束时各多发一次 `tracing` 事件（`crates/openwork-core/src/session/trace.rs`）。
- OTLP 出口作为可选的 `tracing` Subscriber，默认关闭。
- 出口失败不影响 PostgreSQL 中的领域 Trace，也不影响 Turn。
- 领域 Trace 的现状见 [trace.md §1](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**用 OTLP 替代 PostgreSQL 中的领域 Trace。** 没有采用：`attempt_count`、`denied`、`outcome_unknown`、完整度是 OpenWork 的领域语义，正文与标注也要留在本机。三者各有职责，不能互相替代。

**由 Subscriber 从日志事件反推领域值。** 没有采用：日志事件不保证完整，也不保证顺序。领域值由类型化 Guard 显式写入，理由见 [Trace 是质量追踪](../../implemented/architecture/2026-07-27-trace-quality-tracking.md)。

## 验收条件

- 出口关闭时，行为与现在相同，没有额外的网络连接。
- 出口开启后，每个 Model Call、Tool Call 与 Compaction 各产生一个 `tracing::Span`，`trace_id` 与父子关系与领域 Trace 一致。
- 出口关闭、丢弃或导出失败时，PostgreSQL 中的 Trace 与 Turn 结果不变。
- 正文不经出口发出。

## 风险

- OTel 的父 Span 通常在时间上包含子 Span，领域 Trace 的父子关系表示发起关系。两者语义不同，接收端的瀑布图可能显示 Tool Span 在父 Span 之外。
- 出口把数据发出本机。属性里有路径、模型名与错误信息，开启前要让用户知道发出了什么。
