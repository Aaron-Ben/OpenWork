# Agent Note: Trace 是质量追踪，不是运维埋点

Status: implemented

## 问题

早期的 Trace 记录字节数、重试轨迹和持久化耗时。它回答“runtime 是不是在正常工作”。

这个定位有两处问题。第一，运维型 trace 的价值来自规模：很多用户同时运行，才需要知道哪个 Provider 不稳定。OpenWork 是本地单机应用，没有这种规模。第二，它答不了用户真正会问的问题：“这次回答为什么不对”。字节数和重试次数回答不了这个问题。

## 决策

- Trace 回答四件事：模型看到了什么、回复了什么、用了多少 token、人怎么评价。范围见 [trace.md §1](../../../../docs/subsystems/trace.md)。
- 正文是质量追踪的主体。组装后的请求、System Context、工具定义与没有 Message 的响应写进 `trace_payloads` / `trace_span_payloads`（[trace.md §5](../../../../docs/subsystems/trace.md)）。
- Recorder 始终记录全部正文槽位，没有记录档位，也没有运行时切换（`crates/openwork-core/src/session/trace.rs` 的 `TracePayloads`）。
- 领域 Trace 存在 PostgreSQL。`attempt_count`、`denied`、`outcome_unknown`、完整度由类型化 Guard 显式写入，不从日志推导。

## 考虑过的方案

**保持运维定位。** 没有采用，理由见“问题”一节。逐次传输轨迹 `attempts[]` 是这个定位留下的最后一块内容，后来也删除了，见 [Trace 结构增长的门槛](2026-07-27-trace-schema-growth-gates.md)。

**正文按档位记录，并在运行时切换。** 曾有 `TraceContentPolicy` 与对应的切换路径。没有保留：正文不出本机，数据库在用户自己的机器上。云端 observability 服务因为内容会离开用户环境，通常默认不记录内容（未确认具体厂商的默认值）。OpenWork 没有这个约束。

**不建正文表。** 早期设计认为 Trace 不该处理内容。定位改变后，这一条作废，因为内容正是质量追踪的主体。它有一半判断仍然成立：内容不能放进 `trace_spans` 的行里。所以正文有独立的两张表，并按需读取。

**由 tracing Subscriber 反推领域值。** 没有采用：`denied`、`outcome_unknown`、完整度是领域语义，日志事件不保证完整，也不保证顺序。Rust `tracing` 只输出运行日志。

## 后果

- 正文让 Trace 第一次体积无界，必须有截断、按天保留与孤儿清扫，见 [正文的存储与清扫](2026-07-27-trace-payload-storage-and-cleanup.md)。
- 删除 Session 后，它的源码内容必须从库里消失。这成为隐私要求。
- 排查“哪个 Provider 在抖”时数据不足。这是有意接受的代价。
- OTLP 出口的设计见 [tracing 与 OTLP 出口](../../proposed/architecture/2026-07-27-trace-tracing-otlp-export.md)。
