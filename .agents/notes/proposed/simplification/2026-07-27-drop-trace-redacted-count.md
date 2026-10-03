# Agent Note: 删除 trace_span_payloads.redacted_count

Status: proposed

## 问题

`trace_span_payloads` 有一列 `redacted_count`，本意是记录“这份正文里剔除了几处敏感字段”。

Trace 有意记录用户的私有代码，不对正文做内容扫描。正文本来就不含 API Key、凭证、HTTP Header 与 Provider 错误 Body，因为 `TracePayloads` 只取消息、工具定义与 System Context。没有任何路径会让这一列非零：写入时硬编码为 `0`（`crates/openwork-core/src/storage/trace.rs` 的 `write_payload`）。

这一列仍出现在查询结果 `TraceSpanPayloadRecord` 与 Desktop 的类型里。读 schema 的人会以为存在一套脱敏机制。

## 提议

- 用一个新迁移删除 `redacted_count` 列与约束 `trace_span_payloads_redacted_non_negative`。
- 从 `write_payload`、`get_span_payload`、`TraceSpanPayloadRecord` 与 Desktop 的 `RuntimeTraceSpanPayload` 中删除这个字段。
- 在同一个改动里更新 [trace.md §4](../../../../docs/subsystems/trace.md) 与 `docs/data-model.md`，并把本 Note 改为 implemented。

## 考虑过的方案

**保留这一列，等以后需要脱敏时再用。** 没有采用：Trace 的定位是在本机记录完整内容，脱敏不在计划里。投机保留的列会误导读者。

**等下次因别的原因修改基线迁移时再删。** `docs/data-model.md` 原定这样做，不为它单独改一次 schema。基线迁移已经在用户的库上执行，修改它不会生效，所以删除只能用新迁移。

## 验收条件

- 迁移后，`trace_span_payloads` 没有 `redacted_count` 列。
- `get_span_payload` 的结果与 Desktop 类型里没有这个字段。
- 现有的正文测试在删除后通过：去重、截断、清扫与保留。

## 风险

- 删除列需要一次迁移，旧版本的 Desktop 读到新库时少一个字段。项目不保留向后兼容，前后端同时更新即可。
- 测试中断言 `redacted_count == 0` 的地方要一起删除。
